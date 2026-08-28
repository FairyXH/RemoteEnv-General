use crate::protocol::EnvironmentEnvelope;
use crate::state::{StateError, StateStore};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("queue capacity must be positive")]
    InvalidCapacity,
    #[error("queue is full (capacity {capacity})")]
    Full { capacity: usize },
    #[error("envelope is already queued")]
    Duplicate,
    #[error("state error: {0}")]
    State(#[from] StateError),
}

#[derive(Debug, Clone)]
pub struct QueuedEnvelope {
    pub id: i64,
    pub envelope: EnvironmentEnvelope,
}

#[derive(Clone)]
pub struct UploadQueue {
    store: StateStore,
    capacity: usize,
    operation_lock: Arc<Mutex<()>>,
}

impl UploadQueue {
    pub fn try_new(store: StateStore, capacity: usize) -> Result<Self, QueueError> {
        if capacity == 0 {
            return Err(QueueError::InvalidCapacity);
        }
        Ok(Self {
            store,
            capacity,
            operation_lock: Arc::new(Mutex::new(())),
        })
    }
    pub fn new(store: StateStore, capacity: usize) -> Self {
        if capacity == 0 {
            Self {
                store,
                capacity: 1,
                operation_lock: Arc::new(Mutex::new(())),
            }
        } else {
            Self {
                store,
                capacity,
                operation_lock: Arc::new(Mutex::new(())),
            }
        }
    }
    pub fn enqueue(&self, envelope: &EnvironmentEnvelope) -> Result<(), QueueError> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| QueueError::State(StateError::Poisoned))?;
        if self.store.count_pending()? >= self.capacity {
            return Err(QueueError::Full {
                capacity: self.capacity,
            });
        }
        if !self.store.enqueue(envelope)? {
            return Err(QueueError::Duplicate);
        }
        Ok(())
    }
    pub fn claim_next(&self) -> Result<Option<QueuedEnvelope>, QueueError> {
        let mut items = self.store.pending()?;
        let Some((id, envelope)) = items.drain(..).next() else {
            return Ok(None);
        };
        self.store.claim(id)?;
        Ok(Some(QueuedEnvelope { id, envelope }))
    }
    pub fn acknowledge(&self, item: &QueuedEnvelope) -> Result<(), QueueError> {
        self.store.acknowledge(item.id)?;
        Ok(())
    }
    pub fn recover_in_flight(&self) -> Result<(), QueueError> {
        self.store.recover_in_flight()?;
        Ok(())
    }
    pub fn block(&self, item: &QueuedEnvelope) -> Result<(), QueueError> {
        self.store.block(item.id)?;
        Ok(())
    }
    pub fn pending_count(&self) -> Result<usize, QueueError> {
        Ok(self.store.count_pending()?)
    }
}
