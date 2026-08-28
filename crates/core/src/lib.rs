pub mod bluetooth;
pub mod collector;
pub mod config;
pub mod dispatcher;
pub mod protocol;
pub mod queue;
pub mod runtime;
pub mod state;
pub mod transport;
pub mod worker;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}
