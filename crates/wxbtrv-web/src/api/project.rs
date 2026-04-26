//! Project (wxbtrv.db file) loader endpoints.
//!
//! Operator flow: open existing → start working. Or create new → init →
//! work. Recent list is stored at the OS config path
//! (`~/.config/wxbtrv-web/recent.json` on Linux).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::state::{forget_recent, read_recent, ApiError, AppState, RecentEntry};

#[derive(Serialize)]
pub struct CurrentProject {
    pub path: Option<String>,
}

pub async fn current(State(state): State<AppState>) -> Json<CurrentProject> {
    Json(CurrentProject {
        path: state.db_path().map(|p| p.to_string_lossy().into_owned()),
    })
}

#[derive(Deserialize)]
pub struct OpenRequest {
    pub path: String,
}

pub async fn open_project(
    State(state): State<AppState>,
    Json(req): Json<OpenRequest>,
) -> Result<Json<CurrentProject>, ApiError> {
    let p = PathBuf::from(&req.path);
    if !p.exists() {
        return Err(ApiError::bad_request(format!(
            "no such file: {}",
            p.display()
        )));
    }
    if !looks_like_wxbtrv_db(&p) {
        return Err(ApiError::bad_request(
            "file does not look like a wxbtrv.db (missing config / btr_tables tables) — \
             use Create… instead if you want to initialize a fresh database here"
                .to_string(),
        ));
    }
    state.set_path(p.clone());
    Ok(Json(CurrentProject {
        path: Some(p.to_string_lossy().into_owned()),
    }))
}

pub async fn close_project(State(state): State<AppState>) -> Json<CurrentProject> {
    state.close();
    Json(CurrentProject { path: None })
}

#[derive(Deserialize)]
pub struct InitRequest {
    pub path: String,
    /// Allow overwriting an existing file (default false — error if file
    /// exists and isn't already a wxbtrv.db).
    pub overwrite: Option<bool>,
}

pub async fn init_project(
    State(state): State<AppState>,
    Json(req): Json<InitRequest>,
) -> Result<Json<CurrentProject>, ApiError> {
    let p = PathBuf::from(&req.path);
    if p.exists() && !req.overwrite.unwrap_or(false) && !looks_like_wxbtrv_db(&p) {
        return Err(ApiError::bad_request(format!(
            "{} already exists and is not a wxbtrv.db; pass overwrite=true to replace it",
            p.display()
        )));
    }
    db_config::commands::manage::do_init(&p).map_err(ApiError::bad_request)?;
    state.set_path(p.clone());
    Ok(Json(CurrentProject {
        path: Some(p.to_string_lossy().into_owned()),
    }))
}

pub async fn list_recent() -> Json<Vec<RecentEntry>> {
    Json(read_recent())
}

#[derive(Deserialize)]
pub struct ForgetRequest {
    pub path: String,
}

pub async fn forget(Json(req): Json<ForgetRequest>) -> Json<serde_json::Value> {
    forget_recent(&req.path);
    Json(serde_json::json!({"ok": true}))
}

/// Sniff a SQLite file to see if it has the wxbtrv config + tables we
/// expect. Cheap; we just open read-only and look for the schema rows.
fn looks_like_wxbtrv_db(p: &Path) -> bool {
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        p,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return false;
    };
    let count: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name IN ('config','btr_tables','btr_fields','btr_indexes')",
        [],
        |r| r.get(0),
    );
    matches!(count, Ok(n) if n >= 4)
}
