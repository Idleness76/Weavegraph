use thiserror::Error;

#[derive(Debug, Error)]
pub enum RagError {
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid document: {0}")]
    InvalidDocument(String),
    #[error("chunking failed: {0}")]
    Chunking(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("io error: {0}")]
    Io(String),
}

impl From<reqwest::Error> for RagError {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(err.to_string())
    }
}

impl From<std::io::Error> for RagError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}
