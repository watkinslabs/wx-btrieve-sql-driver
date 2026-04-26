//! Import / export / migration workflow endpoints.
//!
//! Thin async wrappers over `db_config::commands::*` and friends. They
//! all run inside `spawn_blocking` because the underlying functions are
//! synchronous and may do real filesystem / network work. stdout from
//! the wrapped functions ends up on the server console — the client
//! only sees an `{ok,message}` summary.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;

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

fn db_path(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no project open"))
}

// ── Imports ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ImportIntRequest {
    /// One or more directories to scan for .INT files.
    pub dirs: Vec<String>,
    pub recursive: Option<bool>,
    pub db: Option<String>,
    pub schema: Option<String>,
}

pub async fn import_int(
    State(state): State<AppState>,
    Json(req): Json<ImportIntRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let dirs: Vec<PathBuf> = req.dirs.into_iter().map(PathBuf::from).collect();
    let recursive = req.recursive.unwrap_or(false);
    let db = req.db;
    let schema = req.schema;
    run_blocking(move || {
        db_config::commands::import::do_import_int(
            &p,
            &dirs,
            recursive,
            db.as_deref(),
            schema.as_deref(),
        )
    })
    .await?;
    Ok(Json(ok("INT import complete")))
}

#[derive(Deserialize)]
pub struct PathRequest {
    pub path: String,
}

/// Streaming variant — emits an SSE `log` event per progress line and
/// finishes with a `done` event carrying the import counts.
pub async fn import_int_stream(
    State(state): State<AppState>,
    Json(req): Json<ImportIntRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let p = db_path(&state)?;
    let dirs: Vec<PathBuf> = req.dirs.into_iter().map(PathBuf::from).collect();
    let recursive = req.recursive.unwrap_or(false);
    let db = req.db;
    let schema = req.schema;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    let tx_log = tx.clone();
    let tx_end = tx.clone();
    std::thread::spawn(move || {
        let result = db_config::commands::import::do_import_int_with_logger(
            &p,
            &dirs,
            recursive,
            db.as_deref(),
            schema.as_deref(),
            |line| {
                let _ = tx_log.send(Event::default().event("log").data(line.to_string()));
            },
        );
        let final_evt = match result {
            Ok(stats) => Event::default()
                .event("done")
                .json_data(serde_json::json!({
                    "imported": stats.imported,
                    "skipped": stats.skipped,
                })),
            Err(e) => Event::default().event("error").json_data(serde_json::json!({ "error": e })),
        };
        if let Ok(evt) = final_evt {
            let _ = tx_end.send(evt);
        }
        drop(tx_end);
    });
    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Some(evt) = rx.recv().await {
            yield Ok::<_, Infallible>(evt);
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

pub async fn import_mds(
    State(state): State<AppState>,
    Json(req): Json<PathRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let f = PathBuf::from(req.path);
    run_blocking(move || db_config::commands::import::do_import_mds(&p, &f)).await?;
    Ok(Json(ok("MDS import complete")))
}

#[derive(Deserialize)]
pub struct AnalyzeBRequest {
    pub path: String,
    pub table_name: Option<String>,
}

pub async fn analyze_b(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeBRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let f = PathBuf::from(req.path);
    let table = req.table_name;
    run_blocking(move || {
        db_config::commands::import::do_analyze_b(&p, &f, table.as_deref())
    })
    .await?;
    Ok(Json(ok("B-file analyzed; see server log for layout")))
}

// ── Exports ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExportIntRequest {
    pub out_dir: String,
    #[serde(default)]
    pub tables: Vec<String>,
}

pub async fn export_int(
    State(state): State<AppState>,
    Json(req): Json<ExportIntRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let out = PathBuf::from(req.out_dir);
    let tables = req.tables;
    run_blocking(move || db_config::commands::export::do_export_int(&p, &out, &tables)).await?;
    Ok(Json(ok("INT export complete")))
}

pub async fn export_mds(
    State(state): State<AppState>,
    Json(req): Json<PathRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let f = PathBuf::from(req.path);
    run_blocking(move || db_config::commands::export::do_export_mds(&p, &f)).await?;
    Ok(Json(ok("MDS export complete")))
}

#[derive(Deserialize)]
pub struct ExportDdlRequest {
    pub out: String,
    pub add_recnum: Option<bool>,
    #[serde(default)]
    pub tables: Vec<String>,
}

pub async fn export_ddl(
    State(state): State<AppState>,
    Json(req): Json<ExportDdlRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let out = PathBuf::from(req.out);
    let add_recnum = req.add_recnum.unwrap_or(false);
    let tables = req.tables;
    run_blocking(move || {
        db_config::commands::export::do_gen_ddl(&p, Some(&out), add_recnum, &tables)
    })
    .await?;
    Ok(Json(ok("DDL written")))
}

// ── Migration tracking ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MigrationRow {
    pub table_name: String,
    pub source_dir: String,
    pub field_count: u32,
    pub migrated: bool,
    pub migrated_at: Option<String>,
    pub row_count: Option<i64>,
    pub target_db: String,
}

pub async fn migration_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<MigrationRow>>, ApiError> {
    let p = db_path(&state)?;
    let rows = run_blocking(move || {
        let conn = db_config::db::open(&p).map_err(|e| e.to_string())?;
        db_config::db::upgrade_schema(&conn).map_err(|e| e.to_string())?;
        db_config::db::migration_status(&conn).map_err(|e| e.to_string())
    })
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| MigrationRow {
                table_name: r.table_name,
                source_dir: r.source_dir,
                field_count: r.field_count,
                migrated: r.migrated,
                migrated_at: r.migrated_at,
                row_count: r.row_count,
                target_db: r.target_db,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct MarkMigratedRequest {
    pub table: String,
    pub rows: Option<i64>,
    pub server: Option<String>,
    pub target_db: Option<String>,
}

pub async fn mark_migrated(
    State(state): State<AppState>,
    Json(req): Json<MarkMigratedRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let table = req.table;
    let rows = req.rows;
    let server = req.server.unwrap_or_default();
    let target_db = req.target_db.unwrap_or_default();
    run_blocking(move || {
        db_config::commands::migrate::do_mark_migrated(&p, &table, rows, &server, &target_db)
    })
    .await?;
    Ok(Json(ok("marked migrated")))
}

#[derive(Deserialize)]
pub struct TableRequest {
    pub table: String,
}

pub async fn clear_migrated(
    State(state): State<AppState>,
    Json(req): Json<TableRequest>,
) -> Result<Json<OpResult>, ApiError> {
    let p = db_path(&state)?;
    let table = req.table;
    run_blocking(move || db_config::commands::migrate::do_clear_migrated(&p, &table)).await?;
    Ok(Json(ok("cleared migration flag")))
}
