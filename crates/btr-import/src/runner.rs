//! Programmatic import driver — same workflow as the CLI's `import`
//! subcommand, but returns a structured stats/log payload instead of
//! writing to stdout. Used by `wxbtrv-web` to drive .B → backend bulk
//! imports from the UI without shelling out.

use crate::{bfile, schema, sqlsrv};
use btr_types::{covered_bytes, parse_schema, schema_to_int, unpack_row, IntFile};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub create: bool,
    pub truncate: bool,
    pub dry_run: bool,
    pub batch: usize,
    pub auto_schema: bool,
    pub save_schema: bool,
    /// Collation override; empty string means "use db config / db default".
    pub collation: String,
    /// Override table name. Only honored when `files.len() == 1`.
    pub table: Option<String>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            create: false,
            truncate: false,
            dry_run: false,
            batch: 200,
            auto_schema: false,
            save_schema: false,
            collation: String::new(),
            table: None,
        }
    }
}

#[derive(Default, Debug)]
pub struct ImportStats {
    pub files_ok: usize,
    pub files_skipped: usize,
    pub records: u64,
    /// Per-file or per-step messages, suitable for surfacing in a UI.
    pub log: Vec<String>,
}

/// Import a list of .B files into the backend specified by `db_path`'s
/// connection config. The schema of each file is loaded from the same
/// wxbtrv.db (or auto-derived from the FCR if `opts.auto_schema`).
pub fn import_files(
    db_path: &Path,
    files: &[PathBuf],
    opts: &ImportOptions,
) -> Result<ImportStats, String> {
    if opts.table.is_some() && files.len() > 1 {
        return Err("table override only valid with a single file".into());
    }

    let conn = if opts.save_schema {
        schema::open_db_rw(db_path)?
    } else {
        schema::open_db(db_path)?
    };
    let cfg = schema::load_config(&conn)?;
    let collation = if opts.collation.is_empty() {
        schema::get_config(&conn, "IMPORT", "COLLATION")
    } else {
        opts.collation.clone()
    };

    let mut sql_conn = if opts.dry_run {
        None
    } else {
        Some(sqlsrv::connect(&cfg)?)
    };

    let mut stats = ImportStats::default();
    for file in files {
        let table_name = opts
            .table
            .clone()
            .unwrap_or_else(|| schema::table_name_from_path(file));
        match load_schema_for_file(&conn, &table_name, file, opts, &mut stats) {
            Ok(int_file) => match import_one(
                file,
                &int_file,
                sql_conn.as_mut(),
                cfg.backend.as_str(),
                opts,
                &collation,
                &mut stats,
            ) {
                Ok(()) => {
                    stats.files_ok += 1;
                }
                Err(e) => {
                    stats.files_skipped += 1;
                    stats.log.push(format!("skip {}: {e}", file.display()));
                }
            },
            Err(e) => {
                stats.files_skipped += 1;
                stats.log.push(format!("skip {}: {e}", file.display()));
            }
        }
    }
    Ok(stats)
}

