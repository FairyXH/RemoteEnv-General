use crate::collector::CollectorEvent;
use crate::config::ClientConfig;
use crate::dispatcher::{DispatcherError, DispatcherSupervisor, UploadDispatcher};
use crate::protocol::EnvironmentEnvelope;
use crate::queue::{QueueError, UploadQueue};
use crate::state::{StateError, StateStore};
use crate::transport::ConnectionState;
use crate::worker::ServerWorkerStatus;
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub type CollectorScan = std::sync::Arc<dyn Fn() -> Result<CollectorEvent, String> + Send + Sync>;

#[derive(Clone)]
struct CollectorControl {
    commands: mpsc::Sender<CollectorCommand>,
}

enum CollectorCommand {
    Configure { enabled: bool, interval: Duration },
}

struct PeriodicCollectorHandle {
    control: CollectorControl,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PeriodicCollectorHandle {
    fn start(
        scan: CollectorScan,
        enabled: bool,
        interval: Duration,
        events: mpsc::Sender<CollectorEvent>,
        statuses: mpsc::Sender<WiFiRuntimeStatus>,
    ) -> Self {
        let (commands, mut command_rx) = mpsc::channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("remote-env-wifi-worker".into())
            .spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut enabled = enabled;
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut total = 0;
                let mut successful = 0;
                let mut failed = 0;
                let _ = statuses.send(WiFiRuntimeStatus { enabled, state: if enabled { WiFiRuntimeState::Starting } else { WiFiRuntimeState::Disabled }, ..Default::default() }).await;
                if enabled { ticker.reset(); }
                loop {
                    tokio::select! {
                        _ = ticker.tick(), if enabled => {
                            if stop_for_thread.load(Ordering::Acquire) { break; }
                            total += 1;
                            let _ = statuses.send(WiFiRuntimeStatus { enabled, state: WiFiRuntimeState::Scanning, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                            let started = std::time::Instant::now();
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(event))) => {
                                    successful += 1;
                                    let count = event.data["networks"].as_array().map_or(0, Vec::len);
                                    let now = now_ms();
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled, state: WiFiRuntimeState::Ready, last_scan_ms: Some(now), last_successful_scan_ms: Some(now), network_count: Some(count), total_scans: total, successful_scans: successful, failed_scans: failed, duration_ms: Some(started.elapsed().as_millis() as u64), ..Default::default() }).await;
                                    let _ = events.send(event).await;
                                }
                                Ok(Ok(Err(error))) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error), ..Default::default() }).await;
                                }
                                Ok(Err(error)) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error.to_string()), ..Default::default() }).await;
                                }
                                Err(error) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error.to_string()), ..Default::default() }).await;
                                }
                            }
                        }
                        Some(command) = command_rx.recv() => match command {
                            CollectorCommand::Configure { enabled: next, interval: next_interval } => {
                                enabled = next;
                                ticker = tokio::time::interval(next_interval);
                                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                                ticker.tick().await;
                                if !enabled {
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled: false, state: WiFiRuntimeState::Disabled, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                                }
                            }
                        },
                        _ = tokio::time::sleep(Duration::from_millis(20)) => {
                            if stop_for_thread.load(Ordering::Acquire) { break; }
                        },
                    }
                }
                let _ = statuses.send(WiFiRuntimeStatus { state: WiFiRuntimeState::Stopped, ..Default::default() }).await;
            });
        }).expect("Wi-Fi worker thread failed to start");
        Self {
            control: CollectorControl { commands },
            stop,
            thread: Some(thread),
        }
    }
    fn configure(&self, enabled: bool, interval: Duration) {
        let _ = self
            .control
            .commands
            .try_send(CollectorCommand::Configure { enabled, interval });
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct BluetoothWorkerHandle {
    control: CollectorControl,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BluetoothWorkerHandle {
    fn start(
        scan: CollectorScan,
        enabled: bool,
        interval: Duration,
        events: mpsc::Sender<CollectorEvent>,
        statuses: mpsc::Sender<BluetoothRuntimeStatus>,
    ) -> Self {
        let (commands, mut command_rx) = mpsc::channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new().name("remote-env-bluetooth-worker".into()).spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut enabled = enabled; let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut total = 0; let mut successful = 0; let mut failed = 0;
                if enabled { ticker.reset(); }
                loop {
                    tokio::select! {
                        _ = ticker.tick(), if enabled => {
                            if stop_flag.load(Ordering::Acquire) { break; }
                            total += 1; let started = std::time::Instant::now();
                            let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Scanning, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(event))) => { successful += 1; let items = event.data["observations"].as_array(); let count = items.map_or(0, Vec::len); let ble = items.map_or(0, |v| v.iter().filter(|x| matches!(x["transport"].as_str(), Some("ble") | Some("dual"))).count()); let classic = items.map_or(0, |v| v.iter().filter(|x| matches!(x["transport"].as_str(), Some("classic") | Some("dual"))).count()); let now = now_ms(); let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Ready, ble_device_count: ble, classic_device_count: classic, device_count: Some(count), last_scan_ms: Some(now), last_successful_scan_ms: Some(now), last_error: None, total_scans: total, successful_scans: successful, failed_scans: failed, duration_ms: Some(started.elapsed().as_millis() as u64) }).await; let _ = events.send(event).await; }
                                Ok(Ok(Err(error))) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                                Ok(Err(error)) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                                Err(error) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                            }
                        }
                        Some(CollectorCommand::Configure { enabled: next, interval: next_interval }) = command_rx.recv() => { enabled = next; ticker = tokio::time::interval(next_interval); ticker.tick().await; if !enabled { let _ = statuses.send(BluetoothRuntimeStatus { state: WiFiRuntimeState::Disabled, ..Default::default() }).await; } }
                        _ = tokio::time::sleep(Duration::from_millis(20)) => if stop_flag.load(Ordering::Acquire) { break; },
                    }
                }
            });
        }).expect("Bluetooth worker thread failed to start");
        Self {
            control: CollectorControl { commands },
            stop,
            thread: Some(thread),
        }
    }
    fn configure(&self, enabled: bool, interval: Duration) {
        let _ = self
            .control
            .commands
            .try_send(CollectorCommand::Configure { enabled, interval });
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CollectorStatus {
    NotImplemented,
    Disabled,
    Starting,
    Scanning,
    Ready,
    Error,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum WiFiRuntimeState {
    Disabled,
    Starting,
    Scanning,
    Ready,
    Error,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WiFiRuntimeStatus {
    pub enabled: bool,
    pub state: WiFiRuntimeState,
    pub last_scan_ms: Option<i64>,
    pub last_successful_scan_ms: Option<i64>,
    pub network_count: Option<usize>,
    pub last_error: Option<String>,
    pub total_scans: u64,
    pub successful_scans: u64,
    pub failed_scans: u64,
    pub duration_ms: Option<u64>,
}

impl Default for WiFiRuntimeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            state: WiFiRuntimeState::Disabled,
            last_scan_ms: None,
            last_successful_scan_ms: None,
            network_count: None,
            last_error: None,
            total_scans: 0,
            successful_scans: 0,
            failed_scans: 0,
            duration_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BluetoothRuntimeStatus {
    pub enabled: bool,
    pub state: WiFiRuntimeState,
    pub ble_device_count: usize,
    pub classic_device_count: usize,
    pub device_count: Option<usize>,
    pub last_scan_ms: Option<i64>,
    pub last_successful_scan_ms: Option<i64>,
    pub last_error: Option<String>,
    pub total_scans: u64,
    pub successful_scans: u64,
    pub failed_scans: u64,
    pub duration_ms: Option<u64>,
}

impl Default for BluetoothRuntimeStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            state: WiFiRuntimeState::Disabled,
            ble_device_count: 0,
            classic_device_count: 0,
            device_count: None,
            last_scan_ms: None,
            last_successful_scan_ms: None,
            last_error: None,
            total_scans: 0,
            successful_scans: 0,
            failed_scans: 0,
            duration_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeStatus {
    pub connection: ConnectionState,
    pub wifi: CollectorStatus,
    pub wifi_runtime: WiFiRuntimeStatus,
    pub bluetooth: CollectorStatus,
    pub bluetooth_runtime: BluetoothRuntimeStatus,
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
            wifi_runtime: WiFiRuntimeStatus::default(),
            bluetooth: CollectorStatus::NotImplemented,
            bluetooth_runtime: BluetoothRuntimeStatus::default(),
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
        Self::start_with_collector(config, store, None)
    }

    pub fn start_with_collector(
        config: ClientConfig,
        store: StateStore,
        scan: Option<CollectorScan>,
    ) -> Result<Self, RuntimeError> {
        Self::start_with_collectors(config, store, scan, None)
    }

    pub fn start_with_collectors(
        config: ClientConfig,
        store: StateStore,
        scan: Option<CollectorScan>,
        bluetooth_scan: Option<CollectorScan>,
    ) -> Result<Self, RuntimeError> {
        config.validate().map_err(RuntimeError::Configuration)?;
        let dispatcher = UploadDispatcher::new(store.clone());
        if dispatcher.resolve_targets(&config).is_empty() {
            return Err(RuntimeError::Dispatcher(DispatcherError::NoTargets));
        }
        let (events, mut event_rx) = mpsc::channel::<CollectorEvent>(128);
        let (config_updates, mut config_rx) = mpsc::channel::<ClientConfig>(16);
        let (status_tx, status) = watch::channel(RuntimeStatus::default());
        let (stop, mut stop_rx) = tokio::sync::oneshot::channel();
        let (wifi_status_tx, mut wifi_status_rx) = mpsc::channel::<WiFiRuntimeStatus>(16);
        let (bluetooth_status_tx, mut bluetooth_status_rx) =
            mpsc::channel::<BluetoothRuntimeStatus>(16);
        let events_for_worker = events.clone();
        let events_for_bluetooth_worker = events.clone();
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
                    let worker = scan.map(|scan| PeriodicCollectorHandle::start(scan, current_config.wifi_enabled, Duration::from_secs(current_config.scan_interval_seconds), events_for_worker.clone(), wifi_status_tx));
                    let bluetooth_worker = bluetooth_scan.map(|scan| BluetoothWorkerHandle::start(scan, current_config.bluetooth_enabled, Duration::from_secs(current_config.scan_interval_seconds), events_for_bluetooth_worker.clone(), bluetooth_status_tx));
                    let mut wifi_runtime = WiFiRuntimeStatus::default();
                    let mut bluetooth_runtime = BluetoothRuntimeStatus::default();
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
                                if let Some(worker) = worker.as_ref() { worker.configure(updated_config.wifi_enabled, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                if let Some(worker) = bluetooth_worker.as_ref() { worker.configure(updated_config.bluetooth_enabled, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                current_config = updated_config;
                            }
                            Some(updated_wifi) = wifi_status_rx.recv() => {
                                wifi_runtime = updated_wifi;
                            }
                            Some(updated_bluetooth) = bluetooth_status_rx.recv() => {
                                bluetooth_runtime = updated_bluetooth;
                            }
                            _ = status_tick.tick() => {
                                let servers = supervisor.statuses();
                                let mut snapshot = RuntimeStatus::default();
                                snapshot.connection = if servers.iter().any(|s| s.connection == ConnectionState::Ready) { ConnectionState::Ready } else { ConnectionState::Reconnecting };
                                snapshot.servers = servers;
                                snapshot.pending = snapshot.servers.iter().map(|s| s.pending).sum();
                                snapshot.in_flight = snapshot.servers.iter().map(|s| s.in_flight).sum();
                                snapshot.blocked = snapshot.servers.iter().map(|s| s.blocked).sum();
                                snapshot.wifi_runtime = wifi_runtime.clone(); snapshot.wifi = match wifi_runtime.state { WiFiRuntimeState::Disabled => CollectorStatus::Disabled, WiFiRuntimeState::Starting => CollectorStatus::Starting, WiFiRuntimeState::Scanning => CollectorStatus::Scanning, WiFiRuntimeState::Ready => CollectorStatus::Ready, WiFiRuntimeState::Error => CollectorStatus::Error, WiFiRuntimeState::Stopped => CollectorStatus::Stopped };
                                snapshot.bluetooth_runtime = bluetooth_runtime.clone(); snapshot.bluetooth = match bluetooth_runtime.state { WiFiRuntimeState::Disabled => CollectorStatus::Disabled, WiFiRuntimeState::Starting => CollectorStatus::Starting, WiFiRuntimeState::Scanning => CollectorStatus::Scanning, WiFiRuntimeState::Ready => CollectorStatus::Ready, WiFiRuntimeState::Error => CollectorStatus::Error, WiFiRuntimeState::Stopped => CollectorStatus::Stopped };
                                let _ = status_tx.send(snapshot);
                            }
                        }
                    }
                    supervisor.stop().await;
                    if let Some(mut worker) = worker { worker.stop(); }
                    if let Some(mut worker) = bluetooth_worker { worker.stop(); }
                    let mut snapshot = RuntimeStatus::default();
                    snapshot.connection = ConnectionState::Stopped;
                    snapshot.wifi_runtime = wifi_runtime; snapshot.wifi = CollectorStatus::Stopped;
                    snapshot.bluetooth_runtime = bluetooth_runtime; snapshot.bluetooth = CollectorStatus::Stopped;
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
            .try_send(config)
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
            wifi_runtime: WiFiRuntimeStatus::default(),
            bluetooth: CollectorStatus::NotImplemented,
            bluetooth_runtime: BluetoothRuntimeStatus::default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn periodic_collector_emits_events_and_stops() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_scan = Arc::clone(&calls);
        let scan: CollectorScan = Arc::new(move || {
            calls_for_scan.fetch_add(1, Ordering::SeqCst);
            Ok(CollectorEvent {
                data_type: "wifi".into(),
                timestamp_ms: 1,
                data: serde_json::json!({ "networks": [{"bssid": "AA:BB:CC:DD:EE:FF"}] }),
            })
        });
        let (events, mut event_rx) = mpsc::channel(4);
        let (statuses, mut status_rx) = mpsc::channel(8);
        let mut worker =
            PeriodicCollectorHandle::start(scan, true, Duration::from_millis(20), events, statuses);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(event.data_type, "wifi");
            let mut ready = false;
            for _ in 0..8 {
                if let Some(status) = status_rx.recv().await {
                    ready |= status.state == WiFiRuntimeState::Ready;
                }
                if ready {
                    break;
                }
            }
            assert!(ready);
        });
        worker.stop();
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn periodic_collector_disabled_does_not_scan() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_scan = Arc::clone(&calls);
        let scan: CollectorScan = Arc::new(move || {
            calls_for_scan.fetch_add(1, Ordering::SeqCst);
            Ok(CollectorEvent {
                data_type: "wifi".into(),
                timestamp_ms: 1,
                data: serde_json::json!({}),
            })
        });
        let (events, mut event_rx) = mpsc::channel(1);
        let (statuses, _status_rx) = mpsc::channel(2);
        let mut worker = PeriodicCollectorHandle::start(
            scan,
            false,
            Duration::from_millis(10),
            events,
            statuses,
        );
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(event_rx.try_recv().is_err());
        worker.stop();
    }
}
