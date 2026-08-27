pub mod collector;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}
