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
#[cfg(not(target_os = "android"))]
use tokio_tungstenite::Connector;
use tokio_tungstenite::{connect_async_tls_with_config, tungstenite::Message};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerWorkerStatus {
    pub profile_id: String,
    pub connection: ConnectionState,
    pub heartbeat_alive: bool,
    pub last_heartbeat_ms: Option<i64>,
    pub last_error: Option<String>,
    pub next_retry_at_ms: Option<i64>,
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
            next_retry_at_ms: None,
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
    #[error("服务器触发限速，延长等待后自动重试")]
    RateLimited,
    #[error("worker protocol failed")]
    Protocol,
    #[error("服务器拒绝请求，将自动重试: {0}")]
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
        }
    }

    async fn run(mut self, mut stop: oneshot::Receiver<()>) {
        let mut backoff = Backoff::new(1, 60);
        let mut rate_limit_delay = 30u64;
        loop {
            self.publish(ConnectionState::Connecting);
            // Cancelling the whole connection also interrupts authentication and writes.
            let result = tokio::select! {
                biased;
                _ = &mut stop => Err(WorkerError::Stopped),
                result = self.run_connection(&mut backoff, &mut rate_limit_delay) => result,
            };
            if let Err(error) = &result {
                if !matches!(error, WorkerError::Stopped) {
                    self.status.failed = self.status.failed.saturating_add(1);
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
            let delay = if matches!(result, Err(WorkerError::RateLimited)) {
                let delay = Duration::from_secs(rate_limit_delay);
                rate_limit_delay = rate_limit_delay.saturating_mul(2).min(300);
                delay
            } else {
                backoff.delay()
            };
            self.status.next_retry_at_ms = Some(now_ms() + delay.as_millis() as i64);
            self.publish(ConnectionState::Reconnecting);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                _ = &mut stop => { self.publish(ConnectionState::Stopped); return; }
            }
        }
    }

    async fn run_connection(
        &mut self,
        backoff: &mut Backoff,
        rate_limit_delay: &mut u64,
    ) -> Result<(), WorkerError> {
        #[cfg(not(target_os = "android"))]
        let connector = if self.profile.url.trim_start().starts_with("wss://") {
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
        };
        #[cfg(target_os = "android")]
        let connector = if self.profile.url.trim_start().starts_with("wss://") {
            Some(crate::transport::insecure_tls_connector())
        } else {
            None
        };
        let connect = connect_async_tls_with_config(&self.profile.url, None, false, connector);
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(15), connect)
            .await
            .map_err(|_| WorkerError::Connection("连接超时（15 秒）".into()))?
            .map_err(|error| {
                WorkerError::Connection(format!("连接 {url}: {error}", url = self.profile.url))
            })?;
        self.publish(ConnectionState::Connected);
        let auth = AuthFrame::collector(
            &self.profile.token,
            &self.identity,
            AuthFrame::platform_capabilities(&self.identity),
        );
        send_message(
            &mut socket,
            Message::Text(
                serde_json::to_string(&auth)
                    .map_err(|_| WorkerError::Protocol)?
                    .into(),
            ),
        )
        .await?;
        self.publish(ConnectionState::Authenticating);
        let auth_result = handshake_json(&mut socket).await?;
        if auth_result["code"] == "rate_limited" {
            return Err(WorkerError::RateLimited);
        }
        if auth_result["type"] != "auth_result" || auth_result["success"] != true {
            return Err(WorkerError::Blocked(format!(
                "服务器认证拒绝: {}",
                auth_result
            )));
        }
        let device_list = handshake_json(&mut socket).await?;
        if device_list["code"] == "rate_limited" {
            return Err(WorkerError::RateLimited);
        }
        if device_list["type"] != "device_list" {
            return Err(WorkerError::Blocked(format!(
                "服务器协议拒绝: 认证后未收到 device_list，实际响应: {}",
                device_list
            )));
        }
        // Heartbeat deadlines belong to this connection, never to a previous socket.
        // Start the deadline now so a server that never sends a pong also times out.
        let mut heartbeat_monitor =
            HeartbeatMonitor::new(self.heartbeat_interval, Duration::from_secs(60));
        heartbeat_monitor.mark_pong();
        self.publish(ConnectionState::Ready);
        self.status.last_error = None;
        let _ = self.status_tx.send(self.status.clone());
        let mut heartbeat = tokio::time::interval(self.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut delivery_poll = tokio::time::interval(Duration::from_millis(50));
        delivery_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut in_flight = None;
        let mut in_flight_since: Option<tokio::time::Instant> = None;
        let mut upload_watchdog = tokio::time::interval(Duration::from_secs(1));
        upload_watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
                    send_message(
                        &mut socket,
                        Message::Text(
                            serde_json::to_string(&item.1)
                                .map_err(|_| WorkerError::Protocol)?
                                .into(),
                        ),
                    )
                    .await?;
                    in_flight = Some(item);
                    in_flight_since = Some(tokio::time::Instant::now());
                    self.refresh_counts();
                }
            }
            tokio::select! {
                _ = delivery_poll.tick(), if in_flight.is_none() => {}
                _ = upload_watchdog.tick(), if in_flight.is_some() => {
                    if in_flight_since.is_some_and(|started| started.elapsed() >= Duration::from_secs(30)) {
                        return Err(WorkerError::Connection("等待上传 ACK 超时（30 秒），将重新连接并重试".into()));
                    }
                }
                _ = heartbeat.tick() => {
                    if heartbeat_monitor.is_timed_out() {
                        return Err(WorkerError::Connection("heartbeat pong timeout (60 seconds)".into()));
                    }
                    // The server contract is JSON text ping every five seconds.
                    send_message(&mut socket, Message::Text(r#"{"type":"ping"}"#.into())).await?;
                }
                message = socket.next() => {
                    let Some(message) = message else { return Err(WorkerError::Transport); };
                    let message = message.map_err(|_| WorkerError::Transport)?;
                    let value = match message {
                        Message::Text(text) => serde_json::from_str::<Value>(&text).map_err(|_| WorkerError::Protocol)?,
                        Message::Pong(_) => {
                            heartbeat_monitor.mark_pong();
                            self.mark_heartbeat();
                            continue;
                        }
                        Message::Ping(payload) => {
                            send_message(&mut socket, Message::Pong(payload)).await?;
                            continue;
                        }
                        Message::Close(_) => return Err(WorkerError::Transport),
                        Message::Binary(_) | Message::Frame(_) => continue,
                    };
                    match classify_server_message(value["type"].as_str().unwrap_or_default(), value["code"].as_str()) {
                        ServerEvent::Pong => { heartbeat_monitor.mark_pong(); self.mark_heartbeat(); }
                        ServerEvent::Invalid if value["type"] == "ping" => {
                            send_message(&mut socket, Message::Text(r#"{"type":"pong"}"#.into())).await?;
                            heartbeat_monitor.mark_pong();
                            self.mark_heartbeat();
                        }
                        ServerEvent::Ack => {
                            let Some((id, envelope)) = in_flight.take() else { continue; };
                            in_flight_since = None;
                            let ack: Ack = serde_json::from_value(value).map_err(|_| WorkerError::Protocol)?;
                            if !self.dispatcher.acknowledge(&self.profile.id, id, &ack, &envelope)? { return Err(WorkerError::Protocol); }
                            self.status.uploaded = self.status.uploaded.saturating_add(1);
                            // Reset backoff only after a confirmed upload.
                            backoff.reset();
                            *rate_limit_delay = 30;
                            self.refresh_counts();
                        }
                        ServerEvent::FatalError if value["code"] == "unknown_device" => {
                            if let Some((id, envelope)) = in_flight.take() {
                                in_flight_since = None;
                                self.dispatcher.block(&self.profile.id, id)?;
                                self.status.last_error = Some(format!("服务器拒绝数据设备身份，已隔离该条投递: profile_id={}, auth_device_id={}, envelope_device_id={}, response={}", self.profile.id, self.identity.device_id, envelope.device_id, value));
                                self.refresh_counts();
                                continue;
                            }
                            return Err(WorkerError::Blocked(format!("服务器拒绝设备身份: {}", value)));
                        }
                        ServerEvent::SequenceRejected => {
                            if let Some((_id, envelope)) = in_flight.take() {
                                let next = (now_ms() as u64).max(envelope.sequence.saturating_add(1));
                                self.dispatcher.rebase_target_sequences(&self.profile.id, &envelope.device_id, &envelope.data_type, next)?;
                                self.status.last_error = Some(format!("服务器拒绝旧序号，已重置本地序号基线并准备重传: {}", value));
                                self.refresh_counts();
                            }
                        }
                        // Unknown application frames are transport errors, not permanent auth failures.
                        // Reconnect so one malformed/unsupported frame cannot stop collection forever.
                        ServerEvent::Invalid => return Err(WorkerError::Transport),
                        ServerEvent::FatalError => {
                            if value["retryable"].as_bool() == Some(true) {
                                return Err(WorkerError::RateLimited);
                            }
                            if let Some((id, envelope)) = in_flight.take() {
                                in_flight_since = None;
                                self.dispatcher.block(&self.profile.id, id)?;
                                self.status.last_error = Some(format!(
                                    "服务器拒绝单条上传，已隔离并继续后续投递: data_type={}, sequence={}, response={}",
                                    envelope.data_type, envelope.sequence, value
                                ));
                                self.refresh_counts();
                                continue;
                            }
                            return Err(WorkerError::Blocked(format!("服务器拒绝连接: {}", value)));
                        }
                        ServerEvent::RetryableError => {
                            return Err(WorkerError::RateLimited);
                        }
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
        if connection != ConnectionState::Reconnecting {
            self.status.next_retry_at_ms = None;
        }
        self.status.connection = connection;
        self.status.heartbeat_alive = false;
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

async fn send_message<S>(socket: &mut S, message: Message) -> Result<(), WorkerError>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(15), socket.send(message))
        .await
        .map_err(|_| WorkerError::Connection("发送超时（15 秒）".into()))?
        .map_err(|_| WorkerError::Transport)
}

async fn handshake_json<S>(socket: &mut S) -> Result<Value, WorkerError>
where
    S: StreamExt + Unpin,
    S::Item: Into<Result<Message, tokio_tungstenite::tungstenite::Error>>,
{
    tokio::time::timeout(Duration::from_secs(15), next_json(socket))
        .await
        .map_err(|_| WorkerError::Connection("等待认证响应超时（15 秒）".into()))?
}

async fn next_json<S>(socket: &mut S) -> Result<Value, WorkerError>
where
    S: StreamExt + Unpin,
    S::Item: Into<Result<Message, tokio_tungstenite::tungstenite::Error>>,
{
    loop {
        let message = socket
            .next()
            .await
            .ok_or(WorkerError::Transport)?
            .into()
            .map_err(|_| WorkerError::Transport)?;
        match message {
            Message::Text(text) => {
                return serde_json::from_str::<Value>(&text).map_err(|_| WorkerError::Protocol);
            }
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {
                continue;
            }
            Message::Close(_) => return Err(WorkerError::Transport),
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
