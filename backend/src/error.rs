use std::io;

/// Crate-wide result type. Prefer this over `Result<T, String>` so errors keep
/// their source and can be handled by variant rather than by matching text.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// mpv reported a command failure.
    #[error("mpv error: {0}")]
    Mpv(String),

    /// `get_property` was called for a property that is unavailable because no
    /// file is currently loaded. Callers treat this as an empty value rather
    /// than an error.
    #[error("mpv property unavailable")]
    PropertyUnavailable,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("subtitle error: {0}")]
    Subtitle(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("background task failed: {0}")]
    Task(String),

    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(std::path::PathBuf),
}
