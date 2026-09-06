use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, DeviceIdentity, ServerMode, ServerProfile};
use remote_env_core::runtime::RuntimeSupervisor;
use remote_env_core::state::StateStore;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_tungstenite::{accept_async, tungstenite::Message};

struct TestServer {
    url: String,
    received: Arc<Mutex<Vec<Value>>>,
    ready: Arc<AtomicUsize>,
    auth_attempts: Arc<AtomicUsize>,
    auth_frames: Arc<Mutex<Vec<Value>>>,
    ack: Arc<AtomicBool>,
    auth_failure: Arc<AtomicBool>,
    heartbeat_count: Arc<AtomicUsize>,
    pong_enabled: Arc<AtomicBool>,
    rate_limited: Arc<AtomicBool>,
    reject_next_upload: Arc<AtomicBool>,
    disconnect: Arc<Notify>,
    changed: Arc<Notify>,
}

#[test]
fn reconnect_backoff_is_exponential_and_capped() {
    let mut backoff = remote_env_core::transport::Backoff::new(1, 60);
    assert_eq!(backoff.next_delay_seconds(), 1);
    assert_eq!(backoff.next_delay_seconds(), 2);
    assert_eq!(backoff.next_delay_seconds(), 4);
    assert_eq!(backoff.next_delay_seconds(), 8);
    assert_eq!(backoff.next_delay_seconds(), 16);
    assert_eq!(backoff.next_delay_seconds(), 32);
    assert_eq!(backoff.next_delay_seconds(), 60);
    assert_eq!(backoff.next_delay_seconds(), 60);
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let ready = Arc::new(AtomicUsize::new(0));
        let auth_attempts = Arc::new(AtomicUsize::new(0));
        let auth_frames = Arc::new(Mutex::new(Vec::new()));
        let ack = Arc::new(AtomicBool::new(true));
        let disconnect = Arc::new(Notify::new());
        let auth_failure = Arc::new(AtomicBool::new(false));
        let heartbeat_count = Arc::new(AtomicUsize::new(0));
        let pong_enabled = Arc::new(AtomicBool::new(true));
        let rate_limited = Arc::new(AtomicBool::new(false));
        let reject_next_upload = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(Notify::new());
        let task_state = (
            received.clone(),
            ready.clone(),
            auth_attempts.clone(),
            auth_frames.clone(),
            ack.clone(),
            disconnect.clone(),
            auth_failure.clone(),
            changed.clone(),
            heartbeat_count.clone(),
            pong_enabled.clone(),
            rate_limited.clone(),
            reject_next_upload.clone(),
        );
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = task_state.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = accept_async(stream).await else {
                        return;
                    };
                    let Some(Ok(Message::Text(raw))) = socket.next().await else {
                        return;
                    };
                    state.2.fetch_add(1, Ordering::SeqCst);
                    let auth: Value = serde_json::from_str(&raw).unwrap();
                    state.3.lock().unwrap().push(auth.clone());
                    if auth["type"] != "auth" {
                        return;
                    }
                    if state.6.load(Ordering::SeqCst) {
                        let _ = socket.send(Message::Text(r#"{"type":"auth_result","success":false,"message":"invalid token"}"#.into())).await;
                        return;
                    }
                    let _ = socket
                        .send(Message::Text(
                            r#"{"type":"auth_result","success":true}"#.into(),
                        ))
                        .await;
                    let _ = socket
                        .send(Message::Text(
                            r#"{"type":"device_list","devices":[]}"#.into(),
                        ))
                        .await;
                    state.1.fetch_add(1, Ordering::SeqCst);
                    state.7.notify_waiters();
                    loop {
                        let message = tokio::select! {
                            _ = state.5.notified() => {
                                let _ = socket.close(None).await;
                                break;
                            }
                            message = socket.next() => message,
                        };
                        let Some(Ok(message)) = message else {
                            break;
                        };
                        let Ok(raw) = message.to_text() else {
                            break;
                        };
                        let value: Value = serde_json::from_str(raw).unwrap();
                        match value["type"].as_str() {
                            Some("environment_data") => {
                                state.0.lock().unwrap().push(value.clone());
                                state.7.notify_waiters();
                                if state.10.load(Ordering::SeqCst) {
                                    let _ = socket
                                        .send(Message::Text(
                                            r#"{"type":"error","code":"rate_limited","message":"slow down","retryable":true}"#.into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                if state.11.swap(false, Ordering::SeqCst) {
                                    let _ = socket
                                        .send(Message::Text(
                                            r#"{"type":"error","code":"validation_error","message":"invalid payload","retryable":false}"#.into(),
                                        ))
                                        .await;
                                    continue;
                                }
                                if state.4.load(Ordering::SeqCst) {
                                    let ack = serde_json::json!({"type":"data_result","success":true,"device_id":value["device_id"],"data_type":value["data_type"],"sequence":value["sequence"]});
                                    let _ =
                                        socket.send(Message::Text(ack.to_string().into())).await;
                                }
                            }
                            Some("ping") => {
                                state.8.fetch_add(1, Ordering::SeqCst);
                                if state.9.load(Ordering::SeqCst) {
                                    let _ = socket
                                        .send(Message::Text(r#"{"type":"pong"}"#.into()))
                                        .await;
                                }
                                state.7.notify_waiters();
                            }
                            _ => {}
                        }
                    }
                });
            }
        });
        Self {
            url,
            received,
            ready,
            auth_attempts,
            auth_frames,
            ack,
            disconnect,
            auth_failure,
            heartbeat_count,
            pong_enabled,
            rate_limited,
            reject_next_upload,
            changed,
        }
    }

    async fn wait_for(&self, predicate: impl Fn(&Self) -> bool) {
        tokio::time::timeout(Duration::from_secs(12), async {
            while !predicate(self) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    fn sequences(&self) -> Vec<u64> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|v| v["sequence"].as_u64())
            .collect()
    }

    fn auth_attempt_count(&self) -> usize {
        self.auth_attempts.load(Ordering::SeqCst)
    }

    fn heartbeat_count(&self) -> usize {
        self.heartbeat_count.load(Ordering::SeqCst)
    }

    fn auth_frames(&self) -> Vec<Value> {
        self.auth_frames.lock().unwrap().clone()
    }

    fn rate_limit(&self, enabled: bool) {
        self.rate_limited.store(enabled, Ordering::SeqCst);
    }

    fn pong(&self, enabled: bool) {
        self.pong_enabled.store(enabled, Ordering::SeqCst);
    }

    fn reject_next_upload(&self) {
        self.reject_next_upload.store(true, Ordering::SeqCst);
    }
}

fn config(identity: DeviceIdentity, a: &TestServer, b: &TestServer) -> ClientConfig {
    let mut config = ClientConfig::default();
    let profile_device_id = identity.device_id.clone();
    config.identity = identity;
    config.scan_interval_seconds = 1;
    config.upload_interval_seconds = 1;
    config.heartbeat_interval_seconds = 1;
    config.server_profiles = vec![
        ServerProfile {
            id: "a".into(),
            name: "A".into(),
            url: a.url.clone(),
            device_id: profile_device_id.clone(),
            token: "a-token".into(),
            enabled: true,
        },
        ServerProfile {
            id: "b".into(),
            name: "B".into(),
            url: b.url.clone(),
            device_id: profile_device_id,
            token: "b-token".into(),
            enabled: true,
        },
    ];
    config.active_server_id = Some("a".into());
    config
}

fn event(label: &str) -> CollectorEvent {
    CollectorEvent {
        data_type: "wifi".into(),
        timestamp_ms: 1,
        data: serde_json::json!({"label": label}),
    }
}

async fn wait_ready(runtime: &RuntimeSupervisor, count: usize) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while runtime
            .status()
            .servers
            .iter()
            .filter(|s| s.connection.to_string() == "Ready")
            .count()
            != count
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_complete(store: &StateStore, device_id: &str, data_type: &str, sequence: u64) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while !store
            .event_complete(device_id, data_type, sequence)
            .unwrap()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_profiles(runtime: &RuntimeSupervisor, expected: &[&str]) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while runtime
            .status()
            .servers
            .iter()
            .map(|server| server.profile_id.as_str())
            .collect::<Vec<_>>()
            != expected
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_delivery_status(
    store: &StateStore,
    target_id: &str,
    device_id: &str,
    data_type: &str,
    sequence: u64,
    expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while store
            .delivery_status(target_id, device_id, data_type, sequence)
            .unwrap()
            .as_deref()
            != Some(expected)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_connection(runtime: &RuntimeSupervisor, profile_id: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(65), async {
        loop {
            let current = runtime
                .status()
                .servers
                .iter()
                .find(|server| server.profile_id == profile_id)
                .map(|server| server.connection.to_string());
            if current.as_deref() == Some(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn phase_175c_runtime_uses_independent_dual_servers_and_recovery() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "test-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut multi = config(identity, &a, &b);
    multi.server_mode = ServerMode::Multi;
    let mut runtime = RuntimeSupervisor::start(multi.clone(), store.clone()).unwrap();
    runtime.set_collection_running(multi.clone(), true).unwrap();
    wait_ready(&runtime, 2).await;
    assert_eq!(a.ready.load(Ordering::SeqCst), 1);
    assert_eq!(b.ready.load(Ordering::SeqCst), 1);
    a.wait_for(|server| server.heartbeat_count() > 0).await;
    assert_eq!(a.auth_frames()[0]["type"], "auth");
    assert_eq!(b.auth_frames()[0]["type"], "auth");

    runtime.submit(event("one")).unwrap();
    a.wait_for(|s| !s.sequences().is_empty()).await;
    b.wait_for(|s| !s.sequences().is_empty()).await;
    let first_sequence = a.sequences()[0];
    assert_eq!(a.received.lock().unwrap()[0], b.received.lock().unwrap()[0]);
    wait_complete(&store, "test-device", "wifi", first_sequence).await;

    a.ack.store(false, Ordering::SeqCst);
    runtime.submit(event("recovery")).unwrap();
    a.wait_for(|s| s.sequences().len() >= 2).await;
    b.wait_for(|s| s.sequences().len() >= 2).await;
    let second_sequence = a.sequences()[1];
    wait_delivery_status(
        &store,
        "b",
        "test-device",
        "wifi",
        second_sequence,
        "completed",
    )
    .await;
    assert_eq!(
        store
            .delivery_status("a", "test-device", "wifi", second_sequence)
            .unwrap()
            .as_deref(),
        Some("in_flight")
    );
    a.disconnect.notify_one();
    a.ack.store(true, Ordering::SeqCst);
    wait_connection(&runtime, "a", "Reconnecting").await;
    wait_connection(&runtime, "b", "Ready").await;
    a.wait_for(|s| s.ready.load(Ordering::SeqCst) >= 2).await;
    a.wait_for(|s| s.sequences().len() >= 3).await;
    assert!(a.sequences().len() >= 3);
    wait_complete(&store, "test-device", "wifi", second_sequence).await;

    runtime
        .update_config({
            let mut c = multi.clone();
            c.server_mode = ServerMode::Single;
            c.active_server_id = Some("b".into());
            c
        })
        .unwrap();
    wait_for_profiles(&runtime, &["b"]).await;
    runtime.submit(event("single-b")).unwrap();
    let a_before = a.sequences().len();
    b.wait_for(|s| s.sequences().len() >= 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(a.sequences().len(), a_before);
    runtime.stop();
    assert_eq!(runtime.status().connection.to_string(), "Stopped");
}

#[tokio::test]
async fn phase_175c_auth_failure_retries_until_server_recovers() {
    let a = TestServer::start().await;
    a.auth_failure.store(true, Ordering::SeqCst);
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "blocked-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut config = ClientConfig::default();
    config.identity = identity;
    config.server_profiles = vec![ServerProfile {
        id: "a".into(),
        name: "A".into(),
        url: a.url.clone(),
        device_id: "blocked-device".into(),
        token: "bad".into(),
        enabled: true,
    }];
    config.active_server_id = Some("a".into());
    let mut runtime = RuntimeSupervisor::start(config, store).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime
            .status()
            .servers
            .first()
            .map(|s| s.connection.to_string())
            != Some("Reconnecting".into())
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(a.ready.load(Ordering::SeqCst), 0);
    assert!(runtime.status().servers[0].next_retry_at_ms.is_some());
    a.wait_for(|server| server.auth_attempt_count() >= 3).await;
    a.auth_failure.store(false, Ordering::SeqCst);
    wait_ready(&runtime, 1).await;
    assert_eq!(runtime.status().servers[0].next_retry_at_ms, None);
    runtime.stop();
}

#[tokio::test]
async fn silent_auth_times_out_and_rapid_mode_changes_remain_responsive() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent_url = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                if let Ok(mut socket) = accept_async(stream).await {
                    while socket.next().await.is_some() {}
                }
            });
        }
    });
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("cache.sqlite3"),
    )
    .unwrap();
    let mut identity = ClientConfig::default().identity;
    identity.device_id = "silent-auth-device".into();
    let mut config = config(identity, &a, &b);
    config.server_profiles[0].url = silent_url;
    config.server_mode = ServerMode::Multi;
    let mut runtime = RuntimeSupervisor::start(config.clone(), store).unwrap();
    wait_connection(&runtime, "a", "Authenticating").await;
    tokio::time::timeout(
        Duration::from_secs(18),
        wait_connection(&runtime, "a", "Reconnecting"),
    )
    .await
    .unwrap();
    assert!(
        runtime
            .status()
            .servers
            .iter()
            .find(|server| server.profile_id == "a")
            .unwrap()
            .last_error
            .as_deref()
            .unwrap()
            .contains("认证响应超时")
    );
    wait_connection(&runtime, "a", "Authenticating").await;
    for _ in 0..5 {
        config.server_mode = ServerMode::Single;
        config.active_server_id = Some("b".into());
        runtime.update_config(config.clone()).unwrap();
        config.server_mode = ServerMode::Multi;
        runtime.update_config(config.clone()).unwrap();
    }
    config.server_mode = ServerMode::Single;
    runtime.update_config(config).unwrap();
    tokio::time::timeout(Duration::from_secs(2), wait_for_profiles(&runtime, &["b"]))
        .await
        .unwrap();
    wait_ready(&runtime, 1).await;
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_profile_removal_cancels_target_delivery() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "delete-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut multi = config(identity, &a, &b);
    multi.server_mode = ServerMode::Multi;
    let mut runtime = RuntimeSupervisor::start(multi.clone(), store.clone()).unwrap();
    runtime.set_collection_running(multi.clone(), true).unwrap();
    wait_ready(&runtime, 2).await;
    a.ack.store(false, Ordering::SeqCst);
    runtime.submit(event("delete")).unwrap();
    a.wait_for(|server| !server.sequences().is_empty()).await;
    let sequence = a.sequences()[0];
    let mut single = multi;
    single.server_mode = ServerMode::Single;
    single.active_server_id = Some("b".into());
    runtime.update_config(single).unwrap();
    wait_for_profiles(&runtime, &["b"]).await;
    wait_delivery_status(&store, "a", "delete-device", "wifi", sequence, "cancelled").await;
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_single_to_multi_and_rate_limit_keep_other_target_independent() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "transition-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut single = config(identity, &a, &b);
    single.server_mode = ServerMode::Single;
    let mut runtime = RuntimeSupervisor::start(single.clone(), store.clone()).unwrap();
    runtime
        .set_collection_running(single.clone(), true)
        .unwrap();
    wait_ready(&runtime, 1).await;
    runtime.submit(event("single")).unwrap();
    a.wait_for(|server| !server.sequences().is_empty()).await;
    assert!(b.sequences().is_empty());
    let mut multi = single;
    multi.server_mode = ServerMode::Multi;
    runtime.update_config(multi.clone()).unwrap();
    wait_ready(&runtime, 2).await;
    b.rate_limit(true);
    runtime.submit(event("limited")).unwrap();
    a.wait_for(|server| server.sequences().len() >= 2).await;
    b.wait_for(|server| !server.sequences().is_empty()).await;
    wait_connection(&runtime, "b", "Reconnecting").await;
    let second_sequence = a.sequences()[1];
    wait_delivery_status(
        &store,
        "a",
        "transition-device",
        "wifi",
        second_sequence,
        "completed",
    )
    .await;
    assert_eq!(
        store
            .delivery_status("b", "transition-device", "wifi", second_sequence)
            .unwrap()
            .as_deref(),
        Some("pending")
    );
    let attempts = b.auth_attempt_count();
    let uploads = b.sequences().len();
    // Updating unrelated settings must not bypass the rate-limit cooldown.
    runtime.update_config(multi.clone()).unwrap();
    runtime.submit(event("after-limit")).unwrap();
    a.wait_for(|server| server.sequences().len() >= 3).await;
    let third_sequence = a.sequences()[2];
    wait_delivery_status(
        &store,
        "a",
        "transition-device",
        "wifi",
        third_sequence,
        "completed",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(b.auth_attempt_count(), attempts);
    assert_eq!(b.sequences().len(), uploads);
    let status = runtime.status();
    let paused = status
        .servers
        .iter()
        .find(|server| server.profile_id == "b")
        .unwrap();
    assert_eq!(paused.connection.to_string(), "Reconnecting");
    assert_eq!(paused.in_flight, 0);
    assert!(paused.last_error.as_deref().unwrap().contains("限速"));

    let retry_at = paused.next_retry_at_ms.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!((20_000..=30_000).contains(&(retry_at - now)));
    b.rate_limit(false);
    tokio::time::timeout(Duration::from_secs(35), async {
        while b.auth_attempt_count() == attempts {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    wait_ready(&runtime, 2).await;
    wait_delivery_status(
        &store,
        "b",
        "transition-device",
        "wifi",
        second_sequence,
        "completed",
    )
    .await;
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_missing_pong_enters_reconnecting() {
    verify_heartbeat_timeout_recovers(true).await;
}

#[tokio::test]
async fn phase_175c_no_initial_pong_times_out_and_recovers() {
    verify_heartbeat_timeout_recovers(false).await;
}

async fn verify_heartbeat_timeout_recovers(initial_pong: bool) {
    let a = TestServer::start().await;
    a.pong(initial_pong);
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "heartbeat-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut config = config(identity, &a, &b);
    config.server_profiles.truncate(1);
    config.heartbeat_interval_seconds = 1;
    let mut runtime = RuntimeSupervisor::start(config, store).unwrap();
    wait_ready(&runtime, 1).await;
    a.wait_for(|server| server.heartbeat_count() > 0).await;
    if initial_pong {
        wait_heartbeat_alive(&runtime).await;
    }
    a.pong(false);
    wait_connection(&runtime, "a", "Reconnecting").await;
    let previous_heartbeats = a.heartbeat_count();
    a.pong(true);
    a.wait_for(|server| server.heartbeat_count() > previous_heartbeats)
        .await;
    wait_ready(&runtime, 1).await;
    wait_heartbeat_alive(&runtime).await;
    runtime.stop();
}

async fn wait_heartbeat_alive(runtime: &RuntimeSupervisor) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !runtime
            .status()
            .servers
            .iter()
            .any(|server| server.heartbeat_alive)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn phase_175c_multi_to_single_b_stops_a_and_keeps_b() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "mode-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut multi = config(identity, &a, &b);
    multi.server_mode = ServerMode::Multi;
    let mut runtime = RuntimeSupervisor::start(multi.clone(), store).unwrap();
    runtime.set_collection_running(multi.clone(), true).unwrap();
    wait_ready(&runtime, 2).await;
    let mut single = multi;
    single.server_mode = ServerMode::Single;
    single.active_server_id = Some("b".into());
    runtime.update_config(single).unwrap();
    wait_for_profiles(&runtime, &["b"]).await;
    runtime.submit(event("single-b")).unwrap();
    b.wait_for(|server| !server.sequences().is_empty()).await;
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_ack_isolation_leaves_b_pending_until_b_ack() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    a.ack.store(false, Ordering::SeqCst);
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "ack-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut config = config(identity, &a, &b);
    config.server_mode = ServerMode::Multi;
    let mut runtime = RuntimeSupervisor::start(config.clone(), store.clone()).unwrap();
    runtime
        .set_collection_running(config.clone(), true)
        .unwrap();
    wait_ready(&runtime, 2).await;
    runtime.submit(event("ack-isolation")).unwrap();
    a.wait_for(|server| !server.sequences().is_empty()).await;
    b.wait_for(|server| !server.sequences().is_empty()).await;
    let sequence = a.sequences()[0];
    wait_delivery_status(&store, "a", "ack-device", "wifi", sequence, "in_flight").await;
    wait_delivery_status(&store, "b", "ack-device", "wifi", sequence, "completed").await;
    assert!(
        !store
            .event_complete("ack-device", "wifi", sequence)
            .unwrap()
    );
    a.ack.store(true, Ordering::SeqCst);
    a.disconnect.notify_one();
    wait_complete(&store, "ack-device", "wifi", sequence).await;
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_non_retryable_upload_error_blocks_only_rejected_delivery() {
    let server = TestServer::start().await;
    let other = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "reject-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut config = config(identity, &server, &other);
    config.server_profiles.truncate(1);
    let mut runtime = RuntimeSupervisor::start(config.clone(), store.clone()).unwrap();
    runtime
        .set_collection_running(config.clone(), true)
        .unwrap();
    wait_ready(&runtime, 1).await;

    server.reject_next_upload();
    runtime.submit(event("rejected")).unwrap();
    server
        .wait_for(|server| server.sequences().len() == 1)
        .await;
    let rejected_sequence = server.sequences()[0];
    wait_delivery_status(
        &store,
        "a",
        "reject-device",
        "wifi",
        rejected_sequence,
        "blocked",
    )
    .await;
    runtime.update_config(config.clone()).unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(server.sequences().len(), 1);

    runtime.submit(event("accepted")).unwrap();
    server
        .wait_for(|server| server.sequences().len() == 2)
        .await;
    let accepted_sequence = server.sequences()[1];
    wait_delivery_status(
        &store,
        "a",
        "reject-device",
        "wifi",
        accepted_sequence,
        "completed",
    )
    .await;
    assert_eq!(server.auth_attempt_count(), 1);
    assert_eq!(runtime.status().servers[0].blocked, 1);
    assert!(
        runtime.status().servers[0]
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("validation_error"))
    );
    runtime.stop();
}

#[tokio::test]
async fn phase_175c_profile_url_and_token_changes_replace_worker() {
    let old_server = TestServer::start().await;
    let new_server = TestServer::start().await;
    let other = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(
        dir.path().join("state.sqlite3"),
        dir.path().join("state_cache.sqlite3"),
    )
    .unwrap();
    let identity = DeviceIdentity {
        device_id: "profile-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut config = config(identity, &old_server, &other);
    config.server_profiles.truncate(1);
    config.server_profiles[0].token = "old-token".into();
    let mut runtime = RuntimeSupervisor::start(config.clone(), store).unwrap();
    wait_ready(&runtime, 1).await;
    old_server
        .wait_for(|server| !server.auth_frames().is_empty())
        .await;
    let mut updated = config;
    updated.server_profiles[0].url = new_server.url.clone();
    updated.server_profiles[0].token = "new-token".into();
    runtime.update_config(updated).unwrap();
    wait_for_profiles(&runtime, &["a"]).await;
    new_server
        .wait_for(|server| !server.auth_frames().is_empty())
        .await;
    assert_eq!(new_server.auth_frames()[0]["token"], "new-token");
    runtime.stop();
}
