use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("server returned error: {0}")]
    ServerError(String),

    #[error("challenge not found: {0}")]
    ChallengeNotFound(String),

    #[error("shutdown signal received")]
    Shutdown,

    #[error("{0}")]
    Other(String),
}
