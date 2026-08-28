use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, LoggingLevel};
use remote_env_core::protocol::{Ack, EnvironmentEnvelope, ErrorFrame};
use remote_env_core::queue::{QueueError, UploadQueue};
use remote_env_core::runtime::CollectorStatus;
use remote_env_core::state::StateStore;
use remote_env_core::transport::{
    Backoff, ConnectionState, HeartbeatMonitor, ServerEvent, classify_server_message, matches_ack,
};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[test]
fn sequence_is_atomic_and_survives_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.sqlite3");
    let store = StateStore::open(&path).unwrap();
    assert_eq!(store.next_sequence("device-a", "wifi").unwrap(), 1);
    assert_eq!(store.next_sequence("device-a", "wifi").unwrap(), 2);
    drop(store);
    let reopened = StateStore::open(&path).unwrap();
    assert_eq!(reopened.next_sequence("device-a", "wifi").unwrap(), 3);
    reopened.recover_sequence("device-a", "wifi", 10).unwrap();
    assert_eq!(reopened.next_sequence("device-a", "wifi").unwrap(), 11);
}

#[test]
fn identity_is_created_once_and_config_is_round_trippable() {
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let first = store
        .load_or_create_identity("Desktop", "windows", "1.0")
        .unwrap();
    let second = store
        .load_or_create_identity("Changed", "windows", "2.0")
        .unwrap();
    assert_eq!(first.device_id, second.device_id);
    assert_eq!(second.device_name, "Desktop");
    let config = ClientConfig {
        server_url: "ws://example.invalid/ws".into(),
        token: "secret".into(),
        identity: first,
        wifi_enabled: true,
        ble_enabled: false,
        classic_bluetooth_enabled: false,
        scan_interval_seconds: 30,
        heartbeat_interval_seconds: 15,
        max_uploads_per_minute: 60,
        max_queue_size: 100,
        log_level: LoggingLevel::Info,
    };
    store.save_config(&config).unwrap();
    assert_eq!(
        store.load_config().unwrap().unwrap().server_url,
        config.server_url
    );
}

#[test]
fn queue_is_bounded_and_ack_requires_exact_identity() {
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let queue = UploadQueue::new(store, 1);
    let envelope =
        EnvironmentEnvelope::new("device-a", "wifi", 1, serde_json::json!({"networks": []}));
    queue.enqueue(&envelope).unwrap();
    assert!(matches!(
        queue.enqueue(&envelope),
        Err(QueueError::Full { .. })
    ));
    assert!(!matches_ack(
        &Ack {
            device_id: "device-b".into(),
            data_type: "wifi".into(),
            sequence: 1
        },
        &envelope
    ));
    assert!(matches_ack(
        &Ack {
            device_id: "device-a".into(),
            data_type: "wifi".into(),
            sequence: 1
        },
        &envelope
    ));
}

#[test]
fn backoff_is_capped_and_state_is_explicit() {
    let mut backoff = Backoff::new(1, 30);
    assert_eq!(backoff.next_delay_seconds(), 1);
    assert_eq!(backoff.next_delay_seconds(), 2);
    for _ in 0..10 {
        backoff.next_delay_seconds();
    }
    assert_eq!(backoff.next_delay_seconds(), 30);
    backoff.reset();
    assert_eq!(backoff.next_delay_seconds(), 1);
    assert_eq!(ConnectionState::Ready.to_string(), "Ready");
}

#[test]
fn protocol_serializes_server_envelope_and_classifies_sequence_error() {
    let envelope =
        EnvironmentEnvelope::new("device-a", "wifi", 7, serde_json::json!({"networks": []}));
    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["type"], "environment_data");
    assert_eq!(json["version"], 1);
    let error = ErrorFrame {
        code: "sequence_rejected".into(),
        message: "sequence is not newer".into(),
        retryable: true,
    };
    assert!(error.requires_sequence_recovery());
}

