use crate::collector::CollectorEvent;
use crate::config::ClientConfig;
use crate::protocol::EnvironmentEnvelope;
use crate::queue::{QueueError, UploadQueue};
use crate::state::{StateError, StateStore};
use crate::transport::{ConnectionState, WebSocketManager};
use serde::Serialize;
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CollectorStatus {
    NotImplemented,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    status: watch::Receiver<RuntimeStatus>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl RuntimeSupervisor {
    pub fn start(config: ClientConfig, store: StateStore) -> Result<Self, RuntimeError> {
        config.validate().map_err(RuntimeError::Configuration)?;
        let (events, mut event_rx) = mpsc::channel::<CollectorEvent>(128);
        let (status_tx, status) = watch::channel(RuntimeStatus::default());
        let (stop, mut stop_rx) = tokio::sync::oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("remote-env-runtime".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
                runtime.block_on(async move {
                    let queue = UploadQueue::new(store.clone(), config.max_queue_size as usize);
                    let mut manager = WebSocketManager::new(Duration::from_secs(config.heartbeat_interval_seconds));
                    let identity = config.identity.clone();
                    let mut snapshot = RuntimeStatus::default();
                    let event_queue = queue.clone();
                    let event_store = store.clone();
                    let event_identity = identity.clone();
                    let event_task = tokio::spawn(async move {
                        while let Some(event) = event_rx.recv().await {
                            let Ok(sequence) = event_store.next_sequence(&event_identity.device_id, &event.data_type) else { continue; };
                            let envelope = EnvironmentEnvelope::new(event_identity.device_id.clone(), event.data_type, sequence, event.data);
                            let _ = event_queue.enqueue(&envelope);
                        }
                    });
                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => {
                                manager.stop();
                                event_task.abort();
                                snapshot.connection = ConnectionState::Stopped;
                                let _ = status_tx.send(snapshot);
                                break;
                            }
                            result = manager.run_once(&config.server_url, &config.token, &identity, &queue) => {
                                if result.is_ok() { snapshot.uploaded = snapshot.uploaded.saturating_add(1); }
                                else {
                                    snapshot.failed = snapshot.failed.saturating_add(1);
                                    let _ = queue.recover_in_flight();
                                }
                                manager.mark_reconnecting();
                                snapshot.connection = ConnectionState::Reconnecting;
                                let _ = status_tx.send(snapshot);
                                let delay = manager.next_reconnect_delay();
                                tokio::select! {
                                    _ = tokio::time::sleep(delay) => {}
                                    _ = &mut stop_rx => {
                                        manager.stop();
                                        snapshot.connection = ConnectionState::Stopped;
                                        let _ = status_tx.send(snapshot);
                                        event_task.abort();
                                        break;
                                    }
                                }
                            }
                        }
                        snapshot.connection = manager.state;
                        snapshot.pending = queue.pending_count().unwrap_or(0);
                        snapshot.in_flight = queue.in_flight_count().unwrap_or(0);
                        snapshot.blocked = queue.blocked_count().unwrap_or(0);
                        let _ = status_tx.send(snapshot);
                    }
                });
            })
            .map_err(|error| RuntimeError::Thread(error.to_string()))?;
        Ok(Self {
            events,
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
        *self.status.borrow()
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
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("event channel is closed")]
    EventChannelClosed,
    #[error("runtime thread failed to start: {0}")]
    Thread(String),
}
