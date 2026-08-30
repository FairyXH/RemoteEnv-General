#![allow(unsafe_op_in_unsafe_fn)]
use super::error::WiFiError;
use super::model::{
    WiFiObservation, WiFiSnapshot, band_for_frequency, channel_for_frequency, decode_ssid,
    format_bssid,
};
use std::collections::HashMap;
use std::ptr::null_mut;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GetAdaptersAddresses, IF_TYPE_IEEE80211, IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::WiFi::{
    DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_80211_SHARED_KEY, DOT11_AUTH_ALGO_OWE,
    DOT11_AUTH_ALGO_RSNA, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA, DOT11_AUTH_ALGO_WPA_NONE,
    DOT11_AUTH_ALGO_WPA_PSK, DOT11_AUTH_ALGO_WPA3, DOT11_AUTH_ALGO_WPA3_ENT,
    DOT11_AUTH_ALGO_WPA3_SAE, WLAN_AVAILABLE_NETWORK_LIST, WLAN_BSS_LIST, WLAN_INTERFACE_INFO_LIST,
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
    WlanGetNetworkBssList, WlanOpenHandle, WlanScan, wlan_interface_state_connected,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
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
    let mut interface_description = None;
    let mut is_connected = false;
    for index in 0..count {
        let interface = &*base.add(index);
        if index == 0 {
            interface_description =
                Some(read_utf16(&interface.strInterfaceDescription).unwrap_or_default());
            is_connected = interface.isState == wlan_interface_state_connected;
        }
        let scan_status = WlanScan(
            handle,
            &interface.InterfaceGuid,
            null_mut(),
            null_mut(),
            null_mut(),
        );
        if scan_status == 0 {
            // WlanScan is asynchronous; give the WLAN service time to refresh its BSS cache.
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        // WLAN_BSS_ENTRY does not expose negotiated security suites, so query
        // the visible-network list and match each BSS by its SSID bytes.
        let security_by_ssid = query_security_map(handle, &interface.InterfaceGuid);
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
            let ssid_bytes = &entry.dot11Ssid.ucSSID[..length];
            let (ssid, raw, hidden) = decode_ssid(ssid_bytes);
            let security = security_by_ssid
                .get(ssid_bytes)
                .cloned()
                .unwrap_or_default();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let frequency = (entry.ulChCenterFrequency > 0)
                .then_some((entry.ulChCenterFrequency / 1000) as f64);
            // WLAN API reports RSSI in dBm; some drivers report a 0..100 quality
            // percentage in the same field. Only transmit a canonical dBm value.
            let rssi = normalize_rssi(entry.lRssi);
            networks.push(WiFiObservation {
                ssid,
                ssid_bytes_hex: raw,
                hidden,
                bssid: format_bssid(&entry.dot11Bssid),
                rssi,
                signal_dbm: rssi,
                signal_percent: Some(entry.uLinkQuality.min(100) as u8),
                channel: frequency.and_then(|value| channel_for_frequency(value as u32)),
                frequency_mhz: frequency,
                band: band_for_frequency(frequency),
                phy_type: Some(format!("{}", entry.dot11BssPhyType)),
                network_type: None,
                security,
                timestamp,
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
    let connection = if is_connected {
        // The server requires gateway/dns/ip whenever is_connected=true. If the
        // IP Helper lookup cannot find the Wi-Fi adapter details, report the
        // connection as disconnected rather than send an invalid payload.
        let details = query_adapter_details(first_guid(&list));
        match details {
            Some(details) if details.gateway.is_some() && details.ip_address.is_some() => {
                Some(details)
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(WiFiSnapshot {
        networks,
        interfaces: count,
        scan_duration_ms: started.elapsed().as_millis() as u64,
        interface: interface_description,
        is_connected: connection.is_some(),
        gateway: connection.as_ref().and_then(|value| value.gateway.clone()),
        dns_servers: connection
            .as_ref()
            .map(|value| value.dns_servers.clone())
            .unwrap_or_default(),
        ip_address: connection
            .as_ref()
            .and_then(|value| value.ip_address.clone()),
    })
}

/// Queries the visible-network list and returns a map from raw SSID bytes to
/// the server-canonical security suite names (e.g. `WPA2-PSK`, `OPEN`).
unsafe fn query_security_map(handle: HANDLE, guid: &GUID) -> HashMap<Vec<u8>, Vec<String>> {
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
    let status = WlanGetAvailableNetworkList(handle, guid, 0, null_mut(), &mut list);
    if status != 0 || list.is_null() {
        return HashMap::new();
    }
    let entries = &*list;
    let base = entries.Network.as_ptr();
    let mut map = HashMap::new();
    for index in 0..entries.dwNumberOfItems as usize {
        let entry = &*base.add(index);
        let length = (entry.dot11Ssid.uSSIDLength as usize).min(entry.dot11Ssid.ucSSID.len());
        if length == 0 {
            continue;
        }
        let ssid = entry.dot11Ssid.ucSSID[..length].to_vec();
        let security = security_for_network(
            entry.bSecurityEnabled != 0,
            entry.dot11DefaultAuthAlgorithm,
            entry.dot11DefaultCipherAlgorithm,
        );
        let values = map.entry(ssid).or_insert_with(Vec::new);
        values.extend(security);
        values.sort();
        values.dedup();
    }
    WlanFreeMemory(list.cast());
    map
}

fn security_for_network(security_enabled: bool, auth: i32, cipher: i32) -> Vec<String> {
    let mut result = Vec::new();
    let suite = match auth {
        DOT11_AUTH_ALGO_80211_OPEN => {
            if security_enabled {
                // Open-system authentication with an enabled cipher is unusual;
                // report the cipher rather than inventing a WPA suite.
                None
            } else {
                Some("OPEN".to_string())
            }
        }
        DOT11_AUTH_ALGO_80211_SHARED_KEY => Some("WEP".to_string()),
        DOT11_AUTH_ALGO_WPA => Some("WPA".to_string()),
        DOT11_AUTH_ALGO_WPA_PSK => Some("WPA-PSK".to_string()),
        DOT11_AUTH_ALGO_WPA_NONE => Some("WPA-NONE".to_string()),
        DOT11_AUTH_ALGO_RSNA => Some("WPA2".to_string()),
        DOT11_AUTH_ALGO_RSNA_PSK => Some("WPA2-PSK".to_string()),
        DOT11_AUTH_ALGO_WPA3 => Some("WPA3".to_string()),
        DOT11_AUTH_ALGO_WPA3_ENT => Some("WPA3-ENT".to_string()),
        DOT11_AUTH_ALGO_WPA3_SAE => Some("WPA3-SAE".to_string()),
        DOT11_AUTH_ALGO_OWE => Some("OWE".to_string()),
        _ => None,
    };
    if let Some(suite) = suite {
        result.push(suite);
    } else if security_enabled {
        // Unknown auth algorithm but the network is secured; report the cipher
        // so the record is never a fabricated WPA/WPA2/WPA3 claim.
        let cipher_name = match cipher {
            1 | 5 => "WEP",
            2 => "TKIP",
            4 => "CCMP",
            8 => "GCMP",
            10 => "CCMP-256",
            9 => "GCMP-256",
            _ => "SECURED",
        };
        result.push(cipher_name.to_string());
    }
    result.sort();
    result.dedup();
    result
}

struct AdapterDetails {
    gateway: Option<String>,
    dns_servers: Vec<String>,
    ip_address: Option<String>,
}

/// Reads the first Wi-Fi adapter (IF_TYPE_IEEE80211) address details,
/// preferring the adapter whose Windows name/network GUID matches the WLAN
/// interface GUID when multiple wireless adapters exist.
unsafe fn query_adapter_details(wlan_guid: GUID) -> Option<AdapterDetails> {
    let mut size = 0u32;
    let initial = GetAdaptersAddresses(
        0,
        GAA_FLAG_INCLUDE_GATEWAYS,
        null_mut(),
        null_mut(),
        &mut size,
    );
    if initial != 111 || size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let result = GetAdaptersAddresses(
        0,
        GAA_FLAG_INCLUDE_GATEWAYS,
        null_mut(),
        buffer.as_mut_ptr().cast(),
        &mut size,
    );
    if result != 0 {
        return None;
    }
    let mut fallback: Option<AdapterDetails> = None;
    let mut adapter = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !adapter.is_null() {
        let current = &*adapter;
        if current.IfType != IF_TYPE_IEEE80211 {
            adapter = current.Next;
            continue;
        }
        let details = read_adapter_details(current);
        let guid_match = adapter_name_matches(current.AdapterName, &wlan_guid)
            || guid_eq(&current.NetworkGuid, &wlan_guid);
        if guid_match {
            return Some(details);
        }
        if fallback.is_none() {
            fallback = Some(details);
        }
        adapter = current.Next;
    }
    fallback
}

unsafe fn read_adapter_details(current: &IP_ADAPTER_ADDRESSES_LH) -> AdapterDetails {
    let mut ip_address = None;
    let mut unicast = current.FirstUnicastAddress;
    while !unicast.is_null() {
        let address = (*unicast).Address;
        if let Some(text) = socket_address_text(address) {
            if is_valid_host_ip(&text) {
                // Prefer IPv4 unicast addresses.
                if text.contains(':') {
                    if ip_address.is_none() {
                        ip_address = Some(text);
                    }
                } else {
                    ip_address = Some(text);
                    break;
                }
            }
        }
        unicast = (*unicast).Next;
    }
    let mut gateway = None;
    let mut gateway_addr = current.FirstGatewayAddress;
    while !gateway_addr.is_null() {
        if let Some(text) = socket_address_text((*gateway_addr).Address) {
            if !text.contains(':') && is_valid_host_ip(&text) {
                gateway = Some(text);
                break;
            }
        }
        gateway_addr = (*gateway_addr).Next;
    }
    let mut dns_servers = Vec::new();
    let mut dns_addr = current.FirstDnsServerAddress;
    while !dns_addr.is_null() {
        if let Some(text) = socket_address_text((*dns_addr).Address) {
            if !dns_servers.contains(&text) {
                dns_servers.push(text);
            }
        }
        dns_addr = (*dns_addr).Next;
    }
    AdapterDetails {
        gateway,
        dns_servers,
        ip_address,
    }
}

/// Accepts only unicast, non-loopback, non-multicast IPv4/IPv6 addresses that
/// the server will accept as gateway/ip_address/dns values.
fn is_valid_host_ip(text: &str) -> bool {
    if let Ok(ip) = text.parse::<std::net::IpAddr>() {
        !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast()
    } else {
        false
    }
}

unsafe fn adapter_name_matches(name: windows_sys::core::PSTR, guid: &GUID) -> bool {
    if name.is_null() {
        return false;
    }
    let expected = format_guid(guid);
    std::ffi::CStr::from_ptr(name.cast::<i8>())
        .to_string_lossy()
        .trim_matches(|c| c == '{' || c == '}')
        .eq_ignore_ascii_case(&expected)
}

fn guid_eq(left: &GUID, right: &GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

unsafe fn socket_address_text(address: SOCKET_ADDRESS) -> Option<String> {
    if address.lpSockaddr.is_null() {
        return None;
    }
    let sockaddr = &*address.lpSockaddr;
    match sockaddr.sa_family {
        AF_INET => {
            let value = &*(address.lpSockaddr.cast::<SOCKADDR_IN>());
            Some(format_ipv4(value.sin_addr.S_un.S_addr))
        }
        AF_INET6 => {
            let value = &*(address.lpSockaddr.cast::<SOCKADDR_IN6>());
            Some(format_ipv6(&value.sin6_addr.u.Byte))
        }
        _ => None,
    }
}

fn format_ipv4(value: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (value >> 24) & 0xff,
        (value >> 16) & 0xff,
        (value >> 8) & 0xff,
        value & 0xff
    )
}

fn format_ipv6(bytes: &[u8; 16]) -> String {
    let segments = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    format!(
        "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
        segments[0],
        segments[1],
        segments[2],
        segments[3],
        segments[4],
        segments[5],
        segments[6],
        segments[7]
    )
}

/// Returns the first WLAN interface GUID for adapter matching.
fn first_guid(list: &WLAN_INTERFACE_INFO_LIST) -> GUID {
    list.InterfaceInfo[0].InterfaceGuid
}

fn normalize_rssi(value: i32) -> Option<f64> {
    if (-150..=0).contains(&value) {
        Some(value as f64)
    } else {
        None
    }
}

fn read_utf16(value: &[u16]) -> Option<String> {
    let end = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    if end == 0 {
        return None;
    }
    String::from_utf16(&value[..end]).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_dot11_auth_to_canonical_security_names() {
        assert_eq!(
            security_for_network(false, DOT11_AUTH_ALGO_80211_OPEN, 0),
            vec!["OPEN"]
        );
        assert_eq!(
            security_for_network(true, DOT11_AUTH_ALGO_RSNA_PSK, 4),
            vec!["WPA2-PSK"]
        );
        assert_eq!(
            security_for_network(true, DOT11_AUTH_ALGO_WPA3_SAE, 0),
            vec!["WPA3-SAE"]
        );
        assert_eq!(
            security_for_network(true, DOT11_AUTH_ALGO_WPA, 2),
            vec!["WPA"]
        );
        assert_eq!(
            security_for_network(true, DOT11_AUTH_ALGO_80211_SHARED_KEY, 1),
            vec!["WEP"]
        );
        // Unknown secured auth reports the cipher instead of a fabricated suite.
        assert_eq!(security_for_network(true, 999, 4), vec!["CCMP"]);
        assert_eq!(security_for_network(true, 999, 999), vec!["SECURED"]);
    }

    #[test]
    fn rejects_non_unicast_host_addresses() {
        assert!(is_valid_host_ip("192.168.1.20"));
        assert!(is_valid_host_ip("2409:8a34:2422:aed1:4d3d:2a45:c745:7155"));
        assert!(!is_valid_host_ip("0.0.0.0"));
        assert!(!is_valid_host_ip("127.0.0.1"));
        assert!(!is_valid_host_ip("224.0.0.10"));
        assert!(!is_valid_host_ip("not-an-ip"));
    }
}