fn load_schema_for_file(
    conn: &rusqlite::Connection,
    table_name: &str,
    file: &Path,
    opts: &ImportOptions,
    stats: &mut ImportStats,
) -> Result<IntFile, String> {
    match schema::load_table(conn, table_name) {
        Ok(s) => Ok(s),
        Err(_) if opts.auto_schema => {
            let b = parse_schema(file)?;
            let source_file = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let int_file = schema_to_int(&b, table_name, &source_file);

            let uncovered = b.record_length as u32 - covered_bytes(&b);
            if uncovered > 0 {
                stats.log.push(format!(
                    "{}: {} bytes uncovered by key fields — non-key fields missing from import",
                    table_name, uncovered
                ));
            }
            if opts.save_schema {
                schema::upsert_table(conn, &int_file)?;
                stats
                    .log
                    .push(format!("{}: schema saved to wxbtrv.db", table_name));
            }
            Ok(int_file)
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
fn import_one(
    path: &Path,
    int_file: &IntFile,
    sql_conn: Option<&mut sqlsrv::SqlConnection>,
    backend: &str,
    opts: &ImportOptions,
    collation: &str,
    stats: &mut ImportStats,
) -> Result<(), String> {
    let bf = bfile::BtrieveFile::open(path)?;
    let h = &bf.header;

    if h.logical_rec_len > 0 && h.logical_rec_len != int_file.record_length {
        stats.log.push(format!(
            "warning: {} FCR rec_len={} but schema record_length={} — using schema",
            path.display(),
            h.logical_rec_len,
            int_file.record_length
        ));
    }

    let mut conn_holder = sql_conn;
    if let Some(c) = conn_holder.as_deref_mut() {
        if opts.create {
            let ddl = sqlsrv::gen_create_table(backend, int_file, collation);
            sqlsrv::execute(c, &ddl)
                .map_err(|e| format!("CREATE TABLE failed for {}: {e}", int_file.table_name))?;
        }
        if opts.truncate {
            let tref = sqlsrv::table_ref(backend, int_file);
            let stmt = if backend == "sqlite" {
                format!("DELETE FROM {tref}")
            } else {
                format!("TRUNCATE TABLE {tref}")
            };
            sqlsrv::execute(c, &stmt)
                .map_err(|e| format!("TRUNCATE failed for {}: {e}", int_file.table_name))?;
        }
    }

    let mut total = 0u64;
    let mut batch: Vec<Vec<(String, String)>> = Vec::with_capacity(opts.batch);
    for record in bf.records() {
        batch.push(unpack_row(&int_file.fields, &record));
        if batch.len() >= opts.batch {
            if let Some(c) = conn_holder.as_deref_mut() {
                sqlsrv::batch_insert(c, int_file, &batch, opts.dry_run)?;
            }
            total += batch.len() as u64;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        if let Some(c) = conn_holder.as_deref_mut() {
            sqlsrv::batch_insert(c, int_file, &batch, opts.dry_run)?;
        }
        total += batch.len() as u64;
    }

    stats.records += total;
    let mode = if opts.dry_run { " (dry-run)" } else { "" };
    stats.log.push(format!(
        "{}: {} records → {}{}",
        path.display(),
        total,
        sqlsrv::table_ref(backend, int_file),
        mode
    ));
    Ok(())
}

/// Header / FCR summary for a single .B file.
#[derive(Debug, Clone)]
pub struct InfoReport {
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
    pub keys: Vec<KeyInfo>,
    pub record_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub number: u16,
    pub segments: Vec<KeySegmentInfo>,
}

#[derive(Debug, Clone)]
pub struct KeySegmentInfo {
    pub offset: u16,
    pub length: u16,
    pub data_type: u8,
    pub null_value: u8,
    pub descending: bool,
    pub allows_dups: bool,
}

pub fn info(path: &Path) -> Result<InfoReport, String> {
    let bf = bfile::BtrieveFile::open(path)?;
    let h = &bf.header;
    let mut report = InfoReport {
        path: path.display().to_string(),
        version: h.version,
        page_size: h.page_size,
        logical_rec_len: h.logical_rec_len,
        physical_rec_len: h.physical_rec_len,
        key_count: h.key_count,
        declared_records: h.record_count,
        page_count: h.page_count,
        file_size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        active_records: bf.records().count() as u32,
        uncovered_bytes: None,
        keys: Vec::new(),
        record_kind: None,
    };
    if let Ok(s) = parse_schema(path) {
        report.record_kind = Some(format!("{:?}", s.record_kind));
        let uncovered = s.record_length as u32 - covered_bytes(&s);
        if uncovered > 0 {
            report.uncovered_bytes = Some(uncovered);
        }
        for key in &s.keys {
            report.keys.push(KeyInfo {
                number: key.number,
                segments: key
                    .segments
                    .iter()
                    .map(|seg| KeySegmentInfo {
                        offset: seg.offset,
                        length: seg.length,
                        data_type: seg.data_type,
                        null_value: seg.null_value,
                        descending: seg.descending,
                        allows_dups: seg.allows_dups,
                    })
                    .collect(),
            });
        }
    }
    Ok(report)
}

/// Walk a directory for .B files (case-insensitive on the extension).
pub fn collect_b_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && recursive {
                stack.push(path);
            } else if path.is_file()
                && path
                    .extension()
                    .map(|e| e.to_string_lossy().eq_ignore_ascii_case("b"))
                    .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
