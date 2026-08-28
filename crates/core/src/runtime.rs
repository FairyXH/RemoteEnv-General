use crate::collector::CollectorEvent;
use crate::protocol::EnvironmentEnvelope;
use crate::queue::{QueueError, UploadQueue};
use crate::state::{StateError, StateStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectorStatus {
    NotImplemented,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub wifi: CollectorStatus,
    pub ble: CollectorStatus,
    pub classic_bluetooth: CollectorStatus,
    pub pending: usize,
    pub uploaded: u64,
    pub failed: u64,
}

pub struct Runtime {
    device_id: String,
    store: StateStore,
    queue: UploadQueue,
    uploaded: u64,
    failed: u64,
}

impl Runtime {
    pub fn new(device_id: impl Into<String>, store: StateStore, max_queue_size: usize) -> Self {
        Self {
            device_id: device_id.into(),
            queue: UploadQueue::new(store.clone(), max_queue_size),
            store,
            uploaded: 0,
            failed: 0,
        }
    }

    pub fn submit_event(
        &mut self,
        event: CollectorEvent,
    ) -> Result<EnvironmentEnvelope, RuntimeError> {
        let sequence = self
            .store
            .next_sequence(&self.device_id, &event.data_type)?;
        let envelope = EnvironmentEnvelope::new(
            self.device_id.clone(),
            event.data_type,
            sequence,
            event.data,
        );
        self.queue.enqueue(&envelope)?;
        Ok(envelope)
    }

    pub fn queue(&self) -> &UploadQueue {
        &self.queue
    }
    pub fn record_uploaded(&mut self) {
        self.uploaded = self.uploaded.saturating_add(1);
    }
    pub fn record_failed(&mut self) {
        self.failed = self.failed.saturating_add(1);
    }

    pub fn status(&self) -> Result<RuntimeStatus, RuntimeError> {
        Ok(RuntimeStatus {
            wifi: CollectorStatus::NotImplemented,
            ble: CollectorStatus::NotImplemented,
            classic_bluetooth: CollectorStatus::NotImplemented,
            pending: self.queue.pending_count()?,
            uploaded: self.uploaded,
            failed: self.failed,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("state error: {0}")]
    State(#[from] StateError),
    #[error("queue error: {0}")]
    Queue(#[from] QueueError),
}
