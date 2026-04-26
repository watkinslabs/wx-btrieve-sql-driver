//! .B → backend bulk import endpoints. Wraps `btr_import::runner` so
//! the UI can drive Btrieve flat-file migrations into the configured
//! backend (MSSQL/Postgres/SQLite) without shelling out.

use axum::{extract::State, Json};
use btr_import::runner::{self, ImportOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::state::{ApiError, AppState};

#[derive(Deserialize, Default)]
pub struct ImportBOptions {
    pub create: Option<bool>,
    pub truncate: Option<bool>,
    pub dry_run: Option<bool>,
    pub batch: Option<usize>,
    pub auto_schema: Option<bool>,
    pub save_schema: Option<bool>,
    pub collation: Option<String>,
    pub table: Option<String>,
}

impl ImportBOptions {
    fn into_runner(self) -> ImportOptions {
        ImportOptions {
            create: self.create.unwrap_or(false),
            truncate: self.truncate.unwrap_or(false),
            dry_run: self.dry_run.unwrap_or(false),
            batch: self.batch.unwrap_or(200),
            auto_schema: self.auto_schema.unwrap_or(false),
            save_schema: self.save_schema.unwrap_or(false),
            collation: self.collation.unwrap_or_default(),
            table: self.table,
        }
    }
}

#[derive(Deserialize)]
pub struct ImportBFilesRequest {
    pub files: Vec<String>,
    #[serde(flatten)]
    pub options: ImportBOptions,
}

#[derive(Deserialize)]
pub struct ImportBDirRequest {
    pub dir: String,
    pub recursive: Option<bool>,
    #[serde(flatten)]
    pub options: ImportBOptions,
}

#[derive(Serialize)]
pub struct ImportStatsResponse {
    pub files_ok: usize,
    pub files_skipped: usize,
    pub records: u64,
    pub log: Vec<String>,
}

fn db_path(state: &AppState) -> Result<PathBuf, ApiError> {
    state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no project open"))
}

async fn run<F, R>(f: F) -> Result<R, ApiError>
where
    F: FnOnce() -> Result<R, String> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(format!("join: {e}")))?
        .map_err(ApiError::bad_request)
}

pub async fn import_files(
    State(state): State<AppState>,
    Json(req): Json<ImportBFilesRequest>,
) -> Result<Json<ImportStatsResponse>, ApiError> {
    let p = db_path(&state)?;
    let files: Vec<PathBuf> = req.files.into_iter().map(PathBuf::from).collect();
    let opts = req.options.into_runner();
    let stats = run(move || runner::import_files(&p, &files, &opts)).await?;
    Ok(Json(ImportStatsResponse {
        files_ok: stats.files_ok,
        files_skipped: stats.files_skipped,
        records: stats.records,
        log: stats.log,
    }))
}

pub async fn import_dir(
    State(state): State<AppState>,
    Json(req): Json<ImportBDirRequest>,
) -> Result<Json<ImportStatsResponse>, ApiError> {
    let p = db_path(&state)?;
    let dir = PathBuf::from(req.dir);
    let recursive = req.recursive.unwrap_or(false);
    let opts = req.options.into_runner();
    let stats = run(move || {
        let files = runner::collect_b_files(&dir, recursive);
        if files.is_empty() {
            return Err(format!("no .B files found in {}", dir.display()));
        }
        runner::import_files(&p, &files, &opts)
    })
    .await?;
    Ok(Json(ImportStatsResponse {
        files_ok: stats.files_ok,
        files_skipped: stats.files_skipped,
        records: stats.records,
        log: stats.log,
    }))
}

#[derive(Deserialize)]
pub struct InfoRequest {
    pub path: String,
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub path: String,
    pub version: u8,
    pub page_size: u32,
    pub logical_rec_len: u32,
    pub physical_rec_len: u32,
    pub key_count: u16,
    pub declared_records: u32,
    pub page_count: u32,
    pub file_size: u64,
    pub active_records: u32,
    pub uncovered_bytes: Option<u32>,
    pub record_kind: Option<String>,
    pub keys: Vec<KeyInfo>,
}

#[derive(Serialize)]
pub struct KeyInfo {
    pub number: u16,
    pub segments: Vec<KeySegmentInfo>,
}

#[derive(Serialize)]
pub struct KeySegmentInfo {
    pub offset: u16,
    pub length: u16,
    pub data_type: u8,
    pub null_value: u8,
    pub descending: bool,
    pub allows_dups: bool,
}

pub async fn info(
    Json(req): Json<InfoRequest>,
) -> Result<Json<InfoResponse>, ApiError> {
    let path = PathBuf::from(req.path);
    let r = run(move || runner::info(&path)).await?;
    Ok(Json(InfoResponse {
        path: r.path,
        version: r.version,
        page_size: r.page_size,
        logical_rec_len: r.logical_rec_len,
        physical_rec_len: r.physical_rec_len,
        key_count: r.key_count,
        declared_records: r.declared_records,
        page_count: r.page_count,
        file_size: r.file_size,
        active_records: r.active_records,
        uncovered_bytes: r.uncovered_bytes,
        record_kind: r.record_kind,
        keys: r
            .keys
            .into_iter()
            .map(|k| KeyInfo {
                number: k.number,
                segments: k
                    .segments
                    .into_iter()
                    .map(|s| KeySegmentInfo {
                        offset: s.offset,
                        length: s.length,
                        data_type: s.data_type,
                        null_value: s.null_value,
                        descending: s.descending,
                        allows_dups: s.allows_dups,
                    })
                    .collect(),
            })
            .collect(),
    }))
}
