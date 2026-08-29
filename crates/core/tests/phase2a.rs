use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, DeviceIdentity, ServerProfile};
use remote_env_core::runtime::{CollectorScan, RuntimeSupervisor};
use remote_env_core::state::StateStore;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_tungstenite::{accept_async, tungstenite::Message};

struct Fixture {
    url: String,
    received: Arc<Mutex<Vec<Value>>>,
    ack: Arc<AtomicBool>,
    disconnect: Arc<Notify>,
}

impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let ack = Arc::new(AtomicBool::new(true));
        let disconnect = Arc::new(Notify::new());
        let state = (received.clone(), ack.clone(), disconnect.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = state.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = accept_async(stream).await else {
                        return;
                    };
                    let Some(Ok(Message::Text(auth))) = socket.next().await else {
                        return;
                    };
                    let auth: Value = serde_json::from_str(&auth).unwrap();
                    assert_eq!(auth["type"], "auth");
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
                    loop {
                        let message = tokio::select! {
                            _ = state.2.notified() => { let _ = socket.close(None).await; return; }
                            message = socket.next() => message,
                        };
                        let Some(Ok(message)) = message else {
                            break;
                        };
                        let Ok(raw) = message.to_text() else {
                            break;
                        };
                        let value: Value = serde_json::from_str(raw).unwrap();
                        if value["type"] == "environment_data" {
                            state.0.lock().unwrap().push(value.clone());
                            if !state.1.load(Ordering::SeqCst) {
                                let _ = socket.close(None).await;
                                return;
                            }
                            let ack = serde_json::json!({"type":"data_result","success":true,"device_id":value["device_id"],"data_type":value["data_type"],"sequence":value["sequence"]});
                            socket
                                .send(Message::Text(ack.to_string().into()))
                                .await
                                .unwrap();
                        }
                    }
                });
            }
        });
        Self {
            url,
            received,
            ack,
            disconnect,
        }
    }

    async fn wait_for(&self, predicate: impl Fn(&Self) -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !predicate(self) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}

fn snapshot_event() -> CollectorEvent {
    CollectorEvent {
        data_type: "wifi".into(),
        timestamp_ms: 1,
        data: serde_json::json!({
            "interfaces": 1,
            "scan_duration_ms": 12,
            "networks": [{
                "ssid": "fixture-wifi", "ssid_bytes_hex": null, "hidden": false,
                "bssid": "AA:BB:CC:DD:EE:FF", "signal_strength_dbm": -42,
                "signal_percent": 80, "channel": 36, "frequency_mhz": 5180,
                "band": "band5_ghz", "phy_type": "802.11ac",
                "network_type": "infrastructure",
                "security": {"authentication": null, "encryption": null, "privacy": true},
                "interface_id": "fixture-interface"
            }]
        }),
    }
}

async fn wait_collection_running(runtime: &RuntimeSupervisor) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !runtime.status().collection_running {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn config(identity: DeviceIdentity, fixture: &Fixture) -> ClientConfig {
    let mut config = ClientConfig::default();
    config.identity = identity;
    config.wifi_enabled = true;
    config.scan_interval_seconds = 1;
    config.upload_interval_seconds = 1;
    config.heartbeat_interval_seconds = 1;
    let profile_device_id = config.identity.device_id.clone();
    config.server_profiles = vec![ServerProfile {
        id: "wifi-server".into(),
        name: "Wi-Fi fixture".into(),
        url: fixture.url.clone(),
        device_id: profile_device_id,
        token: "fixture-token".into(),
        enabled: true,
    }];
    config.active_server_id = Some("wifi-server".into());
    config
}

async fn wait_completed(store: &StateStore, device_id: &str, sequence: u64) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while store
            .delivery_status("wifi-server", device_id, "wifi", sequence)
            .unwrap()
            .as_deref()
            != Some("completed")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn wifi_event_reaches_upload_delivery_and_completes() {
    let fixture = Fixture::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let identity = DeviceIdentity {
        device_id: "wifi-runtime-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let scan: CollectorScan = Arc::new(|| Ok(snapshot_event()));
    let run_config = config(identity, &fixture);
    let mut runtime = RuntimeSupervisor::start_with_collector(
        run_config.clone(),
        store.clone(),
        Some(scan),
    )
    .unwrap();
    runtime.set_collection_running(run_config, true).unwrap();
    wait_collection_running(&runtime).await;
    fixture
        .wait_for(|fixture| !fixture.received.lock().unwrap().is_empty())
        .await;
    let payload = fixture.received.lock().unwrap()[0].clone();
    assert_eq!(payload["data_type"], "wifi");
    assert_eq!(payload["data"]["networks"][0]["bssid"], "AA:BB:CC:DD:EE:FF");
    let sequence = payload["sequence"].as_u64().expect("sequence must be numeric");
    assert!(sequence > 1_000_000_000_000);
    wait_completed(&store, "wifi-runtime-device", sequence).await;
    assert!(
        store
            .event_complete("wifi-runtime-device", "wifi", sequence)
            .unwrap()
    );
    runtime.stop();
}

#[tokio::test]
async fn wifi_delivery_reconnects_with_same_sequence_and_envelope() {
    let fixture = Fixture::start().await;
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let identity = DeviceIdentity {
        device_id: "wifi-recovery-device".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let scan: CollectorScan = Arc::new(|| Ok(snapshot_event()));
    let mut recovery_config = config(identity, &fixture);
    recovery_config.wifi_enabled = false;
    let mut runtime = RuntimeSupervisor::start_with_collector(
        recovery_config.clone(),
        store.clone(),
        Some(scan),
    )
    .unwrap();
    let running = recovery_config;
    runtime.set_collection_running(running, true).unwrap();
    fixture.ack.store(false, Ordering::SeqCst);
    runtime.submit(snapshot_event()).unwrap();
    fixture
        .wait_for(|fixture| fixture.received.lock().unwrap().len() >= 1)
        .await;
    let first = fixture.received.lock().unwrap()[0].clone();
    assert!(matches!(
        store
            .delivery_status("wifi-server", "wifi-recovery-device", "wifi", first["sequence"].as_u64().unwrap())
            .unwrap()
            .as_deref(),
        Some("pending") | Some("in_flight")
    ));
    fixture.disconnect.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;
    fixture.ack.store(true, Ordering::SeqCst);
    fixture
        .wait_for(|fixture| fixture.received.lock().unwrap().len() >= 2)
        .await;
    let resent = fixture.received.lock().unwrap()[1].clone();
    assert_eq!(first["device_id"], resent["device_id"]);
    assert_eq!(first["data_type"], resent["data_type"]);
    assert_eq!(first["sequence"], resent["sequence"]);
    assert_eq!(first["data"], resent["data"]);
    wait_completed(&store, "wifi-recovery-device", first["sequence"].as_u64().unwrap()).await;
    assert_eq!(
        store.get_sequence("wifi-recovery-device", "wifi").unwrap(),
        Some(first["sequence"].as_u64().unwrap())
    );
    runtime.stop();
}
