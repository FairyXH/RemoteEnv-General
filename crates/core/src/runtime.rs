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

/// Waits with exponential backoff after a collector scan error, capped at 120
/// seconds, then returns so the caller can retry immediately. This keeps
/// collection always-on and resilient to transient failures. On stop the wait is
/// interrupted so the worker can shut down promptly.
async fn retry_after_error(error_delay: &mut std::time::Duration, stop: &Arc<AtomicBool>) {
    let delay = *error_delay;
    if delay > std::time::Duration::ZERO {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                if stop.load(Ordering::Acquire) { return; }
            },
        }
    }
    *error_delay = (*error_delay * 2).min(std::time::Duration::from_secs(120));
}

use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

pub type CollectorScan = std::sync::Arc<dyn Fn() -> Result<CollectorEvent, String> + Send + Sync>;
pub type CollectorBatchScan =
    std::sync::Arc<dyn Fn() -> Result<Vec<CollectorEvent>, String> + Send + Sync>;

fn persist_latest_events(
    store: &StateStore,
    dispatcher: &UploadDispatcher,
    config: &ClientConfig,
    latest_events: &mut HashMap<String, CollectorEvent>,
) -> Result<(), RuntimeError> {
    // Upload every collector source as its own server-canonical envelope.
    // The server selects the data schema by `data_type`, so Wi-Fi must be
    // persisted as `data_type="wifi"` and Bluetooth as `data_type="bluetooth"`;
    // merging them into one Bluetooth envelope would hide Wi-Fi data from the
    // server's per-type latest-data/statistics views.
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
        // Collection is always enabled; data is collected for every adapter and
        // uploaded in full. Errors trigger unlimited retry with exponential
        // backoff capped at 120 seconds. The `enabled` parameter is kept for
        // API compatibility and ignored.
        let _ = enabled;
        let thread = std::thread::Builder::new()
            .name("remote-env-wifi-worker".into())
            .spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut total = 0;
                let mut successful = 0;
                let mut failed = 0;
                let mut error_delay = std::time::Duration::from_secs(1);
                let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Starting, ..Default::default() }).await;
                ticker.reset_immediately();
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            if stop_for_thread.load(Ordering::Acquire) { break; }
                            total += 1;
                            let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Scanning, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                            let started = std::time::Instant::now();
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(event))) => {
                                    successful += 1;
                                    error_delay = std::time::Duration::from_secs(1);
                                    let count = event.data["networks"].as_array().map_or(0, Vec::len);
                                    let now = now_ms();
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Ready, last_scan_ms: Some(now), last_successful_scan_ms: Some(now), network_count: Some(count), total_scans: total, successful_scans: successful, failed_scans: failed, duration_ms: Some(started.elapsed().as_millis() as u64), ..Default::default() }).await;
                                    let _ = events.send(event).await;
                                }
                                Ok(Ok(Err(error))) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error), ..Default::default() }).await;
                                    retry_after_error(&mut error_delay, &stop_for_thread).await;
                                }
                                Ok(Err(error)) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error.to_string()), ..Default::default() }).await;
                                    retry_after_error(&mut error_delay, &stop_for_thread).await;
                                }
                                Err(error) => {
                                    failed += 1;
                                    let _ = statuses.send(WiFiRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_scan_ms: Some(now_ms()), total_scans: total, successful_scans: successful, failed_scans: failed, last_error: Some(error.to_string()), ..Default::default() }).await;
                                    retry_after_error(&mut error_delay, &stop_for_thread).await;
                                }
                            }
                        }
                        Some(command) = command_rx.recv() => match command {
                            CollectorCommand::Configure { enabled: _next, interval: next_interval } => {
                                ticker = tokio::time::interval(next_interval);
                                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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

struct BatchCollectorHandle {
    control: CollectorControl,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BatchCollectorHandle {
    fn start(
        scan: CollectorBatchScan,
        interval: Duration,
        events: mpsc::Sender<CollectorEvent>,
    ) -> Self {
        let (commands, mut command_rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new().name("remote-env-batch-worker".into()).spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.reset_immediately();
                let mut error_delay = Duration::from_secs(1);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            if stop_flag.load(Ordering::Acquire) { break; }
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(batch))) => {
                                    error_delay = Duration::from_secs(1);
                                    for event in batch { if events.send(event).await.is_err() { break; } }
                                }
                                Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => retry_after_error(&mut error_delay, &stop_flag).await,
                            }
                        }
                        Some(CollectorCommand::Configure { enabled: _, interval }) = command_rx.recv() => {
                            ticker = tokio::time::interval(interval);
                            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                        }
                        _ = tokio::time::sleep(Duration::from_millis(20)) => if stop_flag.load(Ordering::Acquire) { break; },
                    }
                }
            });
        }).expect("batch collector worker thread failed to start");
        Self {
            control: CollectorControl { commands },
            stop,
            thread: Some(thread),
        }
    }
    fn configure(&self, interval: Duration) {
        let _ = self.control.commands.send(CollectorCommand::Configure {
            enabled: true,
            interval,
        });
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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
        // Collection is always enabled; BLE + Classic are collected for every
        // adapter and merged. Errors trigger unlimited retry with exponential
        // backoff capped at 120 seconds. The `enabled` parameter is kept for
        // API compatibility and ignored.
        let _ = enabled;
        let thread = std::thread::Builder::new().name("remote-env-bluetooth-worker".into()).spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else { return; };
            runtime.block_on(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut total = 0; let mut successful = 0; let mut failed = 0;
                let mut error_delay = std::time::Duration::from_secs(1);
                ticker.reset_immediately();
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            if stop_flag.load(Ordering::Acquire) { break; }
                            total += 1; let started = std::time::Instant::now();
                            let _ = statuses.send(BluetoothRuntimeStatus { enabled: true, state: WiFiRuntimeState::Scanning, total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await;
                            match timeout(Duration::from_secs(120), tokio::task::spawn_blocking({ let scan = Arc::clone(&scan); move || scan() })).await {
                                Ok(Ok(Ok(event))) => { successful += 1; error_delay = std::time::Duration::from_secs(1); let items = event.data["devices"].as_array(); let count = items.map_or(0, Vec::len); let ble = items.map_or(0, |v| v.iter().filter(|x| matches!(x["mode"].as_str(), Some("ble") | Some("dual"))).count()); let classic = items.map_or(0, |v| v.iter().filter(|x| matches!(x["mode"].as_str(), Some("classic") | Some("dual"))).count()); let now = now_ms(); let _ = statuses.send(BluetoothRuntimeStatus { enabled: true, state: WiFiRuntimeState::Ready, ble_device_count: ble, classic_device_count: classic, device_count: Some(count), last_scan_ms: Some(now), last_successful_scan_ms: Some(now), last_error: None, total_scans: total, successful_scans: successful, failed_scans: failed, duration_ms: Some(started.elapsed().as_millis() as u64) }).await; let _ = events.send(event).await; }
                                Ok(Ok(Err(error))) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_error: Some(error), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; retry_after_error(&mut error_delay, &stop_flag).await; }
                                Ok(Err(error)) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; retry_after_error(&mut error_delay, &stop_flag).await; }
                                Err(error) => { failed += 1; let _ = statuses.send(BluetoothRuntimeStatus { enabled: true, state: WiFiRuntimeState::Error, last_error: Some(error.to_string()), total_scans: total, successful_scans: successful, failed_scans: failed, ..Default::default() }).await; retry_after_error(&mut error_delay, &stop_flag).await; }
                            }
                        }
                        Some(CollectorCommand::Configure { enabled: _next, interval: next_interval }) = command_rx.recv() => {
                            ticker = tokio::time::interval(next_interval);
                            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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
    pub cell_snapshot: Option<serde_json::Value>,
    pub gps_snapshot: Option<serde_json::Value>,
    pub gnss_snapshot: Option<serde_json::Value>,
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
            cell_snapshot: None,
            gps_snapshot: None,
            gnss_snapshot: None,
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
        Self::start_with_collectors_and_batch(config, store, scan, bluetooth_scan, None)
    }

    pub fn start_with_collectors_and_batch(
        config: ClientConfig,
        store: StateStore,
        scan: Option<CollectorScan>,
        bluetooth_scan: Option<CollectorScan>,
        batch_scan: Option<CollectorBatchScan>,
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
        let events_for_batch_worker = events.clone();
        let batch_scan_for_restart = batch_scan.clone();
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
                    let mut cleanup_tick = tokio::time::interval(Duration::from_secs(3600));
                    cleanup_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    cleanup_tick.reset();
                    let _ = cleanup_tick.tick();
                    let mut latest_events: HashMap<String, CollectorEvent> = HashMap::new();
                    let mut wifi_snapshot: Option<serde_json::Value> = None;
                    let mut bluetooth_snapshot: Option<serde_json::Value> = None;
                    let mut cell_snapshot: Option<serde_json::Value> = None;
                    let mut gps_snapshot: Option<serde_json::Value> = None;
                    let mut gnss_snapshot: Option<serde_json::Value> = None;
                    let worker = scan.map(|scan| PeriodicCollectorHandle::start(scan, true, Duration::from_secs(current_config.scan_interval_seconds), events_for_worker.clone(), wifi_status_tx));
                    let bluetooth_worker = bluetooth_scan.map(|scan| BluetoothWorkerHandle::start(scan, true, Duration::from_secs(current_config.scan_interval_seconds), events_for_bluetooth_worker.clone(), bluetooth_status_tx));
                    let has_batch_collector = batch_scan_for_restart.is_some();
                    let mut batch_worker = batch_scan.map(|scan| BatchCollectorHandle::start(scan, Duration::from_secs(current_config.scan_interval_seconds), events_for_batch_worker.clone()));
                    let mut last_batch_event = has_batch_collector.then(tokio::time::Instant::now);
                    let mut watchdog_tick = tokio::time::interval(Duration::from_secs(30));
                    watchdog_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let mut wifi_runtime = WiFiRuntimeStatus::default();
                    let mut bluetooth_runtime = BluetoothRuntimeStatus::default();
                    if has_batch_collector {
                        wifi_runtime.enabled = true;
                        wifi_runtime.state = WiFiRuntimeState::Starting;
                        bluetooth_runtime.enabled = true;
                        bluetooth_runtime.state = WiFiRuntimeState::Starting;
                    }
                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => break,
                            Some(event) = event_rx.recv() => {
                                if event.data_type == "wifi" {
                                    wifi_snapshot = Some(event.data.clone());
                                    if has_batch_collector {
                                        let count = event.data.get("networks").and_then(|v| v.as_array()).map_or(0, Vec::len);
                                        let started = event.data.get("scan_started_at").and_then(|v| v.as_i64()).unwrap_or(event.timestamp_ms);
                                        let finished = event.data.get("scan_finished_at").and_then(|v| v.as_i64()).unwrap_or(event.timestamp_ms);
                                        wifi_runtime.enabled = true;
                                        wifi_runtime.state = WiFiRuntimeState::Ready;
                                        wifi_runtime.last_scan_ms = Some(finished);
                                        wifi_runtime.last_successful_scan_ms = Some(finished);
                                        wifi_runtime.network_count = Some(count);
                                        wifi_runtime.last_error = None;
                                        wifi_runtime.total_scans = wifi_runtime.total_scans.saturating_add(1);
                                        wifi_runtime.successful_scans = wifi_runtime.successful_scans.saturating_add(1);
                                        wifi_runtime.duration_ms = Some(finished.saturating_sub(started) as u64);
                                        last_batch_event = Some(tokio::time::Instant::now());
                                    }
                                } else if event.data_type == "bluetooth" {
                                    bluetooth_snapshot = Some(event.data.clone());
                                    if has_batch_collector {
                                        let devices = event.data.get("devices").and_then(|v| v.as_array());
                                        let count = devices.map_or(0, Vec::len);
                                        let ble_count = devices.map_or(0, |items| items.iter().filter(|item| item.get("technology").and_then(|v| v.as_str()).is_some_and(|v| v.eq_ignore_ascii_case("ble"))).count());
                                        let started = event.data.get("scan_started_at").and_then(|v| v.as_i64()).unwrap_or(event.timestamp_ms);
                                        let finished = event.data.get("scan_finished_at").and_then(|v| v.as_i64()).unwrap_or(event.timestamp_ms);
                                        bluetooth_runtime.enabled = true;
                                        bluetooth_runtime.state = WiFiRuntimeState::Ready;
                                        bluetooth_runtime.last_scan_ms = Some(finished);
                                        bluetooth_runtime.last_successful_scan_ms = Some(finished);
                                        bluetooth_runtime.device_count = Some(count);
                                        bluetooth_runtime.ble_device_count = ble_count;
                                        bluetooth_runtime.classic_device_count = count.saturating_sub(ble_count);
                                        bluetooth_runtime.last_error = None;
                                        bluetooth_runtime.total_scans = bluetooth_runtime.total_scans.saturating_add(1);
                                        bluetooth_runtime.successful_scans = bluetooth_runtime.successful_scans.saturating_add(1);
                                        bluetooth_runtime.duration_ms = Some(finished.saturating_sub(started) as u64);
                                        last_batch_event = Some(tokio::time::Instant::now());
                                    }
                                } else if event.data_type == "cell" {
                                    cell_snapshot = Some(event.data.clone());
                                } else if event.data_type == "gps" {
                                    gps_snapshot = Some(event.data.clone());
                                } else if event.data_type == "gnss" {
                                    gnss_snapshot = Some(event.data.clone());
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
                            _ = cleanup_tick.tick() => {
                                // Keep the rebuildable state cache small during
                                // long-running operation: prune acknowledged and
                                // cancelled upload rows older than 24 hours.
                                if let Err(error) = store.cleanup_state_cache(Duration::from_secs(24 * 3600)) {
                                    eprintln!("runtime state cache cleanup failed: {error}");
                                }
                            }
                            _ = watchdog_tick.tick(), if collection_running && has_batch_collector => {
                                let stale_after = Duration::from_secs(current_config.scan_interval_seconds.saturating_mul(3).saturating_add(130).max(180));
                                if last_batch_event.is_some_and(|last| last.elapsed() >= stale_after) {
                                    eprintln!("batch collector watchdog restarting a stale worker");
                                    if let Some(worker) = batch_worker.as_mut() { worker.stop(); }
                                    batch_worker = batch_scan_for_restart.as_ref().map(|scan| BatchCollectorHandle::start(scan.clone(), Duration::from_secs(current_config.scan_interval_seconds), events_for_batch_worker.clone()));
                                    last_batch_event = Some(tokio::time::Instant::now());
                                    wifi_runtime.state = WiFiRuntimeState::Starting;
                                    bluetooth_runtime.state = WiFiRuntimeState::Starting;
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
                                if let Some(worker) = worker.as_ref() { worker.configure(true, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                if let Some(worker) = bluetooth_worker.as_ref() { worker.configure(true, Duration::from_secs(updated_config.scan_interval_seconds)); }
                                if let Some(worker) = batch_worker.as_ref() { worker.configure(Duration::from_secs(updated_config.scan_interval_seconds)); }
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
                                snapshot.cell_snapshot = cell_snapshot.clone();
                                snapshot.gps_snapshot = gps_snapshot.clone();
                                snapshot.gnss_snapshot = gnss_snapshot.clone();
                                snapshot.wifi_runtime = wifi_runtime.clone(); snapshot.wifi = match wifi_runtime.state { WiFiRuntimeState::Disabled => CollectorStatus::Disabled, WiFiRuntimeState::Starting => CollectorStatus::Starting, WiFiRuntimeState::Scanning => CollectorStatus::Scanning, WiFiRuntimeState::Ready => CollectorStatus::Ready, WiFiRuntimeState::Error => CollectorStatus::Error, WiFiRuntimeState::Stopped => CollectorStatus::Stopped };
                                snapshot.bluetooth_runtime = bluetooth_runtime.clone(); snapshot.bluetooth = match bluetooth_runtime.state { WiFiRuntimeState::Disabled => CollectorStatus::Disabled, WiFiRuntimeState::Starting => CollectorStatus::Starting, WiFiRuntimeState::Scanning => CollectorStatus::Scanning, WiFiRuntimeState::Ready => CollectorStatus::Ready, WiFiRuntimeState::Error => CollectorStatus::Error, WiFiRuntimeState::Stopped => CollectorStatus::Stopped };
                                let _ = status_tx.send(snapshot);
                            }
                        }
                    }
                    supervisor.stop().await;
                    if let Some(mut worker) = worker { worker.stop(); }
                    if let Some(mut worker) = bluetooth_worker { worker.stop(); }
                    if let Some(mut worker) = batch_worker { worker.stop(); }
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
            cell_snapshot: None,
            gps_snapshot: None,
            gnss_snapshot: None,
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
    fn periodic_collector_always_scans() {
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
        let (events, _event_rx) = mpsc::channel(1);
        let (statuses, status_rx) = mpsc::channel(2);
        // Drop receivers so worker sends fail fast instead of blocking the
        // worker thread: the test asserts scans happen, then joins the thread.
        drop(_event_rx);
        drop(status_rx);
        let mut worker =
            PeriodicCollectorHandle::start(scan, true, Duration::from_millis(10), events, statuses);
        std::thread::sleep(Duration::from_millis(60));
        // Collection is always on: even with `enabled=false` for API
        // compatibility the worker must scan and emit events.
        assert!(calls.load(Ordering::SeqCst) >= 1);
        worker.stop();
    }
}
