/// Errors produced by sources and the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{0}` is not installed")]
    UnknownSource(String),

    #[error("nothing installed can play `{0}` tracks")]
    NoResolver(String),

    #[error("could not play `{track}`: {}", describe(.attempts))]
    AllResolversFailed {
        track: String,
        /// Each resolver tried, with its error, in order.
        attempts: Vec<(String, String)>,
    },

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

fn describe(attempts: &[(String, String)]) -> String {
    attempts
        .iter()
        .map(|(id, error)| format!("{id}: {error}"))
        .collect::<Vec<_>>()
        .join("; ")
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
