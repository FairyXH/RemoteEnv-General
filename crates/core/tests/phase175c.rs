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
    ack: Arc<AtomicBool>,
    auth_failure: Arc<AtomicBool>,
    disconnect: Arc<Notify>,
    changed: Arc<Notify>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let ready = Arc::new(AtomicUsize::new(0));
        let auth_attempts = Arc::new(AtomicUsize::new(0));
        let ack = Arc::new(AtomicBool::new(true));
        let disconnect = Arc::new(Notify::new());
        let auth_failure = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(Notify::new());
        let task_state = (
            received.clone(),
            ready.clone(),
            auth_attempts.clone(),
            ack.clone(),
            disconnect.clone(),
            auth_failure.clone(),
            changed.clone(),
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
                    assert_eq!(auth["type"], "auth");
                    if state.5.load(Ordering::SeqCst) {
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
                    state.6.notify_waiters();
                    loop {
                        let message = tokio::select! {
                            _ = state.4.notified() => {
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
                                state.6.notify_waiters();
                                if state.3.load(Ordering::SeqCst) {
                                    let ack = serde_json::json!({"type":"data_result","success":true,"device_id":value["device_id"],"data_type":value["data_type"],"sequence":value["sequence"]});
                                    let _ =
                                        socket.send(Message::Text(ack.to_string().into())).await;
                                }
                            }
                            Some("heartbeat") => {
                                let _ = socket
                                    .send(Message::Text(r#"{"type":"pong"}"#.into()))
                                    .await;
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
            ack,
            disconnect,
            auth_failure,
            changed,
        }
    }

    async fn wait_for(&self, predicate: impl Fn(&Self) -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !predicate(self) {
                self.changed.notified().await;
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
}

fn config(identity: DeviceIdentity, a: &TestServer, b: &TestServer) -> ClientConfig {
    let mut config = ClientConfig::default();
    config.identity = identity;
    config.heartbeat_interval_seconds = 1;
    config.server_profiles = vec![
        ServerProfile {
            id: "a".into(),
            name: "A".into(),
            url: a.url.clone(),
            token: "a-token".into(),
            enabled: true,
        },
        ServerProfile {
            id: "b".into(),
            name: "B".into(),
            url: b.url.clone(),
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
    tokio::time::timeout(Duration::from_secs(5), async {
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
    tokio::time::timeout(Duration::from_secs(5), async {
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
    tokio::time::timeout(Duration::from_secs(5), async {
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

#[tokio::test]
async fn phase_175c_runtime_uses_independent_dual_servers_and_recovery() {
    let a = TestServer::start().await;
    let b = TestServer::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
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
    wait_ready(&runtime, 2).await;
    assert_eq!(a.ready.load(Ordering::SeqCst), 1);
    assert_eq!(b.ready.load(Ordering::SeqCst), 1);

    runtime.submit(event("one")).unwrap();
    a.wait_for(|s| s.sequences() == vec![1]).await;
    b.wait_for(|s| s.sequences() == vec![1]).await;
    assert_eq!(a.received.lock().unwrap()[0], b.received.lock().unwrap()[0]);
    wait_complete(&store, "test-device", "wifi", 1).await;

    a.ack.store(false, Ordering::SeqCst);
    runtime.submit(event("recovery")).unwrap();
    a.wait_for(|s| s.sequences().contains(&2)).await;
    b.wait_for(|s| s.sequences().contains(&2)).await;
    a.disconnect.notify_one();
    a.ack.store(true, Ordering::SeqCst);
    a.wait_for(|s| s.ready.load(Ordering::SeqCst) >= 2).await;
    a.wait_for(|s| s.sequences().iter().filter(|&&n| n == 2).count() >= 2)
        .await;
    assert_eq!(a.sequences().iter().filter(|&&n| n == 2).count(), 2);
    wait_complete(&store, "test-device", "wifi", 2).await;

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
    b.wait_for(|s| s.sequences().contains(&3)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(a.sequences().len(), a_before);
    runtime.stop();
    assert_eq!(runtime.status().connection.to_string(), "Stopped");
}

#[tokio::test]
async fn phase_175c_auth_failure_becomes_blocked_without_retry() {
    let a = TestServer::start().await;
    a.auth_failure.store(true, Ordering::SeqCst);
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
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
            != Some("Blocked".into())
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(a.ready.load(Ordering::SeqCst), 0);
    let attempts = a.auth_attempt_count();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(a.auth_attempt_count(), attempts);
    runtime.stop();
}
