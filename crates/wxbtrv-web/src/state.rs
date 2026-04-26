//! AppState — handle to the currently-open `wxbtrv.db` file.
//!
//! One project open at a time, switchable at runtime via the
//! `/api/project/*` endpoints. Optional `--db <path>` on the CLI just
//! pre-opens a project at startup.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    /// Currently-open wxbtrv.db. `None` when the user hasn't opened a
    /// project yet — every endpoint that needs the DB returns 404 in
    /// that case so the UI can render a "no project" empty state.
    db_path: RwLock<Option<PathBuf>>,
}

impl AppState {
    pub fn new(initial: Option<PathBuf>) -> Self {
        let resolved = initial.or_else(|| {
            let p = std::env::current_dir().ok()?.join("wxbtrv.db");
            p.exists().then_some(p)
        });
        if let Some(p) = &resolved {
            push_recent(p);
        }
        Self {
            inner: Arc::new(Inner {
                db_path: RwLock::new(resolved),
            }),
        }
    }

    pub fn db_path(&self) -> Option<PathBuf> {
        self.inner.db_path.read().ok().and_then(|g| g.clone())
    }

    /// Switch the open project. Caller is responsible for ensuring the
    /// path exists / is initialized — `set_path()` is the dumb store.
    pub fn set_path(&self, path: PathBuf) {
        if let Ok(mut g) = self.inner.db_path.write() {
            *g = Some(path.clone());
        }
        push_recent(&path);
    }

    pub fn close(&self) {
        if let Ok(mut g) = self.inner.db_path.write() {
            *g = None;
        }
    }

    pub fn open(&self) -> Result<rusqlite::Connection, ApiError> {
        let p = self
            .db_path()
            .ok_or_else(|| ApiError::not_found("no project open"))?;
        rusqlite::Connection::open(&p)
            .map_err(|e| ApiError::internal(format!("open {}: {e}", p.display())))
    }

    pub fn open_rw(&self) -> Result<rusqlite::Connection, ApiError> {
        let p = self
            .db_path()
            .ok_or_else(|| ApiError::not_found("no project open"))?;
        rusqlite::Connection::open_with_flags(
            &p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|e| ApiError::internal(format!("open_rw {}: {e}", p.display())))
    }
}

// ── Recent-files store: ~/.config/wxbtrv-web/recent.json ──────────────────

const RECENT_MAX: usize = 12;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: String,
    /// Unix epoch seconds.
    pub opened_at: u64,
}

fn recent_file() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    let dir = base.join("wxbtrv-web");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("recent.json"))
}

pub fn read_recent() -> Vec<RecentEntry> {
    let Some(p) = recent_file() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(p) else { return Vec::new() };
    serde_json::from_str::<Vec<RecentEntry>>(&text).unwrap_or_default()
}

pub fn push_recent(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    let mut list = read_recent();
    list.retain(|e| e.path != path_str);
    list.insert(
        0,
        RecentEntry {
            path: path_str,
            opened_at: now_epoch(),
        },
    );
    if list.len() > RECENT_MAX {
        list.truncate(RECENT_MAX);
    }
    if let Some(p) = recent_file() {
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(p, json);
        }
    }
}

pub fn forget_recent(path: &str) {
    let mut list = read_recent();
    let len = list.len();
    list.retain(|e| e.path != path);
    if list.len() != len {
        if let Some(p) = recent_file() {
            if let Ok(json) = serde_json::to_string_pretty(&list) {
                let _ = std::fs::write(p, json);
            }
        }
    }
}

fn now_epoch() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
