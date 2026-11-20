/// Errors produced by sources and the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("source `{0}` is not registered")]
    UnknownSource(String),

    #[error("source `{source_id}` does not support {operation}")]
    Unsupported {
        source_id: String,
        operation: &'static str,
    },

    #[error("no playable stream for `{0}`")]
    NoStream(String),

    #[error("source error: {0}")]
    Source(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
