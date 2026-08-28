use crate::config::{ClientConfig, ServerProfile};
use crate::protocol::{Ack, EnvironmentEnvelope};
use crate::state::{StateError, StateStore};
use crate::transport::ConnectionState;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    InFlight,
    Completed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRuntimeStatus {
    pub server_id: String,
    pub state: String,
    pub pending: usize,
    pub in_flight: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerTargetStatus {
    pub profile_id: String,
    pub connection: ConnectionState,
    pub pending: usize,
    pub in_flight: usize,
    pub blocked: usize,
}

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
pub enum DispatcherError {
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("no upload target selected")]
    NoTargets,
}

#[derive(Clone)]
pub struct UploadDispatcher {
    store: StateStore,
}

impl UploadDispatcher {
    pub fn new(store: StateStore) -> Self {
        Self { store }
    }

    pub fn resolve_targets<'a>(&self, config: &'a ClientConfig) -> Vec<&'a ServerProfile> {
        config.selected_servers()
    }

    pub fn persist_event(
        &self,
        config: &ClientConfig,
        envelope: &EnvironmentEnvelope,
    ) -> Result<usize, DispatcherError> {
        let targets = self.resolve_targets(config);
        if targets.is_empty() {
            return Err(DispatcherError::NoTargets);
        }
        let mut created = 0;
        for target in targets {
            if self.store.enqueue_target(&target.id, envelope)? {
                created += 1;
            }
        }
        Ok(created)
    }

    pub fn claim_next(
        &self,
        server_id: &str,
    ) -> Result<Option<(i64, EnvironmentEnvelope)>, DispatcherError> {
        let Some((id, envelope)) = self.store.pending_target(server_id)?.into_iter().next() else {
            return Ok(None);
        };
        self.store.claim_target(server_id, id)?;
        Ok(Some((id, envelope)))
    }

    pub fn acknowledge(
        &self,
        server_id: &str,
        id: i64,
        ack: &Ack,
        envelope: &EnvironmentEnvelope,
    ) -> Result<bool, DispatcherError> {
        if !crate::protocol::matches_ack(ack, envelope) {
            return Ok(false);
        }
        self.store.acknowledge_target(server_id, id)?;
        Ok(true)
    }

    pub fn recover(&self, server_id: &str) -> Result<(), DispatcherError> {
        Ok(self.store.recover_target(server_id)?)
    }
    pub fn block(&self, server_id: &str, id: i64) -> Result<(), DispatcherError> {
        Ok(self.store.block_target(server_id, id)?)
    }
    pub fn cancel_target(&self, server_id: &str) -> Result<(), DispatcherError> {
        Ok(self.store.cancel_target(server_id)?)
    }

    pub fn selected_targets(&self, config: &ClientConfig) -> Vec<String> {
        self.resolve_targets(config)
            .into_iter()
            .map(|profile| profile.id.clone())
            .collect()
    }

    pub fn target_status(
        &self,
        profile_id: &str,
        connection: ConnectionState,
    ) -> Result<ServerTargetStatus, DispatcherError> {
        Ok(ServerTargetStatus {
            profile_id: profile_id.to_string(),
            connection,
            pending: self.store.count_target_status(profile_id, "pending")?,
            in_flight: self.store.count_target_status(profile_id, "in_flight")?,
            blocked: self.store.count_target_status(profile_id, "blocked")?,
        })
    }

    pub fn retry_delay(attempt: u32) -> Duration {
        Duration::from_secs(1_u64.checked_shl(attempt.min(5)).unwrap_or(30))
            .min(Duration::from_secs(30))
    }

    pub fn status(
        &self,
        server_id: &str,
        state: impl Into<String>,
    ) -> Result<ServerRuntimeStatus, DispatcherError> {
        Ok(ServerRuntimeStatus {
            server_id: server_id.into(),
            state: state.into(),
            pending: self.store.count_target_status(server_id, "pending")?,
            in_flight: self.store.count_target_status(server_id, "in_flight")?,
            blocked: self.store.count_target_status(server_id, "blocked")?,
        })
    }
}
