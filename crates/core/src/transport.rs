use crate::config::DeviceIdentity;
use crate::protocol::{Ack, AuthFrame, EnvironmentEnvelope, HeartbeatFrame};
use crate::queue::{QueueError, QueuedEnvelope, UploadQueue};
use crate::state::StateError;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Error)]
pub enum WebSocketError {
    #[error("transport error: {0}")]
    Transport(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("sequence rejected: {0}")]
    SequenceRejected(String),
}

pub struct WebSocketManager {
    pub state: ConnectionState,
    pub backoff: Backoff,
    heartbeat: HeartbeatMonitor,
}

impl WebSocketManager {
    pub fn new(heartbeat_interval: Duration) -> Self {
        Self {
            state: ConnectionState::Disconnected,
            backoff: Backoff::new(1, 30),
            heartbeat: HeartbeatMonitor::new(
                heartbeat_interval,
                heartbeat_interval.saturating_mul(3),
            ),
        }
    }

    pub fn heartbeat(&self) -> &HeartbeatMonitor {
        &self.heartbeat
    }

    pub fn stop(&mut self) {
        self.state = ConnectionState::Stopped;
    }

    pub fn mark_reconnecting(&mut self) {
        self.state = ConnectionState::Reconnecting;
    }

    pub async fn run_once(
        &mut self,
        server_url: &str,
        token: &str,
        identity: &DeviceIdentity,
        queue: &UploadQueue,
    ) -> Result<(), WebSocketError> {
        self.state = ConnectionState::Connecting;
        let (mut socket, _) = connect_async(server_url).await?;
        self.state = ConnectionState::Connected;
        let auth = AuthFrame::collector(
            token,
            identity,
            vec!["wifi".into(), "ble".into(), "bluetooth".into()],
        );
        socket
            .send(Message::Text(serde_json::to_string(&auth)?.into()))
            .await?;
        self.state = ConnectionState::Authenticating;
        let auth_result = socket.next().await.ok_or_else(|| {
            WebSocketError::Authentication("connection closed before auth_result".into())
        })??;
        let auth_value: Value = serde_json::from_str(auth_result.to_text()?)?;
        if classify_server_message(auth_value["type"].as_str().unwrap_or_default(), None)
            != ServerEvent::Authenticated
            || auth_value["success"] != true
        {
            return Err(WebSocketError::Authentication(
                auth_value["message"]
                    .as_str()
                    .unwrap_or("authentication failed")
                    .into(),
            ));
        }
        self.state = ConnectionState::Ready;
        self.backoff.reset();
        self.heartbeat.mark_pong();
        let device_list = socket.next().await.ok_or_else(|| {
            WebSocketError::Authentication("connection closed before device_list".into())
        })??;
        let device_list_value: Value = serde_json::from_str(device_list.to_text()?)?;
        if classify_server_message(device_list_value["type"].as_str().unwrap_or_default(), None)
            != ServerEvent::DeviceList
        {
            return Err(WebSocketError::Authentication(
                "device_list required after auth_result".into(),
            ));
        }
        while let Some(item) = queue.claim_next()? {
            socket
                .send(Message::Text(serde_json::to_string(&item.envelope)?.into()))
                .await?;
            if let Some(message) = socket.next().await {
                let value: Value = serde_json::from_str(message?.to_text()?)?;
                match classify_server_message(
                    value["type"].as_str().unwrap_or_default(),
                    value["code"].as_str(),
                ) {
                    ServerEvent::Ack => {
                        let ack = serde_json::from_value(value)?;
                        if crate::protocol::matches_ack(&ack, &item.envelope) {
                            queue.acknowledge(&item)?;
                        } else {
                            return Err(WebSocketError::Authentication(
                                "ACK does not match in-flight envelope".into(),
                            ));
                        }
                    }
                    ServerEvent::SequenceRejected => {
                        queue.block(&item)?;
                        return Err(WebSocketError::SequenceRejected(
                            value["message"].as_str().unwrap_or_default().into(),
                        ));
                    }
                    ServerEvent::RetryableError => {
                        return Err(WebSocketError::Authentication("upload rate limited".into()));
                    }
                    ServerEvent::FatalError => {
                        queue.block(&item)?;
                        return Err(WebSocketError::Authentication(
                            value["message"].as_str().unwrap_or_default().into(),
                        ));
                    }
                    _ => {}
                }
            } else {
                return Err(WebSocketError::Authentication(
                    "connection closed while uploading".into(),
                ));
            }
        }
        let heartbeat = HeartbeatFrame {
            r#type: "heartbeat".into(),
            timestamp: now_ms(),
        };
        socket
            .send(Message::Text(serde_json::to_string(&heartbeat)?.into()))
            .await?;
        if let Some(message) = socket.next().await {
            let value: Value = serde_json::from_str(message?.to_text()?)?;
            if classify_server_message(value["type"].as_str().unwrap_or_default(), None)
                == ServerEvent::Pong
            {
                self.heartbeat.mark_pong();
            }
        }
        if self.heartbeat.is_timed_out() {
            return Err(WebSocketError::Authentication("heartbeat timeout".into()));
        }
        Ok(())
    }

    pub fn next_reconnect_delay(&mut self) -> Duration {
        self.backoff.delay()
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn _queued_type_is_used(_: QueuedEnvelope) {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticating,
    Ready,
    Reconnecting,
    Stopped,
    Blocked,
}
impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone)]
pub struct Backoff {
    current: u64,
    base: u64,
    max: u64,
}
impl Backoff {
    pub fn new(base: u64, max: u64) -> Self {
        Self {
            current: base,
            base,
            max,
        }
    }
    pub fn next_delay_seconds(&mut self) -> u64 {
        let out = self.current;
        self.current = self.current.saturating_mul(2).min(self.max);
        out
    }
    pub fn reset(&mut self) {
        self.current = self.base
    }
    pub fn delay(&mut self) -> Duration {
        Duration::from_secs(self.next_delay_seconds())
    }
}

pub fn matches_ack(ack: &Ack, envelope: &EnvironmentEnvelope) -> bool {
    crate::protocol::matches_ack(ack, envelope)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerEvent {
    Authenticated,
    DeviceList,
    Pong,
    Ack,
    RetryableError,
    SequenceRejected,
    FatalError,
    Invalid,
}

pub fn classify_server_message(message_type: &str, error_code: Option<&str>) -> ServerEvent {
    match (message_type, error_code) {
        ("auth_result", _) => ServerEvent::Authenticated,
        ("device_list", _) => ServerEvent::DeviceList,
        ("pong", _) => ServerEvent::Pong,
        ("data_result", _) => ServerEvent::Ack,
        ("error", Some("sequence_rejected")) => ServerEvent::SequenceRejected,
        ("error", Some("rate_limited")) => ServerEvent::RetryableError,
        ("error", _) => ServerEvent::FatalError,
        _ => ServerEvent::Invalid,
    }
}

#[derive(Debug, Clone)]
pub struct HeartbeatMonitor {
    interval: Duration,
    timeout: Duration,
    last_pong: Option<std::time::Instant>,
}

impl HeartbeatMonitor {
    pub fn new(interval: Duration, timeout: Duration) -> Self {
        Self {
            interval,
            timeout,
            last_pong: None,
        }
    }
    pub fn interval(&self) -> Duration {
        self.interval
    }
    pub fn mark_pong(&mut self) {
        self.last_pong = Some(std::time::Instant::now());
    }
    pub fn is_timed_out(&self) -> bool {
        self.last_pong
            .is_some_and(|last| last.elapsed() > self.timeout)
    }
}
