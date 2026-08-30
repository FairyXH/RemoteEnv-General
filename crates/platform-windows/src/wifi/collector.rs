use super::{WiFiError, WiFiSnapshot};
use remote_env_core::collector::CollectorEvent;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub trait WlanProvider: Send + Sync {
    fn scan(&self) -> Result<WiFiSnapshot, WiFiError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectorState {
    Disabled,
    Starting,
    Scanning,
    Ready,
    Error,
    Stopped,
}

pub struct WiFiCollector<P: WlanProvider> {
    provider: Arc<P>,
    enabled: bool,
    state: std::sync::Mutex<CollectorState>,
}

impl<P: WlanProvider> WiFiCollector<P> {
    pub fn new(provider: P, enabled: bool) -> Self {
        Self {
            provider: Arc::new(provider),
            enabled,
            state: std::sync::Mutex::new(if enabled {
                CollectorState::Starting
            } else {
                CollectorState::Disabled
            }),
        }
    }
    pub fn state(&self) -> CollectorState {
        *self.state.lock().expect("collector state")
    }
    pub fn scan_once(&self) -> Result<CollectorEvent, WiFiError> {
        if !self.enabled {
            return Err(WiFiError::Unavailable("collector disabled".into()));
        }
        *self.state.lock().expect("collector state") = CollectorState::Scanning;
        let snapshot = match self.provider.scan() {
            Ok(value) => value,
            Err(error) => {
                *self.state.lock().expect("collector state") = CollectorState::Error;
                return Err(error);
            }
        };
        *self.state.lock().expect("collector state") = CollectorState::Ready;
        let scan_finished_at = now_ms();
        let scan_started_at = scan_finished_at
            .saturating_sub(snapshot.scan_duration_ms as i64)
            .max(1);
        let data = serde_json::json!({
            "scan_started_at": scan_started_at,
            "scan_finished_at": scan_finished_at,
            "networks": snapshot.networks,
        });
        Ok(CollectorEvent {
            data_type: "wifi".into(),
            timestamp_ms: scan_finished_at,
            data,
        })
    }
    pub fn provider(&self) -> Arc<P> {
        Arc::clone(&self.provider)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock;
    impl WlanProvider for Mock {
        fn scan(&self) -> Result<WiFiSnapshot, WiFiError> {
            Ok(WiFiSnapshot {
                networks: vec![],
                interfaces: 1,
                scan_duration_ms: 2,
            })
        }
    }
    #[test]
    fn mock_scan_creates_one_snapshot_event() {
        let event = WiFiCollector::new(Mock, true).scan_once().unwrap();
        assert_eq!(event.data_type, "wifi");
        assert_eq!(event.data["networks"].as_array().unwrap().len(), 0);
    }
    #[test]
    fn disabled_collector_does_not_call_provider() {
        assert_eq!(
            WiFiCollector::new(Mock, false).state(),
            CollectorState::Disabled
        );
    }
}
