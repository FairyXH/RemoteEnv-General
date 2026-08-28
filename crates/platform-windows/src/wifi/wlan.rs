use super::error::WiFiError;
use super::model::{
    Security, WiFiObservation, WiFiSnapshot, band_for_frequency, channel_for_frequency,
    decode_ssid, format_bssid,
};
use std::ptr::null_mut;
use std::time::Instant;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_BSS_LIST, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetNetworkBssList,
    WlanOpenHandle, WlanScan,
};
use windows_sys::core::GUID;

pub struct NativeWlanProvider;

impl NativeWlanProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NativeWlanProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl super::collector::WlanProvider for NativeWlanProvider {
    fn scan(&self) -> Result<WiFiSnapshot, WiFiError> {
        let started = Instant::now();
        unsafe {
            let mut handle: HANDLE = null_mut();
            let mut negotiated = 0;
            check(WlanOpenHandle(2, null_mut(), &mut negotiated, &mut handle))?;
            let result = enumerate(handle, started);
            let _ = WlanCloseHandle(handle, null_mut());
            result
        }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn enumerate(handle: HANDLE, started: Instant) -> Result<WiFiSnapshot, WiFiError> {
    let mut interfaces = null_mut();
    check(WlanEnumInterfaces(handle, null_mut(), &mut interfaces))?;
    if interfaces.is_null() {
        return Err(WiFiError::InvalidData("null interface list".into()));
    }
    let list = &*interfaces;
    let count = list.dwNumberOfItems as usize;
    let base = list.InterfaceInfo.as_ptr();
    let mut networks = Vec::new();
    for index in 0..count {
        let interface = &*base.add(index);
        // A scan request is asynchronous; the BSS list below represents the driver's current results.
        let _ = WlanScan(
            handle,
            &interface.InterfaceGuid,
            null_mut(),
            null_mut(),
            null_mut(),
        );
        let mut bss: *mut WLAN_BSS_LIST = null_mut();
        let status = WlanGetNetworkBssList(
            handle,
            &interface.InterfaceGuid,
            null_mut(),
            3,
            0,
            null_mut(),
            &mut bss,
        );
        if status != 0 || bss.is_null() {
            continue;
        }
        let entries = &*bss;
        let entry_base = entries.wlanBssEntries.as_ptr();
        let interface_id = format_guid(&interface.InterfaceGuid);
        for item in 0..entries.dwNumberOfItems as usize {
            let entry = &*entry_base.add(item);
            let length = (entry.dot11Ssid.uSSIDLength as usize).min(entry.dot11Ssid.ucSSID.len());
            let (ssid, raw, hidden) = decode_ssid(&entry.dot11Ssid.ucSSID[..length]);
            let frequency =
                (entry.ulChCenterFrequency > 0).then_some(entry.ulChCenterFrequency / 1000);
            networks.push(WiFiObservation {
                ssid,
                ssid_bytes_hex: raw,
                hidden,
                bssid: format_bssid(&entry.dot11Bssid),
                signal_strength_dbm: Some(entry.lRssi),
                signal_percent: Some(entry.uLinkQuality.min(100) as u8),
                channel: frequency.and_then(channel_for_frequency),
                frequency_mhz: frequency,
                band: band_for_frequency(frequency),
                phy_type: Some(format!("{}", entry.dot11BssPhyType)),
                network_type: None,
                security: Security {
                    privacy: Some((entry.usCapabilityInformation & 0x0010) != 0),
                    ..Security::default()
                },
                interface_id: interface_id.clone(),
            });
        }
        WlanFreeMemory(bss.cast());
    }
    WlanFreeMemory(interfaces.cast());
    networks.sort_by(|a, b| {
        a.bssid
            .cmp(&b.bssid)
            .then(a.interface_id.cmp(&b.interface_id))
    });
    networks.dedup_by(|a, b| a.bssid == b.bssid && a.interface_id == b.interface_id);
    Ok(WiFiSnapshot {
        networks,
        interfaces: count,
        scan_duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn check(status: u32) -> Result<(), WiFiError> {
    (status == 0).then_some(()).ok_or(WiFiError::Api(status))
}

fn format_guid(guid: &GUID) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X?}",
        guid.data1, guid.data2, guid.data3, guid.data4
    )
}
