use super::error::BluetoothError;
use super::model::format_bluetooth_address;
use remote_env_core::bluetooth::{BluetoothObservation, BluetoothTransport};
use std::mem::size_of;
use std::ptr::null_mut;
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Devices::Bluetooth::{
    BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BLUETOOTH_FIND_RADIO_PARAMS,
    BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindFirstRadio,
    BluetoothFindNextDevice, BluetoothFindNextRadio, BluetoothFindRadioClose,
};

pub trait ClassicBluetoothScanner: Send + Sync {
    fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError>;
    fn available(&self) -> bool {
        true
    }
}

pub struct NativeClassicBluetoothScanner;
impl NativeClassicBluetoothScanner {
    pub fn new() -> Self {
        Self
    }
}
impl Default for NativeClassicBluetoothScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassicBluetoothScanner for NativeClassicBluetoothScanner {
    fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let mut radio = null_mut();
        let radio_params = BLUETOOTH_FIND_RADIO_PARAMS {
            dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
        };
        let radios = unsafe { BluetoothFindFirstRadio(&radio_params, &mut radio) };
        if radios.is_null() {
            return Err(BluetoothError::Unavailable(
                "no Classic Bluetooth radio".into(),
            ));
        }
        let mut result = Vec::new();
        loop {
            let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
                dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
                fReturnAuthenticated: 1,
                fReturnRemembered: 1,
                fReturnUnknown: 1,
                fReturnConnected: 1,
                fIssueInquiry: 1,
                cTimeoutMultiplier: 2,
                hRadio: radio,
            };
            let mut info = BLUETOOTH_DEVICE_INFO {
                dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                ..unsafe { std::mem::zeroed() }
            };
            let devices = unsafe { BluetoothFindFirstDevice(&search, &mut info) };
            if !devices.is_null() {
                loop {
                    let end = info
                        .szName
                        .iter()
                        .position(|value| *value == 0)
                        .unwrap_or(info.szName.len());
                    let name = String::from_utf16_lossy(&info.szName[..end]);
                    let address = unsafe { info.Address.Anonymous.ullLong };
                    result.push(BluetoothObservation {
                        address: format_bluetooth_address(address),
                        transport: BluetoothTransport::Classic,
                        name: (!name.is_empty()).then_some(name),
                        rssi: None,
                        service_uuids: Vec::new(),
                        manufacturer_data: Vec::new(),
                        service_data: Vec::new(),
                        raw_advertisement_sections: Vec::new(),
                        connectable: None,
                        class_of_device: Some(info.ulClassofDevice),
                        appearance: None,
                        tx_power: None,
                        timestamp_ms: now,
                    });
                    if unsafe { BluetoothFindNextDevice(devices, &mut info) } == 0 {
                        break;
                    }
                }
                unsafe {
                    BluetoothFindDeviceClose(devices);
                }
            }
            if unsafe { BluetoothFindNextRadio(radios, &mut radio) } == 0 {
                break;
            }
        }
        unsafe {
            BluetoothFindRadioClose(radios);
        }
        result.sort_by(|a, b| a.address.cmp(&b.address));
        result.dedup_by(|a, b| a.address == b.address);
        Ok(result)
    }
    fn available(&self) -> bool {
        let mut radio = null_mut();
        let params = BLUETOOTH_FIND_RADIO_PARAMS {
            dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
        };
        let handle = unsafe { BluetoothFindFirstRadio(&params, &mut radio) };
        if handle.is_null() {
            return false;
        }
        unsafe {
            BluetoothFindRadioClose(handle);
        }
        true
    }
}
