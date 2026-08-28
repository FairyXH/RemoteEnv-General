use super::error::WiFiError;
use super::model::{
    Security, WiFiObservation, WiFiSnapshot, band_for_frequency, channel_for_frequency,
    decode_ssid, format_bssid,
};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::time::Instant;
use windows_sys::core::GUID;

const WLAN_MAX_PHY_TYPE_NUMBER: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct InterfaceInfo {
    guid: GUID,
    description: [u16; 256],
    state: u32,
}

#[repr(C)]
struct InterfaceInfoList {
    dw_number_of_items: u32,
    dw_index: u32,
    items: [InterfaceInfo; 1],
}

#[repr(C)]
struct Ssid {
    length: u32,
    ssid: [u8; 32],
}

#[repr(C)]
struct BssEntry {
    ssid: Ssid,
    phy_type: u32,
    phy_index: u32,
    bssid: [u8; 6],
    reserved: u8,
    rssi: i32,
    link_quality: u32,
    in_reg_domain: i32,
    beacon_period: u16,
    timestamp: u64,
    host_timestamp: u64,
    capability: u16,
    channel_center_frequency: u32,
    ie_offset: u32,
    ie_size: u32,
}

#[repr(C)]
struct BssList {
    total_size: u32,
    number_of_items: u32,
    items: [BssEntry; 1],
}

#[link(name = "wlanapi")]
unsafe extern "system" {
    fn WlanOpenHandle(
        version: u32,
        reserved: *mut c_void,
        negotiated: *mut u32,
        handle: *mut *mut c_void,
    ) -> u32;
    fn WlanCloseHandle(handle: *mut c_void, reserved: *mut c_void) -> u32;
    fn WlanEnumInterfaces(
        handle: *mut c_void,
        reserved: *mut c_void,
        list: *mut *mut InterfaceInfoList,
    ) -> u32;
    fn WlanScan(
        handle: *mut c_void,
        interface: *const GUID,
        ssid: *const c_void,
        ie: *const c_void,
        reserved: *mut c_void,
    ) -> u32;
    fn WlanGetNetworkBssList(
        handle: *mut c_void,
        interface: *const GUID,
        ssid: *const c_void,
        type_: u32,
        security: i32,
        reserved: *mut c_void,
        list: *mut *mut BssList,
    ) -> u32;
    fn WlanFreeMemory(memory: *mut c_void);
}

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
            let mut handle = null_mut();
            let mut negotiated = 0;
            check(WlanOpenHandle(2, null_mut(), &mut negotiated, &mut handle))?;
            let result = enumerate(handle, started);
            let _ = WlanCloseHandle(handle, null_mut());
            result
        }
    }
}

unsafe fn enumerate(handle: *mut c_void, started: Instant) -> Result<WiFiSnapshot, WiFiError> {
    let mut interfaces = null_mut();
    check(WlanEnumInterfaces(handle, null_mut(), &mut interfaces))?;
    if interfaces.is_null() {
        return Err(WiFiError::InvalidData("null interface list".into()));
    }
    let list = &*interfaces;
    let count = list.dw_number_of_items as usize;
    let base = list.items.as_ptr();
    let mut networks = Vec::new();
    for index in 0..count {
        let interface = &*base.add(index);
        // A scan request is asynchronous; the BSS list below represents the driver's current results.
        let _ = WlanScan(handle, &interface.guid, null_mut(), null_mut(), null_mut());
        let mut bss = null_mut();
        let status = WlanGetNetworkBssList(
            handle,
            &interface.guid,
            null_mut(),
            0,
            0,
            null_mut(),
            &mut bss,
        );
        if status != 0 || bss.is_null() {
            continue;
        }
        let entries = &*bss;
        let entry_base = entries.items.as_ptr();
        let interface_id = format_guid(&interface.guid);
        for item in 0..entries.number_of_items as usize {
            let entry = &*entry_base.add(item);
            let length = (entry.ssid.length as usize).min(entry.ssid.ssid.len());
            let (ssid, raw, hidden) = decode_ssid(&entry.ssid.ssid[..length]);
            let frequency = (entry.channel_center_frequency > 0)
                .then_some(entry.channel_center_frequency / 1000);
            networks.push(WiFiObservation {
                ssid,
                ssid_bytes_hex: raw,
                hidden,
                bssid: format_bssid(&entry.bssid),
                signal_strength_dbm: Some(entry.rssi),
                signal_percent: Some(entry.link_quality.min(100) as u8),
                channel: frequency.and_then(channel_for_frequency),
                frequency_mhz: frequency,
                band: band_for_frequency(frequency),
                phy_type: Some(format!("{}", entry.phy_type)),
                network_type: None,
                security: Security::default(),
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

#[allow(dead_code)]
const _: usize = WLAN_MAX_PHY_TYPE_NUMBER + size_of::<Ssid>();
