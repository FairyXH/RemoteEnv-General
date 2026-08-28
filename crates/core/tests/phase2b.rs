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

async fn fixture() -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let received = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::clone(&received);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                let Some(Ok(Message::Text(_))) = socket.next().await else {
                    return;
                };
                socket
                    .send(Message::Text(
                        r#"{"type":"auth_result","success":true}"#.into(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Text(
                        r#"{"type":"device_list","devices":[]}"#.into(),
                    ))
                    .await
                    .unwrap();
                while let Some(Ok(message)) = socket.next().await {
                    let Ok(raw) = message.to_text() else {
                        continue;
                    };
                    let value: Value = serde_json::from_str(raw).unwrap();
                    if value["type"] == "environment_data" {
                        state.lock().unwrap().push(value.clone());
                        socket.send(Message::Text(serde_json::json!({"type":"data_result","success":true,"device_id":value["device_id"],"data_type":value["data_type"],"sequence":value["sequence"]}).to_string().into())).await.unwrap();
                    }
                }
            });
        }
    });
    (url, received)
}

struct ControlledFixture {
    url: String,
    received: Arc<Mutex<Vec<Value>>>,
    ack: Arc<AtomicBool>,
    disconnect: Arc<Notify>,
}

async fn controlled_fixture() -> ControlledFixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture = ControlledFixture {
        url: format!("ws://{}", listener.local_addr().unwrap()),
        received: Arc::new(Mutex::new(Vec::new())),
        ack: Arc::new(AtomicBool::new(true)),
        disconnect: Arc::new(Notify::new()),
    };
    let received = Arc::clone(&fixture.received);
    let ack = Arc::clone(&fixture.ack);
    let disconnect = Arc::clone(&fixture.disconnect);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let received = Arc::clone(&received);
            let ack = Arc::clone(&ack);
            let disconnect = Arc::clone(&disconnect);
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                let Some(Ok(Message::Text(_))) = socket.next().await else {
                    return;
                };
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
                loop {
                    let message = tokio::select! {
                        _ = disconnect.notified() => { let _ = socket.close(None).await; return; }
                        message = socket.next() => message,
                    };
                    let Some(Ok(message)) = message else {
                        return;
                    };
                    let Ok(raw) = message.to_text() else {
                        continue;
                    };
                    let value: Value = serde_json::from_str(raw).unwrap();
                    if value["type"] == "environment_data" {
                        received.lock().unwrap().push(value.clone());
                        if ack.load(Ordering::SeqCst) {
                            let result = serde_json::json!({"type":"data_result","success":true,"device_id":value["device_id"],"data_type":value["data_type"],"sequence":value["sequence"]});
                            let _ = socket.send(Message::Text(result.to_string().into())).await;
                        }
                    }
                }
            });
        }
    });
    fixture
}

