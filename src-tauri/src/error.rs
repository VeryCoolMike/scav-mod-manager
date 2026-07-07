use serde::Serialize;

/// Application error type. Serializes to a plain string so the frontend
/// receives a readable message from failed `invoke` calls.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),

    #[error("game folder is not configured")]
    NoGamePath,

    #[error("Nexus API key is not configured")]
    NoApiKey,

    #[error("Nexus API error ({status}): {body}")]
    Nexus { status: u16, body: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Message(s.into())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
