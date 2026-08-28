use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, DeviceIdentity, ServerProfile};
use remote_env_core::runtime::RuntimeSupervisor;
use remote_env_core::state::StateStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
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
