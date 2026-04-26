//! Schema-editing endpoints — add/remove fields and indexes, set table
//! properties, delete tables.
//!
//! Wraps `db_config::commands::manage::*` and runs synchronously inside
//! `spawn_blocking`. The existing read-only `/api/tables/*` endpoints
//! handle reflection.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::state::{ApiError, AppState};

#[derive(Serialize)]
pub struct OpResult {
    pub ok: bool,
    pub message: String,
}

fn ok(msg: impl Into<String>) -> OpResult {
    OpResult {
        ok: true,
        message: msg.into(),
    }
}

async fn run_blocking<F, R>(f: F) -> Result<R, ApiError>
where
    F: FnOnce() -> Result<R, String> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(format!("join: {e}")))?
        .map_err(ApiError::bad_request)
}

fn db_path(state: &AppState) -> Result<std::path::PathBuf, ApiError> {
    state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no project open"))
}

#[derive(Deserialize)]
pub struct SetTableProp {
    pub key: String,
    pub value: String,
}

pub async fn set_table_prop(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Json(req): Json<SetTableProp>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || db_config::commands::manage::do_set_table(&p, &table, &req.key, &req.value))
        .await?;
    Ok(Json(ok("table updated")))
}

#[derive(Deserialize)]
pub struct AddField {
    pub num: u32,
    pub name: String,
    pub native_type: i32,
    pub length: u32,
    pub offset: u32,
    pub index: Option<u32>,
    pub default: Option<String>,
}

pub async fn add_field(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Json(req): Json<AddField>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || {
        db_config::commands::manage::do_add_field(
            &p,
            &table,
            req.num,
            &req.name,
            req.native_type,
            req.length,
            req.offset,
            req.index,
            req.default.as_deref(),
        )
    })
    .await?;
    Ok(Json(ok("field saved")))
}

pub async fn rm_field(
    State(state): State<AppState>,
    Path((table, num)): Path<(String, u32)>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || db_config::commands::manage::do_rm_field(&p, &table, num)).await?;
    Ok(Json(ok("field removed")))
}

#[derive(Deserialize)]
pub struct AddIndex {
    pub num: u32,
    /// Comma-separated list of field numbers, e.g. `"1,3,5"`.
    pub fields: String,
    /// One value or a comma-separated list matching the `fields` count.
    pub attrs: String,
    /// "1" for descending, "0" for ascending. Single or comma-list.
    pub desc: String,
}

pub async fn add_index(
    State(state): State<AppState>,
    Path(table): Path<String>,
    Json(req): Json<AddIndex>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || {
        db_config::commands::manage::do_add_index(
            &p, &table, req.num, &req.fields, &req.attrs, &req.desc,
        )
    })
    .await?;
    Ok(Json(ok("index saved")))
}

pub async fn rm_index(
    State(state): State<AppState>,
    Path((table, num)): Path<(String, u32)>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || db_config::commands::manage::do_rm_index(&p, &table, num)).await?;
    Ok(Json(ok("index removed")))
}

pub async fn rm_table(
    State(state): State<AppState>,
    Path(table): Path<String>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    run_blocking(move || db_config::commands::manage::do_rm_table(&p, &table)).await?;
    Ok(Json(ok("table deleted")))
}
