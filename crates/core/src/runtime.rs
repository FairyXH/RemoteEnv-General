use crate::collector::CollectorEvent;
use crate::config::ClientConfig;
use crate::dispatcher::{DispatcherError, DispatcherSupervisor, UploadDispatcher};
use crate::protocol::EnvironmentEnvelope;
use crate::queue::{QueueError, UploadQueue};
use crate::state::{StateError, StateStore};
use crate::transport::ConnectionState;
use crate::worker::ServerWorkerStatus;
use serde::Serialize;
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CollectorStatus {
    NotImplemented,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeStatus {
    pub connection: ConnectionState,
    pub wifi: CollectorStatus,
    pub ble: CollectorStatus,
    pub classic_bluetooth: CollectorStatus,
    pub pending: usize,
    pub in_flight: usize,
    pub blocked: usize,
    pub uploaded: u64,
    pub failed: u64,
    pub servers: Vec<ServerWorkerStatus>,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            wifi: CollectorStatus::NotImplemented,
            ble: CollectorStatus::NotImplemented,
            classic_bluetooth: CollectorStatus::NotImplemented,
            pending: 0,
            in_flight: 0,
            blocked: 0,
            uploaded: 0,
            failed: 0,
            servers: Vec::new(),
        }
    }
}

pub struct Runtime {
    device_id: String,
    store: StateStore,
    queue: UploadQueue,
    uploaded: u64,
    failed: u64,
}

pub struct RuntimeSupervisor {
    events: mpsc::Sender<CollectorEvent>,
    config_updates: mpsc::Sender<ClientConfig>,
    status: watch::Receiver<RuntimeStatus>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl RuntimeSupervisor {
    pub fn start(config: ClientConfig, store: StateStore) -> Result<Self, RuntimeError> {
        config.validate().map_err(RuntimeError::Configuration)?;
        let dispatcher = UploadDispatcher::new(store.clone());
        if dispatcher.resolve_targets(&config).is_empty() {
            return Err(RuntimeError::Dispatcher(DispatcherError::NoTargets));
        }
        let (events, mut event_rx) = mpsc::channel::<CollectorEvent>(128);
        let (config_updates, mut config_rx) = mpsc::channel::<ClientConfig>(16);
        let (status_tx, status) = watch::channel(RuntimeStatus::default());
        let (stop, mut stop_rx) = tokio::sync::oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("remote-env-runtime".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
                runtime.block_on(async move {
                    let dispatcher = UploadDispatcher::new(store.clone());
                    let mut supervisor = DispatcherSupervisor::new(dispatcher.clone(), config.identity.clone(), Duration::from_secs(config.heartbeat_interval_seconds));
                    supervisor.apply_config(&config).await;
                    let mut current_config = config;
                    let mut status_tick = tokio::time::interval(Duration::from_millis(20));
                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => break,
                            Some(event) = event_rx.recv() => {
                                let Ok(sequence) = store.next_sequence(&current_config.identity.device_id, &event.data_type) else { continue; };
                                let envelope = EnvironmentEnvelope::new(current_config.identity.device_id.clone(), event.data_type, sequence, event.data);
                                let _ = dispatcher.persist_event(&current_config, &envelope);
                            }
                            Some(updated_config) = config_rx.recv() => {
                                supervisor.apply_config(&updated_config).await;
                                current_config = updated_config;
                            }
                            _ = status_tick.tick() => {
                                let servers = supervisor.statuses();
                                let mut snapshot = RuntimeStatus::default();
                                snapshot.connection = if servers.iter().any(|s| s.connection == ConnectionState::Ready) { ConnectionState::Ready } else { ConnectionState::Reconnecting };
                                snapshot.servers = servers;
                                snapshot.pending = snapshot.servers.iter().map(|s| s.pending).sum();
                                snapshot.in_flight = snapshot.servers.iter().map(|s| s.in_flight).sum();
                                snapshot.blocked = snapshot.servers.iter().map(|s| s.blocked).sum();
                                let _ = status_tx.send(snapshot);
                            }
                        }
                    }
                    supervisor.stop().await;
                    let mut snapshot = RuntimeStatus::default();
                    snapshot.connection = ConnectionState::Stopped;
                    let _ = status_tx.send(snapshot);
                });
            })
            .map_err(|error| RuntimeError::Thread(error.to_string()))?;
        Ok(Self {
            events,
            config_updates,
            status,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    pub fn submit(&self, event: CollectorEvent) -> Result<(), RuntimeError> {
        self.events
            .try_send(event)
            .map_err(|_| RuntimeError::EventChannelClosed)
    }

    pub fn status(&self) -> RuntimeStatus {
        self.status.borrow().clone()
    }

    pub fn update_config(&self, config: ClientConfig) -> Result<(), RuntimeError> {
        self.config_updates
            .blocking_send(config)
            .map_err(|_| RuntimeError::EventChannelClosed)
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RuntimeSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Runtime {
    pub fn new(device_id: impl Into<String>, store: StateStore, max_queue_size: usize) -> Self {
        Self {
            device_id: device_id.into(),
            queue: UploadQueue::new(store.clone(), max_queue_size),
            store,
            uploaded: 0,
            failed: 0,
        }
    }

    pub fn submit_event(
        &mut self,
        event: CollectorEvent,
    ) -> Result<EnvironmentEnvelope, RuntimeError> {
        let sequence = self
            .store
            .next_sequence(&self.device_id, &event.data_type)?;
        let envelope = EnvironmentEnvelope::new(
            self.device_id.clone(),
            event.data_type,
            sequence,
            event.data,
        );
        self.queue.enqueue(&envelope)?;
        Ok(envelope)
    }

    pub fn queue(&self) -> &UploadQueue {
        &self.queue
    }
    pub fn record_uploaded(&mut self) {
        self.uploaded = self.uploaded.saturating_add(1);
    }
    pub fn record_failed(&mut self) {
        self.failed = self.failed.saturating_add(1);
    }

    pub fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        Ok(RuntimeStatus {
            connection: ConnectionState::Disconnected,
            wifi: CollectorStatus::NotImplemented,
            ble: CollectorStatus::NotImplemented,
            classic_bluetooth: CollectorStatus::NotImplemented,
            pending: self.queue.pending_count()?,
            in_flight: self.queue.in_flight_count()?,
            blocked: self.queue.blocked_count()?,
            uploaded: self.uploaded,
            failed: self.failed,
            servers: Vec::new(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("dispatcher error: {0}")]
    Dispatcher(#[from] DispatcherError),
    #[error("queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("event channel is closed")]
    EventChannelClosed,
    #[error("runtime thread failed to start: {0}")]
    Thread(String),
}
