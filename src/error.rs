#[derive(Debug, thiserror::Error)]
pub enum VibecheckError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Parse(String),

    #[error("{0}")]
    Ollama(String),

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    Index(String),
}
