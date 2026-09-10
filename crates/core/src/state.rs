use crate::config::{ClientConfig, DeviceIdentity, LoggingLevel};
use crate::protocol::EnvironmentEnvelope;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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

/// Schema for the small, always-clean user configuration database.
/// Holds only `metadata` (config + identity). Never grows with upload cache.
const CONFIG_SCHEMA: &str = "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);";

/// Schema for the larger, rebuildable state cache database.
/// Holds sequence bookkeeping and upload delivery rows. This file may be
/// deleted and rebuilt at any time without losing user configuration.
const STATE_SCHEMA: &str = "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS sequences (device_id TEXT NOT NULL, data_type TEXT NOT NULL, value INTEGER NOT NULL, PRIMARY KEY(device_id, data_type)); CREATE TABLE IF NOT EXISTS upload_queue (id INTEGER PRIMARY KEY AUTOINCREMENT, envelope_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', created_at_ms INTEGER NOT NULL DEFAULT 0, UNIQUE(envelope_json)); CREATE INDEX IF NOT EXISTS idx_upload_queue_pending ON upload_queue(status, id); CREATE TABLE IF NOT EXISTS upload_deliveries (id INTEGER PRIMARY KEY AUTOINCREMENT, target_id TEXT NOT NULL, envelope_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', created_at_ms INTEGER NOT NULL DEFAULT 0, UNIQUE(target_id, envelope_json)); CREATE INDEX IF NOT EXISTS idx_upload_deliveries_pending ON upload_deliveries(target_id, status, id);";

const MAX_STATE_CACHE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct StateStore {
    config: Arc<Mutex<Connection>>,
    state: Arc<Mutex<Connection>>,
    state_path: Arc<PathBuf>,
}

