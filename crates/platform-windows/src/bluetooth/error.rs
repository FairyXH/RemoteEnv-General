use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum BluetoothError {
    #[error("Bluetooth API error {0}")]
    Api(u32),
    #[error("invalid Bluetooth data: {0}")]
    InvalidData(String),
    #[error("Bluetooth provider is unavailable: {0}")]
    Unavailable(String),
}