use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    #[serde(rename = "2_4ghz")]
    Band24Ghz,
    #[serde(rename = "5ghz")]
    Band5Ghz,
    #[serde(rename = "6ghz")]
    Band6Ghz,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WiFiObservation {
    pub ssid: Option<String>,
    #[serde(skip_serializing)]
    pub ssid_bytes_hex: Option<String>,
    pub hidden: bool,
    pub bssid: String,
    #[serde(rename = "rssi")]
    pub rssi: Option<f64>,
    #[serde(skip_serializing)]
    pub signal_percent: Option<u8>,
    pub channel: Option<u16>,
    pub frequency_mhz: Option<f64>,
    pub band: Band,
    #[serde(skip_serializing)]
    pub phy_type: Option<String>,
    #[serde(skip_serializing)]
    pub network_type: Option<String>,
    /// WLAN_BSS_ENTRY does not expose negotiated security suites. Do not
    /// infer WPA/OPEN from the privacy bit; emit an empty standard list.
    pub security: Vec<String>,
    #[serde(skip_serializing)]
    pub interface_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WiFiSnapshot {
    pub networks: Vec<WiFiObservation>,
    #[serde(skip_serializing)]
    pub interfaces: usize,
    #[serde(skip_serializing)]
    pub scan_duration_ms: u64,
}

pub(crate) fn decode_ssid(bytes: &[u8]) -> (Option<String>, Option<String>, bool) {
    let raw = bytes.iter().map(|b| format!("{b:02X}")).collect::<String>();
    if bytes.is_empty() {
        return (None, None, true);
    }
    match std::str::from_utf8(bytes) {
        Ok(value) if !value.is_empty() => (Some(value.to_owned()), None, false),
        _ => (None, Some(raw), false),
    }
}

pub(crate) fn format_bssid(bytes: &[u8; 6]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn band_for_frequency(frequency_mhz: Option<f64>) -> Band {
    match frequency_mhz {
        Some(value) if (2400.0..=2500.0).contains(&value) => Band::Band24Ghz,
        Some(value) if (4900.0..=5900.0).contains(&value) => Band::Band5Ghz,
        Some(value) if (5925.0..=7125.0).contains(&value) => Band::Band6Ghz,
        _ => Band::Unknown,
    }
}

pub(crate) fn channel_for_frequency(frequency_mhz: u32) -> Option<u16> {
    match frequency_mhz {
        2412..=2484 => Some(((frequency_mhz - 2407) / 5) as u16),
        5000..=5895 => Some(((frequency_mhz - 5000) / 5) as u16),
        5955..=7115 => Some(((frequency_mhz - 5950) / 5) as u16),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_utf8_and_preserves_invalid_bytes() {
        assert_eq!(decode_ssid(b"Cafe"), (Some("Cafe".into()), None, false));
        assert_eq!(
            decode_ssid(&[0xff, 0x00]),
            (None, Some("FF00".into()), false)
        );
        assert_eq!(decode_ssid(&[]), (None, None, true));
    }
    #[test]
    fn formats_mac_and_maps_frequency() {
        assert_eq!(
            format_bssid(&[0, 1, 0xaa, 0xbb, 0xcc, 0xff]),
            "00:01:AA:BB:CC:FF"
        );
        assert_eq!(channel_for_frequency(2412), Some(1));
        assert_eq!(channel_for_frequency(5180), Some(36));
        assert_eq!(band_for_frequency(Some(5975.0)), Band::Band6Ghz);
    }
}
