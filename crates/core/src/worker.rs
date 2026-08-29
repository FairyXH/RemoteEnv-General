use crate::config::{DeviceIdentity, ServerProfile};
use crate::dispatcher::UploadDispatcher;
use crate::protocol::{Ack, AuthFrame};
use crate::transport::{
    Backoff, ConnectionState, HeartbeatMonitor, ServerEvent, classify_server_message,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerWorkerStatus {
    pub profile_id: String,
    pub connection: ConnectionState,
    pub heartbeat_alive: bool,
    pub last_heartbeat_ms: Option<i64>,
    pub last_error: Option<String>,
    pub pending: usize,
    pub in_flight: usize,
    pub blocked: usize,
    pub uploaded: u64,
    pub failed: u64,
}

impl ServerWorkerStatus {
    pub fn new(profile_id: String) -> Self {
        Self {
            profile_id,
            connection: ConnectionState::Stopped,
            heartbeat_alive: false,
            last_heartbeat_ms: None,
            last_error: None,
            pending: 0,
            in_flight: 0,
            blocked: 0,
            uploaded: 0,
            failed: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("worker stopped")]
    Stopped,
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("worker transport failed")]
    Transport,
    #[error("worker protocol failed")]
    Protocol,
    #[error("worker authentication is blocked: {0}")]
    Blocked(String),
    #[error("worker dispatcher failed: {0}")]
    Dispatcher(#[from] crate::dispatcher::DispatcherError),
}

pub struct WorkerHandle {
    pub profile_id: String,
    pub profile: ServerProfile,
    pub status: watch::Receiver<ServerWorkerStatus>,
    stop: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl WorkerHandle {
    pub fn start(
        profile: ServerProfile,
        dispatcher: UploadDispatcher,
        identity: DeviceIdentity,
        heartbeat_interval: Duration,
    ) -> Self {
        let profile_id = profile.id.clone();
        let (status_tx, status) = watch::channel(ServerWorkerStatus::new(profile_id.clone()));
        let (stop, stop_rx) = oneshot::channel();
        let join = tokio::spawn(
            ServerWorker::new(
                profile.clone(),
                dispatcher,
                identity,
                status_tx,
                heartbeat_interval,
            )
            .run(stop_rx),
        );
        Self {
            profile_id,
            profile,
            status,
            stop: Some(stop),
            join: Some(join),
        }
    }

    pub fn abort(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }

    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

pub struct ServerWorker {
    profile: ServerProfile,
    dispatcher: UploadDispatcher,
    identity: DeviceIdentity,
    status_tx: watch::Sender<ServerWorkerStatus>,
    status: ServerWorkerStatus,
    heartbeat_interval: Duration,
    heartbeat_monitor: HeartbeatMonitor,
}

impl ServerWorker {
    fn new(
        profile: ServerProfile,
        dispatcher: UploadDispatcher,
        identity: DeviceIdentity,
        status_tx: watch::Sender<ServerWorkerStatus>,
        heartbeat_interval: Duration,
    ) -> Self {
        let status = ServerWorkerStatus::new(profile.id.clone());
        Self {
            profile,
            dispatcher,
            identity,
            status_tx,
            status,
            heartbeat_interval,
            heartbeat_monitor: HeartbeatMonitor::new(
                heartbeat_interval,
                heartbeat_interval.saturating_mul(3),
            ),
        }
    }

    async fn run(mut self, mut stop: oneshot::Receiver<()>) {
        let mut backoff = Backoff::new(1, 60);
        loop {
            self.publish(ConnectionState::Connecting);
            let result = self.run_connection(&mut stop).await;
            if let Err(error) = &result {
                if !matches!(error, WorkerError::Stopped) {
                    self.status.last_error = Some(error.to_string());
                    let _ = self.status_tx.send(self.status.clone());
                }
            }
            if matches!(result, Err(WorkerError::Stopped)) {
                self.publish(ConnectionState::Stopped);
                return;
            }
            let _ = self.dispatcher.recover(&self.profile.id);
            self.refresh_counts();
            if matches!(result, Err(WorkerError::Blocked(_))) {
                self.publish(ConnectionState::Blocked);
                return;
            }
            if result.is_ok() {
                backoff.reset();
            }
            self.publish(ConnectionState::Reconnecting);
            let delay = backoff.delay();
            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                _ = &mut stop => { self.publish(ConnectionState::Stopped); return; }
            }
        }
    }

    async fn run_connection(
        &mut self,
        stop: &mut oneshot::Receiver<()>,
    ) -> Result<(), WorkerError> {
        let connect = connect_async_tls_with_config(
            &self.profile.url,
            None,
            false,
            if self.profile.url.trim_start().starts_with("wss://") {
                Some(Connector::NativeTls(
                    native_tls::TlsConnector::builder()
                        .danger_accept_invalid_certs(true)
                        .danger_accept_invalid_hostnames(true)
                        .build()
                        .map_err(|error| WorkerError::Blocked(format!("TLS 配置失败: {error}")))?
                        .into(),
                ))
            } else {
                None
            },
        );
        let (mut socket, _) = tokio::select! {
            _ = &mut *stop => return Err(WorkerError::Stopped),
            result = connect => result.map_err(|error| WorkerError::Connection(format!("连接 {url}: {error}", url = self.profile.url)))?,
        };
        self.publish(ConnectionState::Connected);
        let auth = AuthFrame::collector(
            &self.profile.token,
            &self.identity,
            vec!["wifi".into(), "ble".into(), "bluetooth".into()],
        );
        socket
            .send(Message::Text(
                serde_json::to_string(&auth)
                    .map_err(|_| WorkerError::Protocol)?
                    .into(),
            ))
            .await
            .map_err(|_| WorkerError::Transport)?;
        self.publish(ConnectionState::Authenticating);
        let auth_result = tokio::select! {
            _ = &mut *stop => return Err(WorkerError::Stopped),
            result = next_json(&mut socket) => result?,
        };
        if auth_result["type"] != "auth_result" || auth_result["success"] != true {
            return Err(WorkerError::Blocked(format!(
                "服务器认证拒绝: {}",
                auth_result
            )));
        }
        let device_list = tokio::select! {
            _ = &mut *stop => return Err(WorkerError::Stopped),
            result = next_json(&mut socket) => result?,
        };
        if device_list["type"] != "device_list" {
            return Err(WorkerError::Blocked(format!(
                "服务器协议拒绝: 认证后未收到 device_list，实际响应: {}",
                device_list
            )));
        }
        self.publish(ConnectionState::Ready);
        self.heartbeat_monitor.mark_pong();
        self.mark_heartbeat();
        let mut heartbeat = tokio::time::interval(self.heartbeat_interval);
        let mut in_flight = None;
        loop {
            if in_flight.is_none() {
                if let Some(item) = self.dispatcher.claim_next(&self.profile.id)? {
                    if item.1.device_id != self.identity.device_id {
                        self.dispatcher.cancel_delivery(&self.profile.id, item.0)?;
                        self.status.last_error = Some(format!(
                            "本地队列存在其他设备数据，已跳过: auth_device_id={}, envelope_device_id={}, profile_id={}",
                            self.identity.device_id, item.1.device_id, self.profile.id
                        ));
                        self.refresh_counts();
                        continue;
                    }
                    socket
                        .send(Message::Text(
                            serde_json::to_string(&item.1)
                                .map_err(|_| WorkerError::Protocol)?
                                .into(),
                        ))
                        .await
                        .map_err(|_| WorkerError::Transport)?;
                    in_flight = Some(item);
                    self.refresh_counts();
                }
            }
            tokio::select! {
                _ = &mut *stop => return Err(WorkerError::Stopped),
                _ = heartbeat.tick() => {
                    if self.heartbeat_monitor.is_timed_out() {
                        return Err(WorkerError::Transport);
                    }
                    socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.map_err(|_| WorkerError::Transport)?;
                }
                message = socket.next() => {
                    let Some(message) = message else { return Err(WorkerError::Transport); };
                    let message = message.map_err(|_| WorkerError::Transport)?;
                    let value = match message {
                        Message::Text(text) => serde_json::from_str::<Value>(&text).map_err(|_| WorkerError::Protocol)?,
                        Message::Pong(_) => {
                            self.heartbeat_monitor.mark_pong();
                            self.mark_heartbeat();
                            continue;
                        }
                        Message::Ping(payload) => {
                            socket.send(Message::Pong(payload)).await.map_err(|_| WorkerError::Transport)?;
                            continue;
                        }
                        Message::Close(_) => return Err(WorkerError::Transport),
                        Message::Binary(_) | Message::Frame(_) => continue,
                    };
                    match classify_server_message(value["type"].as_str().unwrap_or_default(), value["code"].as_str()) {
                        ServerEvent::Pong => { self.heartbeat_monitor.mark_pong(); self.mark_heartbeat(); }
                        ServerEvent::Invalid if value["type"] == "ping" => {
                            socket.send(Message::Text(r#"{"type":"pong"}"#.into())).await.map_err(|_| WorkerError::Transport)?;
                            self.heartbeat_monitor.mark_pong();
                            self.mark_heartbeat();
                        }
                        ServerEvent::Ack => {
                            let Some((id, envelope)) = in_flight.take() else { continue; };
                            let ack: Ack = serde_json::from_value(value).map_err(|_| WorkerError::Protocol)?;
                            if !self.dispatcher.acknowledge(&self.profile.id, id, &ack, &envelope)? { return Err(WorkerError::Protocol); }
                            self.status.uploaded = self.status.uploaded.saturating_add(1);
                            self.refresh_counts();
                        }
                        ServerEvent::FatalError if value["code"] == "unknown_device" => {
                            if let Some((id, envelope)) = in_flight.take() {
                                self.dispatcher.block(&self.profile.id, id)?;
                                self.status.last_error = Some(format!("服务器拒绝数据设备身份: profile_id={}, auth_device_id={}, envelope_device_id={}, response={}", self.profile.id, self.identity.device_id, envelope.device_id, value));
                                self.refresh_counts();
                                return Err(WorkerError::Blocked(self.status.last_error.clone().unwrap()));
                            }
                            return Err(WorkerError::Blocked(format!("服务器拒绝设备身份: {}", value)));
                        }
                        ServerEvent::SequenceRejected => {
                            if let Some((_id, envelope)) = in_flight.take() {
                                let next = envelope.sequence.saturating_add(1);
                                self.dispatcher.rebase_target_sequences(&self.profile.id, &envelope.device_id, &envelope.data_type, next)?;
                                self.status.last_error = Some(format!("服务器拒绝旧序号，已重置本地序号基线并准备重传: {}", value));
                                self.refresh_counts();
                            }
                        }
                        // Unknown application frames are transport errors, not permanent auth failures.
                        // Reconnect so one malformed/unsupported frame cannot stop collection forever.
                        ServerEvent::Invalid => return Err(WorkerError::Transport),
                        ServerEvent::FatalError => {
                            if let Some((id, _)) = in_flight.take() { self.dispatcher.block(&self.profile.id, id)?; }
                            return Err(WorkerError::Blocked(format!("服务器拒绝连接/上传: {}", value)));
                        }
                        ServerEvent::RetryableError => return Err(WorkerError::Connection(format!("服务器返回可重试错误: {}", value))),
                        _ => {}
                    }
                }
            }
        }
    }

    fn mark_heartbeat(&mut self) {
        self.status.heartbeat_alive = true;
        self.status.last_heartbeat_ms = Some(now_ms());
        let _ = self.status_tx.send(self.status.clone());
    }

    fn publish(&mut self, connection: ConnectionState) {
        self.status.connection = connection;
        self.status.heartbeat_alive = connection == ConnectionState::Ready;
        if connection != ConnectionState::Ready {
            self.status.last_heartbeat_ms = None;
        }
        self.refresh_counts();
        let _ = self.status_tx.send(self.status.clone());
    }
    fn refresh_counts(&mut self) {
        if let Ok(status) = self
            .dispatcher
            .target_status(&self.profile.id, self.status.connection)
        {
            self.status.pending = status.pending;
            self.status.in_flight = status.in_flight;
            self.status.blocked = status.blocked;
            let _ = self.status_tx.send(self.status.clone());
        }
    }
}

async fn next_json<S>(socket: &mut S) -> Result<Value, WorkerError>
where
    S: StreamExt + Unpin,
    S::Item: Into<Result<Message, tokio_tungstenite::tungstenite::Error>>,
{
    let message = socket
        .next()
        .await
        .ok_or(WorkerError::Transport)?
        .into()
        .map_err(|_| WorkerError::Transport)?;
    serde_json::from_str(message.to_text().map_err(|_| WorkerError::Protocol)?)
        .map_err(|_| WorkerError::Protocol)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
