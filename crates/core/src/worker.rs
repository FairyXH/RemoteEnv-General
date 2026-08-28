use crate::config::{DeviceIdentity, ServerProfile};
use crate::dispatcher::UploadDispatcher;
use crate::protocol::{AuthFrame, HeartbeatFrame};
use crate::transport::{Backoff, ConnectionState, ServerEvent, classify_server_message};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, Serialize)]
pub struct ServerWorkerStatus {
    pub profile_id: String,
    pub connection: ConnectionState,
    pub pending: usize,
    pub in_flight: usize,
    pub blocked: usize,
    pub uploaded: u64,
    pub failed: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("worker transport failed")]
    Transport,
    #[error("worker protocol failed")]
    Protocol,
    #[error("worker dispatcher failed: {0}")]
    Dispatcher(#[from] crate::dispatcher::DispatcherError),
}

pub struct ServerWorker {
    profile: ServerProfile,
    dispatcher: UploadDispatcher,
    identity: DeviceIdentity,
}

impl ServerWorker {
    pub fn new(
        profile: ServerProfile,
        dispatcher: UploadDispatcher,
        identity: DeviceIdentity,
    ) -> Self {
        Self {
            profile,
            dispatcher,
            identity,
        }
    }

    pub async fn run(self) {
        let mut backoff = Backoff::new(1, 30);
        loop {
            let result = self.run_connection().await;
            let _ = self.dispatcher.recover(&self.profile.id);
            if result.is_ok() {
                backoff.reset();
            }
            tokio::time::sleep(backoff.delay()).await;
        }
    }

    async fn run_connection(&self) -> Result<(), WorkerError> {
        let (mut socket, _) = connect_async(&self.profile.url)
            .await
            .map_err(|_| WorkerError::Transport)?;
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
        let auth_result = next_json(&mut socket).await?;
        if classify_server_message(auth_result["type"].as_str().unwrap_or_default(), None)
            != ServerEvent::Authenticated
            || auth_result["success"] != true
        {
            return Err(WorkerError::Protocol);
        }
        let devices = next_json(&mut socket).await?;
        if classify_server_message(devices["type"].as_str().unwrap_or_default(), None)
            != ServerEvent::DeviceList
        {
            return Err(WorkerError::Protocol);
        }
        loop {
            if let Some((id, envelope)) = self.dispatcher.claim_next(&self.profile.id)? {
                socket
                    .send(Message::Text(
                        serde_json::to_string(&envelope)
                            .map_err(|_| WorkerError::Protocol)?
                            .into(),
                    ))
                    .await
                    .map_err(|_| WorkerError::Transport)?;
                let response = next_json(&mut socket).await?;
                match classify_server_message(
                    response["type"].as_str().unwrap_or_default(),
                    response["code"].as_str(),
                ) {
                    ServerEvent::Ack => {
                        let ack =
                            serde_json::from_value(response).map_err(|_| WorkerError::Protocol)?;
                        if self
                            .dispatcher
                            .acknowledge(&self.profile.id, id, &ack, &envelope)?
                        {
                            continue;
                        }
                        return Err(WorkerError::Protocol);
                    }
                    ServerEvent::SequenceRejected | ServerEvent::FatalError => {
                        self.dispatcher.block(&self.profile.id, id)?;
                        return Err(WorkerError::Protocol);
                    }
                    _ => return Err(WorkerError::Protocol),
                }
            }
            let heartbeat = HeartbeatFrame {
                r#type: "heartbeat".into(),
                timestamp: now_ms(),
            };
            socket
                .send(Message::Text(
                    serde_json::to_string(&heartbeat)
                        .map_err(|_| WorkerError::Protocol)?
                        .into(),
                ))
                .await
                .map_err(|_| WorkerError::Transport)?;
            let pong = tokio::time::timeout(Duration::from_secs(45), next_json(&mut socket))
                .await
                .map_err(|_| WorkerError::Transport)??;
            if classify_server_message(pong["type"].as_str().unwrap_or_default(), None)
                != ServerEvent::Pong
            {
                return Err(WorkerError::Protocol);
            }
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