async fn wait_received(received: &Arc<Mutex<Vec<Value>>>, count: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while received.lock().unwrap().len() < count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn config(url: String) -> ClientConfig {
    let mut config = ClientConfig::default();
    config.identity = DeviceIdentity {
        device_id: "phase2b-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    config.bluetooth_enabled = true;
    config.scan_interval_seconds = 1;
    config.heartbeat_interval_seconds = 1;
    config.server_profiles = vec![ServerProfile {
        id: "bluetooth-server".into(),
        name: "Bluetooth fixture".into(),
        url,
        token: "fixture-token".into(),
        enabled: true,
    }];
    config.active_server_id = Some("bluetooth-server".into());
    config
}

#[tokio::test]
async fn bluetooth_event_uses_shared_sequence_and_completes_delivery() {
    let (url, received) = fixture().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let scan = Arc::new(|| {
        Ok(CollectorEvent {
            data_type: "bluetooth".into(),
            timestamp_ms: 1,
            data: serde_json::json!({"observations":[{"address":"AA:BB:CC:DD:EE:01","transport":"ble","name":"fixture","rssi":-61},{"address":"11:22:33:44:55:66","transport":"classic","name":"Keyboard","class_of_device":123456}],"ble_available":true,"classic_available":true,"scan_duration_ms":4}),
        })
    }) as Arc<dyn Fn() -> Result<CollectorEvent, String> + Send + Sync>;
    let mut runtime =
        RuntimeSupervisor::start_with_collectors(config(url), store.clone(), None, Some(scan))
            .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while received.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let payload = received.lock().unwrap()[0].clone();
    assert_eq!(payload["data_type"], "bluetooth");
    assert_eq!(payload["sequence"], 1);
    assert_eq!(payload["data"]["observations"][0]["transport"], "ble");
    assert_eq!(payload["data"]["observations"][1]["transport"], "classic");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !store
            .event_complete("phase2b-device", "bluetooth", 1)
            .unwrap()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store
            .delivery_status("bluetooth-server", "phase2b-device", "bluetooth", 1)
            .unwrap()
            .as_deref(),
        Some("completed")
    );
    runtime.stop();
}

#[tokio::test]
async fn bluetooth_multi_server_ack_isolation_and_recovery_preserve_envelope() {
    let a = controlled_fixture().await;
    let b = controlled_fixture().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let mut config = ClientConfig::default();
    config.identity = DeviceIdentity {
        device_id: "phase2b-multi-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    config.server_mode = ServerMode::Multi;
    config.heartbeat_interval_seconds = 1;
    config.server_profiles = vec![
        ServerProfile {
            id: "bluetooth-a".into(),
            name: "Bluetooth A".into(),
            url: a.url.clone(),
            token: "a-token".into(),
            enabled: true,
        },
        ServerProfile {
            id: "bluetooth-b".into(),
            name: "Bluetooth B".into(),
            url: b.url.clone(),
            token: "b-token".into(),
            enabled: true,
        },
    ];
    config.active_server_id = Some("bluetooth-a".into());
    let mut runtime = RuntimeSupervisor::start(config, store.clone()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    a.ack.store(false, Ordering::SeqCst);
    b.ack.store(false, Ordering::SeqCst);
    runtime.submit(CollectorEvent { data_type: "bluetooth".into(), timestamp_ms: 1, data: serde_json::json!({"observations":[{"address":"AA:BB:CC:DD:EE:01","transport":"ble","name":"fixture"}]}) }).unwrap();
    wait_received(&a.received, 1).await;
    wait_received(&b.received, 1).await;
    assert_ne!(
        store
            .delivery_status("bluetooth-a", "phase2b-multi-device", "bluetooth", 1)
            .unwrap()
            .as_deref(),
        Some("completed")
    );
    assert_ne!(
        store
            .delivery_status("bluetooth-b", "phase2b-multi-device", "bluetooth", 1)
            .unwrap()
            .as_deref(),
        Some("completed")
    );
    a.ack.store(true, Ordering::SeqCst);
    a.disconnect.notify_waiters();
    tokio::time::sleep(Duration::from_millis(100)).await;
    wait_received(&a.received, 2).await;
    let first = a.received.lock().unwrap()[0].clone();
    let resent = a.received.lock().unwrap()[1].clone();
    assert_eq!(first["device_id"], resent["device_id"]);
    assert_eq!(first["data_type"], resent["data_type"]);
    assert_eq!(first["sequence"], resent["sequence"]);
    assert_eq!(first["data"], resent["data"]);
    tokio::time::timeout(Duration::from_secs(5), async {
        while store
            .delivery_status("bluetooth-a", "phase2b-multi-device", "bluetooth", 1)
            .unwrap()
            .as_deref()
            != Some("completed")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_ne!(
        store
            .delivery_status("bluetooth-b", "phase2b-multi-device", "bluetooth", 1)
            .unwrap()
            .as_deref(),
        Some("completed")
    );
    a.disconnect.notify_waiters();
    tokio::time::sleep(Duration::from_millis(100)).await;
    b.ack.store(true, Ordering::SeqCst);
    wait_received(&b.received, 2).await;
    let first = b.received.lock().unwrap()[0].clone();
    let resent = b.received.lock().unwrap()[1].clone();
    assert_eq!(first["device_id"], resent["device_id"]);
    assert_eq!(first["data_type"], resent["data_type"]);
    assert_eq!(first["sequence"], resent["sequence"]);
    assert_eq!(first["data"], resent["data"]);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !store
            .event_complete("phase2b-multi-device", "bluetooth", 1)
            .unwrap()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    runtime.stop();
}

#[tokio::test]
async fn bluetooth_dynamic_enable_disable_reuses_one_worker() {
    let (url, _received) = fixture().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_scan = Arc::clone(&calls);
    let scan = Arc::new(move || {
        calls_for_scan.fetch_add(1, Ordering::SeqCst);
        Ok(CollectorEvent {
            data_type: "bluetooth".into(),
            timestamp_ms: 1,
            data: serde_json::json!({"observations":[],"ble_available":true,"classic_available":true,"scan_duration_ms":1}),
        })
    }) as Arc<dyn Fn() -> Result<CollectorEvent, String> + Send + Sync>;
    let mut config = config(url);
    config.bluetooth_enabled = false;
    config.scan_interval_seconds = 1;
    let mut runtime =
        RuntimeSupervisor::start_with_collectors(config.clone(), store, None, Some(scan)).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut enabled = config.clone();
    enabled.bluetooth_enabled = true;
    runtime.update_config(enabled.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let before_disable = calls.load(Ordering::SeqCst);
    let mut disabled = enabled;
    disabled.bluetooth_enabled = false;
    runtime.update_config(disabled).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(calls.load(Ordering::SeqCst), before_disable);
    assert_eq!(
        runtime.status().bluetooth_runtime.state,
        remote_env_core::runtime::WiFiRuntimeState::Disabled
    );
    runtime.stop();
}
