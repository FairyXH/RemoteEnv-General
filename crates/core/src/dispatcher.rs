use crate::config::{ClientConfig, DeviceIdentity, ServerProfile};
use crate::protocol::{Ack, EnvironmentEnvelope};
use crate::state::{StateError, StateStore};
use crate::transport::ConnectionState;
use crate::worker::WorkerHandle;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
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

pub struct DispatcherSupervisor {
    dispatcher: UploadDispatcher,
    identity: DeviceIdentity,
    workers: HashMap<String, WorkerHandle>,
    heartbeat_interval: Duration,
}

impl DispatcherSupervisor {
    pub fn new(
        dispatcher: UploadDispatcher,
        identity: DeviceIdentity,
        heartbeat_interval: Duration,
    ) -> Self {
        Self {
            dispatcher,
            identity,
            workers: HashMap::new(),
            heartbeat_interval,
        }
    }

    pub async fn apply_config(&mut self, config: &ClientConfig) {
        let selected = self.dispatcher.resolve_targets(config);
        let selected_ids: HashSet<&str> =
            selected.iter().map(|profile| profile.id.as_str()).collect();
        let removed: Vec<String> = self
            .workers
            .keys()
            .filter(|id| !selected_ids.contains(id.as_str()))
            .cloned()
            .collect();
        for id in removed {
            if let Some(worker) = self.workers.remove(&id) {
                worker.stop().await;
            }
            let _ = self.dispatcher.cancel_target(&id);
        }
        for profile in selected {
            let replace = self.workers.get(&profile.id).is_some_and(|worker| {
                worker.profile.url != profile.url || worker.profile.token != profile.token
            });
            if replace {
                if let Some(worker) = self.workers.remove(&profile.id) {
                    worker.stop().await;
                }
                let _ = self.dispatcher.recover(&profile.id);
            }
            if !self.workers.contains_key(&profile.id) {
                let worker = WorkerHandle::start(
                    profile.clone(),
                    self.dispatcher.clone(),
                    DeviceIdentity {
                        device_id: profile.device_id.clone(),
                        ..self.identity.clone()
                    },
                    self.heartbeat_interval,
                );
                self.workers.insert(profile.id.clone(), worker);
            }
        }
    }

    pub fn statuses(&self) -> Vec<crate::worker::ServerWorkerStatus> {
        self.workers
            .values()
            .map(|worker| worker.status.borrow().clone())
            .collect()
    }

    pub async fn stop(&mut self) {
        let workers = std::mem::take(&mut self.workers);
        for (_, worker) in workers {
            worker.stop().await;
        }
    }
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

    pub fn complete_delivery(&self, server_id: &str, id: i64) -> Result<(), DispatcherError> {
        Ok(self.store.acknowledge_target(server_id, id)?)
    }

    pub fn cancel_delivery(&self, server_id: &str, id: i64) -> Result<(), DispatcherError> {
        Ok(self.store.cancel_target_delivery(server_id, id)?)
    }

    pub fn cancel_target_except_device(&self, server_id: &str, device_id: &str) -> Result<(), DispatcherError> {
        Ok(self.store.cancel_target_except_device(server_id, device_id)?)
    }

    pub fn unblock_target(&self, target_id: &str) -> Result<(), DispatcherError> {
        Ok(self.store.unblock_target(target_id)?)
    }

    pub fn rebase_target_sequences(&self, target_id: &str, device_id: &str, data_type: &str, minimum: u64) -> Result<(), DispatcherError> {
        Ok(self.store.rebase_target_sequences(target_id, device_id, data_type, minimum)?)
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
