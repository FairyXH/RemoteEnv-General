pub mod wifi;
pub mod bluetooth;

use remote_env_core::collector::{CapabilityState, CollectorKind};

pub fn collector_capability(kind: CollectorKind) -> CapabilityState {
    match kind {
        CollectorKind::Wifi => CapabilityState::Available,
        CollectorKind::Ble | CollectorKind::ClassicBluetooth => CapabilityState::NotImplemented,
    }
}
