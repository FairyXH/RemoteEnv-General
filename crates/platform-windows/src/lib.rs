use remote_env_core::collector::{CapabilityState, CollectorKind};

pub fn collector_capability(kind: CollectorKind) -> CapabilityState {
    match kind {
        CollectorKind::Wifi | CollectorKind::Ble | CollectorKind::ClassicBluetooth => {
            CapabilityState::NotImplemented
        }
    }
}
