use crate::collector::CollectorEvent;
use crate::config::ClientConfig;
use crate::dispatcher::{DispatcherError, DispatcherSupervisor, UploadDispatcher};
use crate::protocol::EnvironmentEnvelope;
use crate::queue::{QueueError, UploadQueue};
use crate::state::{StateError, StateStore};
use crate::transport::ConnectionState;
use crate::worker::ServerWorkerStatus;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

pub type CollectorScan = std::sync::Arc<dyn Fn() -> Result<CollectorEvent, String> + Send + Sync>;

fn persist_latest_events(
    store: &StateStore,
    dispatcher: &UploadDispatcher,
    config: &ClientConfig,
    latest_events: &mut HashMap<String, CollectorEvent>,
) -> Result<(), RuntimeError> {
    if config.wifi_enabled && config.bluetooth_enabled {
        let (Some(wifi), Some(bluetooth)) = (
            latest_events.get("wifi").cloned(),
            latest_events.get("bluetooth").cloned(),
        ) else {
            return Ok(());
        };
        let max_skew_ms = 15_000_i64;
        if (wifi.timestamp_ms - bluetooth.timestamp_ms).abs() > max_skew_ms {
            if wifi.timestamp_ms < bluetooth.timestamp_ms {
                latest_events.remove("wifi");
            } else {
                latest_events.remove("bluetooth");
            }
            return Ok(());
        }
        latest_events.remove("wifi");
        latest_events.remove("bluetooth");
        let device_id = config.identity.device_id.clone();
        let captured_at_ms = wifi.timestamp_ms.max(bluetooth.timestamp_ms);
        let sequence = store.next_timestamp_sequence(&device_id, "bluetooth")?;
        let envelope = EnvironmentEnvelope::with_timestamp(
            device_id,
            "bluetooth",
            captured_at_ms,
            sequence,
            serde_json::json!({
                "captured_at_ms": captured_at_ms,
                "devices": bluetooth.data["devices"],
                "technology": bluetooth.data["technology"].clone(),
                "wifi": wifi.data,
                "bluetooth": bluetooth.data,
            }),
        );
        dispatcher.persist_event(config, &envelope)?;
        return Ok(());
    }
    for (_, event) in latest_events.drain() {
        let device_id = config.identity.device_id.clone();
        let timestamp = event.timestamp_ms.max(1);
        let sequence = store.next_timestamp_sequence(&device_id, &event.data_type)?;
        let envelope = EnvironmentEnvelope::with_timestamp(
            device_id,
            event.data_type,
            timestamp,
            sequence,
            event.data,
        );
        dispatcher.persist_event(config, &envelope)?;
    }
    Ok(())
}

#[derive(Clone)]
struct CollectorControl {
    commands: mpsc::UnboundedSender<CollectorCommand>,
}

enum CollectorCommand {
    Configure { enabled: bool, interval: Duration },
}

