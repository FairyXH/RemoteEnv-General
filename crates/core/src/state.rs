use crate::config::{ClientConfig, DeviceIdentity, LoggingLevel};
use crate::protocol::EnvironmentEnvelope;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("state is unavailable")]
    Poisoned,
}

#[derive(Clone)]
pub struct StateStore {
    connection: Arc<Mutex<Connection>>,
}

impl StateStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS sequences (device_id TEXT NOT NULL, data_type TEXT NOT NULL, value INTEGER NOT NULL, PRIMARY KEY(device_id, data_type)); CREATE TABLE IF NOT EXISTS upload_queue (id INTEGER PRIMARY KEY AUTOINCREMENT, envelope_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', UNIQUE(envelope_json)); CREATE INDEX IF NOT EXISTS idx_upload_queue_pending ON upload_queue(status, id); CREATE TABLE IF NOT EXISTS upload_deliveries (id INTEGER PRIMARY KEY AUTOINCREMENT, target_id TEXT NOT NULL, envelope_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', UNIQUE(target_id, envelope_json)); CREATE INDEX IF NOT EXISTS idx_upload_deliveries_pending ON upload_deliveries(target_id, status, id);")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StateError> {
        self.connection.lock().map_err(|_| StateError::Poisoned)
    }

    pub fn next_sequence(&self, device_id: &str, data_type: &str) -> Result<u64, StateError> {
        let connection = self.lock()?;
        let tx = connection.unchecked_transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT value FROM sequences WHERE device_id=?1 AND data_type=?2",
                params![device_id, data_type],
                |row| row.get(0),
            )
            .optional()?;
        let next = current
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(rusqlite::Error::InvalidQuery)?;
        tx.execute("INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=excluded.value", params![device_id, data_type, next])?;
        tx.commit()?;
        Ok(next as u64)
    }

    pub fn get_sequence(
        &self,
        device_id: &str,
        data_type: &str,
    ) -> Result<Option<u64>, StateError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT value FROM sequences WHERE device_id=?1 AND data_type=?2",
                params![device_id, data_type],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|v| v as u64))
    }

    pub fn latest_sequence(&self, device_id: &str, data_type: &str) -> Result<u64, StateError> {
        let connection = self.lock()?;
        let sequence = connection
            .query_row(
                "SELECT MAX(CAST(json_extract(envelope_json, '$.sequence') AS INTEGER)) FROM upload_deliveries WHERE json_extract(envelope_json, '$.device_id')=?1 AND json_extract(envelope_json, '$.data_type')=?2",
                params![device_id, data_type],
                |row| row.get::<_, Option<i64>>(0),
            )?
            .unwrap_or(0);
        let sequence_state = connection
            .query_row(
                "SELECT value FROM sequences WHERE device_id=?1 AND data_type=?2",
                params![device_id, data_type],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(sequence.max(sequence_state) as u64)
    }

    pub fn next_timestamp_sequence(&self, device_id: &str, data_type: &str) -> Result<u64, StateError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let connection = self.lock()?;
        let current: Option<i64> = connection
            .query_row(
                "SELECT value FROM sequences WHERE device_id=?1 AND data_type=?2",
                params![device_id, data_type],
                |row| row.get(0),
            )
            .optional()?;
        let latest = current.unwrap_or(0).max(timestamp as i64);
        let latest = latest.saturating_add(1);
        connection.execute("INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=excluded.value", params![device_id, data_type, latest])?;
        Ok(latest as u64)
    }

    pub fn rebase_target_sequences(&self, target_id: &str, device_id: &str, data_type: &str, minimum: u64) -> Result<(), StateError> {
        let c = self.lock()?;
        let mut stmt = c.prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight','blocked')")?;
        let rows = stmt.query_map(params![target_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut next = minimum;
        for (id, raw) in rows {
            let mut envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id == device_id && envelope.data_type == data_type && envelope.sequence < next {
                envelope.sequence = next;
                next = next.saturating_add(1);
                c.execute("UPDATE upload_deliveries SET status='pending', envelope_json=?1 WHERE target_id=?2 AND id=?3", params![serde_json::to_string(&envelope)?, target_id, id])?;
            }
        }
        c.execute("INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=MAX(value, excluded.value)", params![device_id, data_type, next as i64])?;
        Ok(())
    }

    pub fn load_or_create_identity(
        &self,
        name: &str,
        platform: &str,
        version: &str,
    ) -> Result<DeviceIdentity, StateError> {
        let connection = self.lock()?;
        if let Some(raw) = connection
            .query_row("SELECT value FROM metadata WHERE key='identity'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(serde_json::from_str(&raw)?);
        }
        let identity = DeviceIdentity {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: name.into(),
            platform: platform.into(),
            platform_version: version.into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            hardware: None,
        };
        connection.execute(
            "INSERT INTO metadata(key,value) VALUES('identity',?1)",
            params![serde_json::to_string(&identity)?],
        )?;
        Ok(identity)
    }

    pub fn save_config(&self, config: &ClientConfig) -> Result<(), StateError> {
        let connection = self.lock()?;
        connection.execute("INSERT INTO metadata(key,value) VALUES('config',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![serde_json::to_string(config)?])?;
        Ok(())
    }
    pub fn load_config(&self) -> Result<Option<ClientConfig>, StateError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row("SELECT value FROM metadata WHERE key='config'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|v| serde_json::from_str(&v))
            .transpose()?)
    }
    pub fn enqueue(&self, envelope: &EnvironmentEnvelope) -> Result<bool, StateError> {
        let connection = self.lock()?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO upload_queue(envelope_json,status) VALUES(?1,'pending')",
            params![serde_json::to_string(envelope)?],
        )?;
        Ok(inserted == 1)
    }
    pub fn pending(&self) -> Result<Vec<(i64, EnvironmentEnvelope)>, StateError> {
        let connection = self.lock()?;
        let mut stmt = connection.prepare(
            "SELECT id,envelope_json FROM upload_queue WHERE status='pending' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            let id = r.get(0)?;
            let raw = r.get::<_, String>(1)?;
            let envelope = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((id, envelope))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::Database)
    }
    pub fn claim(&self, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_queue SET status='in_flight' WHERE id=?1 AND status='pending'",
            params![id],
        )?;
        Ok(())
    }
    pub fn acknowledge(&self, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute("DELETE FROM upload_queue WHERE id=?1", params![id])?;
        Ok(())
    }
    pub fn recover_in_flight(&self) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_queue SET status='pending' WHERE status='in_flight'",
            [],
        )?;
        Ok(())
    }
    pub fn block(&self, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_queue SET status='blocked' WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }
    pub fn count_pending(&self) -> Result<usize, StateError> {
        let c = self.lock()?;
        Ok(c.query_row(
            "SELECT COUNT(*) FROM upload_queue WHERE status IN ('pending','in_flight')",
            [],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn count_status(&self, status: &str) -> Result<usize, StateError> {
        let c = self.lock()?;
        Ok(c.query_row(
            "SELECT COUNT(*) FROM upload_queue WHERE status=?1",
            params![status],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn recover_sequence(
        &self,
        device_id: &str,
        data_type: &str,
        minimum: u64,
    ) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=MAX(value, excluded.value)",
            params![device_id, data_type, minimum as i64],
        )?;
        Ok(())
    }

    pub fn enqueue_target(
        &self,
        target_id: &str,
        envelope: &EnvironmentEnvelope,
    ) -> Result<bool, StateError> {
        let c = self.lock()?;
        let inserted = c.execute(
            "INSERT OR IGNORE INTO upload_deliveries(target_id,envelope_json,status) VALUES(?1,?2,'pending')",
            params![target_id, serde_json::to_string(envelope)?],
        )?;
        Ok(inserted == 1)
    }

    pub fn count_target_status(&self, target_id: &str, status: &str) -> Result<usize, StateError> {
        let c = self.lock()?;
        Ok(c.query_row(
            "SELECT COUNT(*) FROM upload_deliveries WHERE target_id=?1 AND status=?2",
            params![target_id, status],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn pending_target(
        &self,
        target_id: &str,
    ) -> Result<Vec<(i64, EnvironmentEnvelope)>, StateError> {
        let c = self.lock()?;
        let mut stmt = c.prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status='pending' ORDER BY id")?;
        let rows = stmt.query_map(params![target_id], |r| {
            let id = r.get(0)?;
            let raw = r.get::<_, String>(1)?;
            let envelope = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((id, envelope))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::Database)
    }

    pub fn claim_target(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute("UPDATE upload_deliveries SET status='in_flight' WHERE target_id=?1 AND id=?2 AND status='pending'", params![target_id, id])?;
        Ok(())
    }

    pub fn acknowledge_target(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_deliveries SET status='completed' WHERE target_id=?1 AND id=?2 AND status='in_flight'",
            params![target_id, id],
        )?;
        Ok(())
    }

    pub fn delivery_status(
        &self,
        target_id: &str,
        device_id: &str,
        data_type: &str,
        sequence: u64,
    ) -> Result<Option<String>, StateError> {
        Ok(self
            .target_envelopes(target_id)?
            .into_iter()
            .find(|(_, envelope)| {
                envelope.device_id == device_id
                    && envelope.data_type == data_type
                    && envelope.sequence == sequence
            })
            .map(|(status, _)| status))
    }

    pub fn target_envelopes(
        &self,
        target_id: &str,
    ) -> Result<Vec<(String, EnvironmentEnvelope)>, StateError> {
        let c = self.lock()?;
        let mut stmt = c.prepare(
            "SELECT status,envelope_json FROM upload_deliveries WHERE target_id=?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![target_id], |r| {
            let status = r.get(0)?;
            let raw = r.get::<_, String>(1)?;
            let envelope = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((status, envelope))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::Database)
    }

    pub fn event_complete(
        &self,
        device_id: &str,
        data_type: &str,
        sequence: u64,
    ) -> Result<bool, StateError> {
        let matching: Vec<_> = self
            .all_delivery_envelopes()?
            .into_iter()
            .filter(|(_, envelope)| {
                envelope.device_id == device_id
                    && envelope.data_type == data_type
                    && envelope.sequence == sequence
            })
            .collect();
        Ok(!matching.is_empty()
            && matching
                .iter()
                .all(|(status, _)| status == "completed" || status == "cancelled"))
    }

    fn all_delivery_envelopes(&self) -> Result<Vec<(String, EnvironmentEnvelope)>, StateError> {
        let c = self.lock()?;
        let mut stmt =
            c.prepare("SELECT status,envelope_json FROM upload_deliveries ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            let status = r.get(0)?;
            let raw = r.get::<_, String>(1)?;
            let envelope = serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok((status, envelope))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::Database)
    }

    pub fn recover_target(&self, target_id: &str) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute("UPDATE upload_deliveries SET status='pending' WHERE target_id=?1 AND status='in_flight'", params![target_id])?;
        Ok(())
    }

    pub fn block_target(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_deliveries SET status='blocked' WHERE target_id=?1 AND id=?2",
            params![target_id, id],
        )?;
        Ok(())
    }

    pub fn cancel_target_delivery(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2 AND status IN ('pending','in_flight','blocked')",
            params![target_id, id],
        )?;
        Ok(())
    }

    pub fn unblock_target(&self, target_id: &str) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute("UPDATE upload_deliveries SET status='pending' WHERE target_id=?1 AND status='blocked'", params![target_id])?;
        Ok(())
    }

    pub fn cancel_target_except_device(&self, target_id: &str, device_id: &str) -> Result<(), StateError> {
        let c = self.lock()?;
        let rows = c
            .prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight')")?
            .query_map(params![target_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, raw) in rows {
            let envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id != device_id {
                c.execute("UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2", params![target_id, id])?;
            }
        }
        Ok(())
    }

    pub fn cancel_target_device(&self, target_id: &str, device_id: &str) -> Result<(), StateError> {
        let c = self.lock()?;
        let rows = c
            .prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight')")?
            .query_map(params![target_id], |r| {
                let id = r.get::<_, i64>(0)?;
                let raw = r.get::<_, String>(1)?;
                Ok((id, raw))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, raw) in rows {
            let envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id == device_id {
                c.execute("UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2", params![target_id, id])?;
            }
        }
        Ok(())
    }

    pub fn cancel_target(&self, target_id: &str) -> Result<(), StateError> {
        let c = self.lock()?;
        c.execute(
            "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND status IN ('pending','in_flight')",
            params![target_id],
        )?;
        Ok(())
    }
}
use rusqlite::OptionalExtension;

#[allow(dead_code)]
fn _logging_level(_: LoggingLevel) {}