#[test]
fn queue_recovery_resets_in_flight_items() {
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let queue = UploadQueue::new(store, 10);
    let envelope = EnvironmentEnvelope::new("device-a", "wifi", 1, serde_json::json!({}));
    queue.enqueue(&envelope).unwrap();
    let claimed = queue.claim_next().unwrap().unwrap();
    assert_eq!(claimed.envelope.sequence, 1);
    queue.recover_in_flight().unwrap();
    assert_eq!(queue.pending_count().unwrap(), 1);
}

#[test]
fn transport_classifies_ack_errors_and_heartbeat_contract() {
    assert_eq!(
        classify_server_message("data_result", None),
        ServerEvent::Ack
    );
    assert_eq!(
        classify_server_message("error", Some("sequence_rejected")),
        ServerEvent::SequenceRejected
    );
    assert_eq!(
        classify_server_message("error", Some("rate_limited")),
        ServerEvent::RetryableError
    );
    let monitor = HeartbeatMonitor::new(
        std::time::Duration::from_secs(15),
        std::time::Duration::from_secs(45),
    );
    assert_eq!(monitor.interval(), std::time::Duration::from_secs(15));
    assert!(!monitor.is_timed_out());
}

#[test]
fn runtime_assigns_sequence_and_queues_mock_event_without_claiming_real_scan() {
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let mut runtime = remote_env_core::runtime::Runtime::new("device-a", store, 10);
    let envelope = runtime
        .submit_event(CollectorEvent {
            data_type: "wifi".into(),
            timestamp_ms: 1,
            data: serde_json::json!({"mock": true}),
        })
        .unwrap();
    assert_eq!(envelope.sequence, 1);
    let status = runtime.status().unwrap();
    assert_eq!(status.pending, 1);
    assert_eq!(status.in_flight, 0);
    assert_eq!(status.blocked, 0);
    assert_eq!(status.wifi, CollectorStatus::NotImplemented);
}

#[test]
fn configuration_rejects_rate_limit_above_server_limit() {
    let mut config = ClientConfig::default();
    config.token = "secret".into();
    config.identity.device_id = "device-a".into();
    config.max_uploads_per_minute = 61;
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn websocket_fixture_authenticates_uploads_and_requires_matching_ack() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let auth = socket.next().await.unwrap().unwrap();
        let auth: serde_json::Value = serde_json::from_str(auth.to_text().unwrap()).unwrap();
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
        let upload = socket.next().await.unwrap().unwrap();
        let upload: serde_json::Value = serde_json::from_str(upload.to_text().unwrap()).unwrap();
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "data_result", "success": true,
                    "device_id": upload["device_id"], "data_type": upload["data_type"],
                    "sequence": upload["sequence"]
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        socket.close(None).await.unwrap();
    });
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    let queue = UploadQueue::new(store, 10);
    let envelope =
        EnvironmentEnvelope::new("device-a", "wifi", 1, serde_json::json!({"mock": true}));
    queue.enqueue(&envelope).unwrap();
    let identity = remote_env_core::config::DeviceIdentity {
        device_id: "device-a".into(),
        device_name: "fixture".into(),
        platform: "test".into(),
        platform_version: "1".into(),
        client_version: "1".into(),
        hardware: None,
    };
    let mut manager =
        remote_env_core::transport::WebSocketManager::new(std::time::Duration::from_millis(20));
    let result = manager
        .run_once(&format!("ws://{address}"), "test-token", &identity, &queue)
        .await;
    assert!(result.is_err());
    assert_eq!(queue.pending_count().unwrap(), 0);
    server.await.unwrap();
}

#[test]
fn invalid_queue_capacity_is_rejected() {
    let dir = tempdir().unwrap();
    let store = StateStore::open(dir.path().join("state.sqlite3")).unwrap();
    assert!(matches!(
        UploadQueue::try_new(store, 0),
        Err(QueueError::InvalidCapacity)
    ));
}