enum RuntimeCommand {
    Config(ClientConfig),
    Collection { config: ClientConfig, running: bool },
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
        let (commands, mut command_rx) = mpsc::unbounded_channel();
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
                if enabled {
                    // Start the first scan as soon as collection is enabled.
                    ticker.reset_immediately();
                }
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
                                let was_enabled = enabled;
                                enabled = next;
                                ticker = tokio::time::interval(next_interval);
                                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                                if enabled && !was_enabled {
                                    ticker.reset_immediately();
                                }
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
            .send(CollectorCommand::Configure { enabled, interval });
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
        let (commands, mut command_rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new().name("remote-env-bluetooth-worker".into()).spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut enabled = enabled; let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut total = 0; let mut successful = 0; let mut failed = 0;
                if enabled {
                    ticker.reset_immediately();
                }
                loop {
                    tokio::select! {
                        _ = ticker.tick(), if enabled => {
                            if stop_flag.load(Ordering::Acquire) { break; }
                            total += 1; let started = std::time::Instant::now();
                            let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Scanning, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(event))) => { successful += 1; let items = event.data["devices"].as_array(); let count = items.map_or(0, Vec::len); let ble = items.map_or(0, |v| v.iter().filter(|x| matches!(x["mode"].as_str(), Some("ble") | Some("dual"))).count()); let classic = items.map_or(0, |v| v.iter().filter(|x| matches!(x["mode"].as_str(), Some("classic") | Some("dual"))).count()); let now = now_ms(); let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Ready, ble_device_count: ble, classic_device_count: classic, device_count: Some(count), last_scan_ms: Some(now), last_successful_scan_ms: Some(now), last_error: None, total_scans: total, successful_scans: successful, failed_scans: failed, duration_ms: Some(started.elapsed().as_millis() as u64) }).await; let _ = events.send(event).await; }
                                Ok(Ok(Err(error))) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                                Ok(Err(error)) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                                Err(error) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; }
                            }
                        }
                        Some(CollectorCommand::Configure { enabled: next, interval: next_interval }) = command_rx.recv() => {
                            let was_enabled = enabled;
                            enabled = next;
                            ticker = tokio::time::interval(next_interval);
                            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                            if enabled && !was_enabled {
                                ticker.reset_immediately();
                            }
                            if !enabled {
                                let _ = statuses.send(BluetoothRuntimeStatus { enabled: false, state: WiFiRuntimeState::Disabled, ..Default::default() }).await;
                            }
                        }
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
            .send(CollectorCommand::Configure { enabled, interval });
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
    pub collection_running: bool,
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
    pub wifi_snapshot: Option<serde_json::Value>,
    pub bluetooth_snapshot: Option<serde_json::Value>,
    pub servers: Vec<ServerWorkerStatus>,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            collection_running: false,
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
            wifi_snapshot: None,
            bluetooth_snapshot: None,
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
    commands: mpsc::UnboundedSender<RuntimeCommand>,
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
        let (commands, mut command_rx) = mpsc::unbounded_channel::<RuntimeCommand>();
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
                    let mut supervisor = DispatcherSupervisor::new(dispatcher.clone(), config.identity.clone(), Duration::from_secs(5));
                    supervisor.apply_config(&config).await;
                    for profile in config.selected_servers() {
                        let _ = dispatcher.unblock_target(&profile.id);
                        let _ = dispatcher.cancel_target_except_device(&profile.id, &profile.device_id);
                    }
                    let mut current_config = config;
                    let mut collection_running = false;
                    let mut status_tick = tokio::time::interval(Duration::from_millis(20));
                    let mut upload_tick = tokio::time::interval(Duration::from_millis(100));
                    upload_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let mut latest_events: HashMap<String, CollectorEvent> = HashMap::new();
                    let mut wifi_snapshot: Option<serde_json::Value> = None;
                    let mut bluetooth_snapshot: Option<serde_json::Value> = None;
                    let worker = scan.map(|scan| PeriodicCollectorHandle::start(scan, false, Duration::from_secs(current_config.scan_interval_seconds), events_for_worker.clone(), wifi_status_tx));
                    let bluetooth_worker = bluetooth_scan.map(|scan| BluetoothWorkerHandle::start(scan, false, Duration::from_secs(current_config.scan_interval_seconds), events_for_bluetooth_worker.clone(), bluetooth_status_tx));
                    let mut wifi_runtime = WiFiRuntimeStatus::default();
                    let mut bluetooth_runtime = BluetoothRuntimeStatus::default();
                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => break,
                            Some(event) = event_rx.recv() => {
                                if event.data_type == "wifi" {
                                    wifi_snapshot = Some(event.data.clone());
                                } else if event.data_type == "bluetooth" {
                                    bluetooth_snapshot = Some(event.data.clone());
                                }
                                latest_events.insert(event.data_type.clone(), event);
                                if collection_running {
                                    if let Err(error) = persist_latest_events(&store, &dispatcher, &current_config, &mut latest_events) {
                                        eprintln!("runtime event persistence failed: {error}");
                                    }
                                }
                            }
                            _ = upload_tick.tick(), if collection_running => {
                                if let Err(error) = persist_latest_events(&store, &dispatcher, &current_config, &mut latest_events) {
                                    eprintln!("runtime upload persistence failed: {error}");
                                }
                            }
                            Some(command) = command_rx.recv() => {
                                let (updated_config, next_running) = match command {
                                    RuntimeCommand::Config(updated_config) => (updated_config, collection_running),
                                    RuntimeCommand::Collection { config: updated_config, running } => (updated_config, running),
                                };
                                supervisor.apply_config(&updated_config).await;
                                for profile in updated_config.selected_servers() {
                                    let _ = dispatcher.unblock_target(&profile.id);
                                    let _ = dispatcher.cancel_target_except_device(&profile.id, &profile.device_id);
                                }
                                if let Some(worker) = worker.as_ref() { worker.configure(updated_config.wifi_enabled && next_running, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                if let Some(worker) = bluetooth_worker.as_ref() { worker.configure(updated_config.bluetooth_enabled && next_running, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                upload_tick = tokio::time::interval(Duration::from_millis(updated_config.upload_interval_seconds.saturating_mul(1000).max(100)));
                                upload_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                                current_config = updated_config;
                                collection_running = next_running;
                                if collection_running {
                                    if let Err(error) = persist_latest_events(&store, &dispatcher, &current_config, &mut latest_events) {
                                        eprintln!("runtime start persistence failed: {error}");
                                    }
                                }
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
                                snapshot.collection_running = collection_running;
                                snapshot.connection = servers
                                    .iter()
                                    .find(|s| s.connection == ConnectionState::Ready)
                                    .map(|s| s.connection)
                                    .or_else(|| servers.iter().find(|s| s.connection == ConnectionState::Authenticating).map(|s| s.connection))
                                    .or_else(|| servers.iter().find(|s| s.connection == ConnectionState::Connecting).map(|s| s.connection))
                                    .or_else(|| servers.iter().find(|s| s.connection == ConnectionState::Reconnecting).map(|s| s.connection))
                                    .unwrap_or_else(|| if servers.is_empty() { ConnectionState::Stopped } else { ConnectionState::Connecting });
                                snapshot.servers = servers;
                                snapshot.pending = snapshot.servers.iter().map(|s| s.pending).sum();
                                snapshot.in_flight = snapshot.servers.iter().map(|s| s.in_flight).sum();
                                snapshot.blocked = snapshot.servers.iter().map(|s| s.blocked).sum();
                                snapshot.uploaded = snapshot.servers.iter().map(|s| s.uploaded).sum();
                                snapshot.failed = snapshot.servers.iter().map(|s| s.failed).sum();
                                snapshot.wifi_snapshot = wifi_snapshot.clone();
                                snapshot.bluetooth_snapshot = bluetooth_snapshot.clone();
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
            commands,
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

    pub fn status_has_changed(&mut self) -> bool {
        self.status.has_changed().unwrap_or(false)
    }

    pub fn update_config(&self, config: ClientConfig) -> Result<(), RuntimeError> {
        self.commands
            .send(RuntimeCommand::Config(config))
            .map_err(|_| RuntimeError::EventChannelClosed)
    }

    pub fn set_collection_running(
        &self,
        config: ClientConfig,
        running: bool,
    ) -> Result<(), RuntimeError> {
        self.commands
            .send(RuntimeCommand::Collection { config, running })
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
            .next_timestamp_sequence(&self.device_id, &event.data_type)?;
        let envelope = EnvironmentEnvelope::with_timestamp(
            self.device_id.clone(),
            event.data_type,
            event.timestamp_ms.max(1),
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
            collection_running: false,
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
            wifi_snapshot: None,
            bluetooth_snapshot: None,
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
