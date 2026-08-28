use remote_env_core::bluetooth::{BluetoothObservation, BluetoothTransport};

pub fn format_bluetooth_address(address: u64) -> String {
    address.to_be_bytes()[2..]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn merge_observations(
    mut observations: Vec<BluetoothObservation>,
    allow_cross_transport_merge: bool,
) -> Vec<BluetoothObservation> {
    observations.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then(a.transport.cmp(&b.transport))
            .then(a.timestamp_ms.cmp(&b.timestamp_ms))
    });
    let mut merged: Vec<BluetoothObservation> = Vec::new();
    for item in observations {
        let same_key = |existing: &BluetoothObservation| {
            existing.address == item.address
                && (allow_cross_transport_merge || existing.transport == item.transport)
        };
        if let Some(existing) = merged.iter_mut().find(|value| same_key(value)) {
            if item.timestamp_ms >= existing.timestamp_ms {
                if item.name.is_some() {
                    existing.name = item.name;
                }
                if item.rssi.is_some() {
                    existing.rssi = item.rssi;
                }
                if item.connectable.is_some() {
                    existing.connectable = item.connectable;
                }
                if item.appearance.is_some() {
                    existing.appearance = item.appearance;
                }
                if item.tx_power.is_some() {
                    existing.tx_power = item.tx_power;
                }
                existing.timestamp_ms = item.timestamp_ms;
            }
            if existing.transport != item.transport {
                existing.transport = BluetoothTransport::Dual;
            }
            existing.service_uuids.extend(item.service_uuids);
            existing.service_uuids.sort();
            existing.service_uuids.dedup();
            for next in item.manufacturer_data {
                existing
                    .manufacturer_data
                    .retain(|value| value.company_id != next.company_id);
                existing.manufacturer_data.push(next);
            }
            for next in item.service_data {
                existing
                    .service_data
                    .retain(|value| value.uuid != next.uuid);
                existing.service_data.push(next);
            }
        } else {
            merged.push(item);
        }
    }
    merged.sort_by(|a, b| {
        a.transport
            .cmp(&b.transport)
            .then(a.address.cmp(&b.address))
    });
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_windows_u64_in_canonical_order() {
        assert_eq!(
            format_bluetooth_address(0xAABBCCDDEEFF),
            "AA:BB:CC:DD:EE:FF"
        );
    }
}
