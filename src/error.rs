use thiserror::Error;

#[derive(Debug, Error)]
pub enum SerialKeelError {
    #[error("Unexpected code path in library- likely bug!")]
    Bug,
}
