use thiserror::Error;

#[derive(Debug, Error)]
pub enum WiFiError {
    #[error("WLAN API error {0}")]
    Api(u32),
    #[error("invalid WLAN data: {0}")]
    InvalidData(String),
    #[error("WLAN provider is unavailable: {0}")]
    Unavailable(String),
}
