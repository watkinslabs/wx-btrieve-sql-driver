//! AppState — request-scoped handle to the wxbtrv.db file.
//!
//! Resolution: explicit `--db` flag wins. Otherwise we fall back to
//! `wxbtrv.db` in CWD; if that's missing, we leave the path None and
//! the API endpoints surface a 404 / explanatory error.

use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db_path: Option<PathBuf>,
}

impl AppState {
    pub fn new(explicit_db: Option<PathBuf>) -> Self {
        let resolved = explicit_db.or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            let p = cwd.join("wxbtrv.db");
            if p.exists() {
                Some(p)
            } else {
                None
            }
        });
        Self {
            inner: Arc::new(Inner { db_path: resolved }),
        }
    }

    /// Returns the configured path, or 404-mapped `None` if no
    /// wxbtrv.db is reachable.
    pub fn db_path(&self) -> Option<&PathBuf> {
        self.inner.db_path.as_ref()
    }

    /// Open a fresh read-only connection. Each request gets its own.
    pub fn open(&self) -> Result<rusqlite::Connection, ApiError> {
        let p = self
            .inner
            .db_path
            .as_ref()
            .ok_or_else(|| ApiError::not_found("no wxbtrv.db found — pass --db <path>"))?;
        rusqlite::Connection::open(p).map_err(|e| ApiError::internal(format!("open {}: {e}", p.display())))
    }

    /// Open a fresh read-write connection. Reserved for the upcoming
    /// import / set-config endpoints; un-used today but harmless.
    #[allow(dead_code)]
    pub fn open_rw(&self) -> Result<rusqlite::Connection, ApiError> {
        // SQLite OpenFlags default already allows read+write+create; explicit:
        let p = self
            .inner
            .db_path
            .as_ref()
            .ok_or_else(|| ApiError::not_found("no wxbtrv.db found — pass --db <path>"))?;
        rusqlite::Connection::open_with_flags(
            p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|e| ApiError::internal(format!("open_rw {}: {e}", p.display())))
    }
}

// ── Error type ────────────────────────────────────────────────────────────

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg.into(),
        }
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg.into(),
        }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16(),
        }));
        (self.status, body).into_response()
    }
}

impl From<String> for ApiError {
    fn from(s: String) -> Self {
        Self::internal(s)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(format!("sqlite: {e}"))
    }
}