impl StateStore {
    /// Opens the split databases. `config_path` is the small, clean user
    /// configuration database; `state_path` is the larger, rebuildable upload
    /// cache. If the state cache cannot be opened or repaired it is deleted and
    /// recreated so the client can keep running.
    pub fn open(
        config_path: impl AsRef<Path>,
        state_path: impl AsRef<Path>,
    ) -> Result<Self, StateError> {
        let config = Self::open_config(config_path.as_ref())?;
        let state_path = state_path.as_ref().to_path_buf();
        let state = Self::open_state(&state_path)?;
        Self::enforce_state_cache_limit(&state, &state_path)?;
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            state: Arc::new(Mutex::new(state)),
            state_path: Arc::new(state_path),
        })
    }

    fn open_config(path: &Path) -> Result<Connection, StateError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(CONFIG_SCHEMA)?;
        Ok(connection)
    }

    fn open_state(path: &Path) -> Result<Connection, StateError> {
        match Self::try_open_state(path) {
            Ok(connection) => Ok(connection),
            Err(error)
                if !matches!(&error, StateError::Database(rusqlite::Error::SqliteFailure(code, _))
                if matches!(code.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase)) =>
            {
                Err(error)
            }
            Err(error) => {
                eprintln!("state cache open failed, rebuilding from empty: {error}");
                Self::remove_sidecar(path, "-wal");
                Self::remove_sidecar(path, "-shm");
                let _ = std::fs::remove_file(path);
                let connection = Connection::open(path)?;
                connection.execute_batch(STATE_SCHEMA)?;
                Ok(connection)
            }
        }
    }

    fn try_open_state(path: &Path) -> Result<Connection, StateError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(STATE_SCHEMA)?;
        // Force a trivial read so a corrupt page is surfaced here, not later.
        connection.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))?;
        if connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? < 1 {
            let transaction = connection.unchecked_transaction()?;
            Self::normalize_legacy_sequences(&transaction)?;
            transaction.pragma_update(None, "user_version", 1)?;
            transaction.commit()?;
        }
        Ok(connection)
    }

    fn remove_sidecar(base: &Path, suffix: &str) {
        if let Some(name) = base.file_name().and_then(|name| name.to_str()) {
            let sidecar = base.with_file_name(format!("{name}{suffix}"));
            let _ = std::fs::remove_file(sidecar);
        }
    }

    fn state_cache_size(path: &Path) -> u64 {
        ["", "-wal", "-shm"]
            .into_iter()
            .filter_map(|suffix| {
                let file = if suffix.is_empty() {
                    path.to_path_buf()
                } else {
                    let name = path.file_name()?.to_str()?;
                    path.with_file_name(format!("{name}{suffix}"))
                };
                std::fs::metadata(file).ok().map(|metadata| metadata.len())
            })
            .sum()
    }

    fn enforce_state_cache_limit(
        connection: &Connection,
        state_path: &Path,
    ) -> Result<bool, StateError> {
        if Self::state_cache_size(state_path) <= MAX_STATE_CACHE_BYTES {
            return Ok(false);
        }

        connection.execute_batch(
            "DELETE FROM upload_deliveries; DELETE FROM upload_queue; PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
        Ok(true)
    }

    fn lock_config(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StateError> {
        self.config.lock().map_err(|_| StateError::Poisoned)
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StateError> {
        self.state.lock().map_err(|_| StateError::Poisoned)
    }

    pub fn next_sequence(&self, device_id: &str, data_type: &str) -> Result<u64, StateError> {
        self.next_timestamp_sequence(device_id, data_type)
    }

    pub fn get_sequence(
        &self,
        device_id: &str,
        data_type: &str,
    ) -> Result<Option<u64>, StateError> {
        let connection = self.lock_state()?;
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
        let connection = self.lock_state()?;
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

    pub fn next_timestamp_sequence(
        &self,
        device_id: &str,
        data_type: &str,
    ) -> Result<u64, StateError> {
        let timestamp = Self::now_ms();
        let connection = self.lock_state()?;
        let current: Option<i64> = connection
            .query_row(
                "SELECT value FROM sequences WHERE device_id=?1 AND data_type=?2",
                params![device_id, data_type],
                |row| row.get(0),
            )
            .optional()?;
        let latest = current.unwrap_or(0).saturating_add(1).max(timestamp as i64);
        connection.execute("INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=excluded.value", params![device_id, data_type, latest])?;
        Ok(latest as u64)
    }

    pub fn rebase_target_sequences(
        &self,
        target_id: &str,
        device_id: &str,
        data_type: &str,
        minimum: u64,
    ) -> Result<(), StateError> {
        let c = self.lock_state()?;
        let mut stmt = c.prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight','blocked')")?;
        let rows = stmt
            .query_map(params![target_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut next = minimum.max(Self::now_ms());
        for (id, raw) in rows {
            let mut envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id == device_id
                && envelope.data_type == data_type
                && envelope.sequence < next
            {
                envelope.sequence = next;
                next = next.saturating_add(1);
                c.execute("UPDATE upload_deliveries SET status='pending', envelope_json=?1 WHERE target_id=?2 AND id=?3", params![serde_json::to_string(&envelope)?, target_id, id])?;
            }
        }
        c.execute("INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=MAX(value, excluded.value)", params![device_id, data_type, next as i64])?;
        Ok(())
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn normalize_legacy_sequences(connection: &Connection) -> Result<(), StateError> {
        let floor = Self::now_ms();
        for table in ["upload_queue", "upload_deliveries"] {
            let query = format!(
                "SELECT id,envelope_json FROM {table} WHERE status IN ('pending','in_flight','blocked')"
            );
            let mut stmt = connection.prepare(&query)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for (id, raw) in rows {
                let mut envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
                let original_payload = envelope.data.clone();
                envelope.normalize_for_transport();
                let payload_changed = envelope.data != original_payload;
                // Timestamp sequences already in use must retain their ACK identity.
                let sequence_changed = envelope.sequence < 1_000_000_000_000;
                if sequence_changed {
                    envelope.sequence = floor.saturating_add(id as u64);
                }
                if payload_changed || sequence_changed {
                    let update = format!(
                        "UPDATE {table} SET envelope_json=?1, status='pending' WHERE id=?2"
                    );
                    connection.execute(&update, params![serde_json::to_string(&envelope)?, id])?;
                }
                connection.execute(
                    "INSERT INTO sequences(device_id,data_type,value) VALUES(?1,?2,?3) ON CONFLICT(device_id,data_type) DO UPDATE SET value=MAX(value, excluded.value)",
                    params![envelope.device_id, envelope.data_type, envelope.sequence as i64],
                )?;
            }
        }
        Ok(())
    }

    pub fn load_or_create_identity(
        &self,
        name: &str,
        platform: &str,
        version: &str,
    ) -> Result<DeviceIdentity, StateError> {
        let connection = self.lock_config()?;
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
        let connection = self.lock_config()?;
        connection.execute("INSERT INTO metadata(key,value) VALUES('config',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![serde_json::to_string(config)?])?;
        Ok(())
    }

    pub fn load_config(&self) -> Result<Option<ClientConfig>, StateError> {
        let connection = self.lock_config()?;
        Ok(connection
            .query_row("SELECT value FROM metadata WHERE key='config'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|v| serde_json::from_str(&v))
            .transpose()?)
    }

    pub fn enqueue(&self, envelope: &EnvironmentEnvelope) -> Result<bool, StateError> {
        let connection = self.lock_state()?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO upload_queue(envelope_json,status,created_at_ms) VALUES(?1,'pending',?2)",
            params![serde_json::to_string(envelope)?, Self::now_ms() as i64],
        )?;
        Self::enforce_state_cache_limit(&connection, &self.state_path)?;
        Ok(inserted == 1)
    }

    pub fn pending(&self) -> Result<Vec<(i64, EnvironmentEnvelope)>, StateError> {
        let connection = self.lock_state()?;
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
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_queue SET status='in_flight' WHERE id=?1 AND status='pending'",
            params![id],
        )?;
        Ok(())
    }

    pub fn acknowledge(&self, id: i64) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute("DELETE FROM upload_queue WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn recover_in_flight(&self) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_queue SET status='pending' WHERE status='in_flight'",
            [],
        )?;
        Ok(())
    }

    pub fn block(&self, id: i64) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_queue SET status='blocked' WHERE id=?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn count_pending(&self) -> Result<usize, StateError> {
        let c = self.lock_state()?;
        Ok(c.query_row(
            "SELECT COUNT(*) FROM upload_queue WHERE status IN ('pending','in_flight')",
            [],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn count_status(&self, status: &str) -> Result<usize, StateError> {
        let c = self.lock_state()?;
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
        let minimum = minimum.max(Self::now_ms());
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
        let inserted = c.execute(
            "INSERT OR IGNORE INTO upload_deliveries(target_id,envelope_json,status,created_at_ms) VALUES(?1,?2,'pending',?3)",
            params![target_id, serde_json::to_string(envelope)?, Self::now_ms() as i64],
        )?;
        Self::enforce_state_cache_limit(&c, &self.state_path)?;
        Ok(inserted == 1)
    }

    pub fn count_target_status(&self, target_id: &str, status: &str) -> Result<usize, StateError> {
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
        c.execute("UPDATE upload_deliveries SET status='in_flight' WHERE target_id=?1 AND id=?2 AND status='pending'", params![target_id, id])?;
        Ok(())
    }

    pub fn acknowledge_target(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
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
        let c = self.lock_state()?;
        c.execute("UPDATE upload_deliveries SET status='pending' WHERE target_id=?1 AND status='in_flight'", params![target_id])?;
        Ok(())
    }

    pub fn block_target(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_deliveries SET status='blocked' WHERE target_id=?1 AND id=?2",
            params![target_id, id],
        )?;
        Ok(())
    }

    pub fn cancel_target_delivery(&self, target_id: &str, id: i64) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2 AND status IN ('pending','in_flight','blocked')",
            params![target_id, id],
        )?;
        Ok(())
    }

    pub fn unblock_target(&self, target_id: &str) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_deliveries SET status='pending' WHERE target_id=?1 AND status='blocked'",
            params![target_id],
        )?;
        Ok(())
    }

    pub fn cancel_target_except_device(
        &self,
        target_id: &str,
        device_id: &str,
    ) -> Result<(), StateError> {
        let c = self.lock_state()?;
        let rows = c
            .prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight')")?
            .query_map(params![target_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, raw) in rows {
            let envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id != device_id {
                c.execute(
                    "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2",
                    params![target_id, id],
                )?;
            }
        }
        Ok(())
    }

    pub fn cancel_target_device(&self, target_id: &str, device_id: &str) -> Result<(), StateError> {
        let c = self.lock_state()?;
        let rows = c
            .prepare("SELECT id,envelope_json FROM upload_deliveries WHERE target_id=?1 AND status IN ('pending','in_flight')")?
            .query_map(params![target_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, raw) in rows {
            let envelope: EnvironmentEnvelope = serde_json::from_str(&raw)?;
            if envelope.device_id == device_id {
                c.execute(
                    "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND id=?2",
                    params![target_id, id],
                )?;
            }
        }
        Ok(())
    }

    pub fn cancel_target(&self, target_id: &str) -> Result<(), StateError> {
        let c = self.lock_state()?;
        c.execute(
            "UPDATE upload_deliveries SET status='cancelled' WHERE target_id=?1 AND status IN ('pending','in_flight')",
            params![target_id],
        )?;
        Ok(())
    }

    /// Prunes acknowledged/cancelled delivery rows older than `older_than`
    /// from the rebuildable state cache and compacts the database. This keeps
    /// the cache file small even under long-running, high-frequency upload.
    pub fn cleanup_state_cache(&self, older_than: Duration) -> Result<u64, StateError> {
        let cutoff = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64)
            .saturating_sub(older_than.as_millis() as i64);
        let c = self.lock_state()?;
        let removed = c.execute(
            "DELETE FROM upload_deliveries WHERE status IN ('completed','cancelled') AND created_at_ms < ?1",
            params![cutoff],
        )? as u64;
        let _ = c.execute(
            "DELETE FROM upload_queue WHERE status IN ('completed','cancelled') AND created_at_ms < ?1",
            params![cutoff],
        );
        let _ = c.execute("VACUUM", []);
        Ok(removed)
    }
}

#[allow(dead_code)]
fn _logging_level(_: LoggingLevel) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_state_cache_is_cleared_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("state.sqlite3");
        let cache_path = dir.path().join("state_cache.sqlite3");
        let connection = Connection::open(&cache_path).unwrap();
        connection.execute_batch(STATE_SCHEMA).unwrap();
        connection
            .execute(
                "INSERT INTO sequences(device_id,data_type,value) VALUES('device','wifi',42)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO upload_deliveries(target_id,envelope_json,status,created_at_ms) VALUES('target',zeroblob(?1),'completed',0)",
                params![(MAX_STATE_CACHE_BYTES + 1) as i64],
            )
            .unwrap();
        drop(connection);
        assert!(StateStore::state_cache_size(&cache_path) > MAX_STATE_CACHE_BYTES);

        let store = StateStore::open(&config_path, &cache_path).unwrap();

        assert_eq!(store.count_target_status("target", "completed").unwrap(), 0);
        assert_eq!(store.latest_sequence("device", "wifi").unwrap(), 42);
        assert!(StateStore::state_cache_size(&cache_path) <= MAX_STATE_CACHE_BYTES);
    }
}
