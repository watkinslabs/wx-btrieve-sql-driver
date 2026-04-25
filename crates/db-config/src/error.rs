#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::InvalidInput(s)
    }
}
