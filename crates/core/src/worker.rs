use crate::config::{DeviceIdentity, ServerProfile};
use crate::dispatcher::UploadDispatcher;
use crate::protocol::{Ack, AuthFrame, HeartbeatFrame};
use crate::transport::{
    Backoff, ConnectionState, HeartbeatMonitor, ServerEvent, classify_server_message,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerWorkerStatus {
    pub profile_id: String,
    pub connection: ConnectionState,
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
        let mut backoff = Backoff::new(1, 30);
        loop {
            self.publish(ConnectionState::Connecting);
            let result = self.run_connection(&mut stop).await;
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
        let connect = connect_async(&self.profile.url);
        let (mut socket, _) = tokio::select! {
            _ = &mut *stop => return Err(WorkerError::Stopped),
            result = connect => result.map_err(|_| WorkerError::Transport)?,
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
            return Err(WorkerError::Blocked(
                auth_result["message"]
                    .as_str()
                    .unwrap_or("authentication failed")
                    .to_string(),
            ));
        }
        let device_list = tokio::select! {
            _ = &mut *stop => return Err(WorkerError::Stopped),
            result = next_json(&mut socket) => result?,
        };
        if device_list["type"] != "device_list" {
            return Err(WorkerError::Blocked(
                "device_list required after auth_result".into(),
            ));
        }
        self.publish(ConnectionState::Ready);
        self.heartbeat_monitor.mark_pong();
        let mut heartbeat = tokio::time::interval(self.heartbeat_interval);
        let mut in_flight = None;
        loop {
            if in_flight.is_none() {
                if let Some(item) = self.dispatcher.claim_next(&self.profile.id)? {
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
                    let frame = HeartbeatFrame { r#type: "heartbeat".into(), timestamp: now_ms() };
                    socket.send(Message::Text(serde_json::to_string(&frame).map_err(|_| WorkerError::Protocol)?.into())).await.map_err(|_| WorkerError::Transport)?;
                }
                message = socket.next() => {
                    let Some(message) = message else { return Err(WorkerError::Transport); };
                    let value = serde_json::from_str::<Value>(message.map_err(|_| WorkerError::Transport)?.to_text().map_err(|_| WorkerError::Protocol)?).map_err(|_| WorkerError::Protocol)?;
                    match classify_server_message(value["type"].as_str().unwrap_or_default(), value["code"].as_str()) {
                        ServerEvent::Pong => self.heartbeat_monitor.mark_pong(),
                        ServerEvent::Ack => {
                            let Some((id, envelope)) = in_flight.take() else { continue; };
                            let ack: Ack = serde_json::from_value(value).map_err(|_| WorkerError::Protocol)?;
                            if !self.dispatcher.acknowledge(&self.profile.id, id, &ack, &envelope)? { return Err(WorkerError::Protocol); }
                            self.status.uploaded = self.status.uploaded.saturating_add(1);
                            self.refresh_counts();
                        }
                        ServerEvent::SequenceRejected | ServerEvent::FatalError => {
                            if let Some((id, _)) = in_flight.take() { self.dispatcher.block(&self.profile.id, id)?; }
                            return Err(WorkerError::Blocked(value["message"].as_str().unwrap_or("server rejected upload").into()));
                        }
                        ServerEvent::RetryableError => return Err(WorkerError::Transport),
                        _ => {}
                    }
                }
            }
        }
    }

    fn publish(&mut self, connection: ConnectionState) {
        self.status.connection = connection;
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
