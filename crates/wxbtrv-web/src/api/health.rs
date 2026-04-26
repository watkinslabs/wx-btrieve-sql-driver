//! Health overview — single endpoint that consolidates connection
//! status, per-table schema drift, and migration progress so the UI
//! can render a "is my project ready?" dashboard.

use axum::{extract::State, Json};
use btr_import::{schema as bschema, sqlsrv};
use serde::Serialize;
use std::collections::HashMap;

use crate::state::{ApiError, AppState};

#[derive(Serialize, Default)]
pub struct ProjectHealth {
    pub path: Option<String>,
    pub connection_ok: Option<bool>,
    pub connection_message: String,
    pub backend: String,
    pub tables: Vec<TableHealth>,
    pub summary: HealthSummary,
}

#[derive(Serialize, Default)]
pub struct HealthSummary {
    pub total: usize,
    pub in_sync: usize,
    pub drifted: usize,
    pub missing_table: usize,
    pub no_schema: usize,
    pub migrated: usize,
}

#[derive(Serialize)]
pub struct TableHealth {
    pub table_name: String,
    pub source_dir: String,
    pub field_count: u32,
    pub has_schema: bool,
    pub backend_table_exists: bool,
    pub columns_match: bool,
    pub missing_in_backend: usize,
    pub extra_in_backend: usize,
    pub type_mismatches: usize,
    pub migrated: bool,
    pub row_count: Option<i64>,
    /// Resolved section name (e.g. "PACIFIC") used to pick a connection.
    /// Empty means global defaults were used.
    pub section: String,
    /// Per-table status string, suitable for a colored badge in the UI.
    /// One of: "ok", "drift", "missing", "no_schema", "no_connection".
    pub status: &'static str,
}

pub async fn overview(
    State(state): State<AppState>,
) -> Result<Json<ProjectHealth>, ApiError> {
    let Some(db) = state.db_path() else {
        return Ok(Json(ProjectHealth::default()));
    };

    let result = tokio::task::spawn_blocking(move || -> Result<ProjectHealth, String> {
        let mut out = ProjectHealth {
            path: Some(db.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let conn = bschema::open_db(&db)?;

        // Pull every table from btr_tables along with field counts and
        // migration state in one query.
        let mut stmt = conn
            .prepare(
                "SELECT t.table_name, t.source_dir, t.schema_name, t.db_name,
                        (SELECT COUNT(*) FROM btr_fields WHERE table_id = t.id) AS fc,
                        COALESCE(t.migrated, 0),
                        t.row_count
                 FROM btr_tables t
                 ORDER BY t.table_name, t.source_dir",
            )
            .map_err(|e| format!("query: {e}"))?;
        struct Row {
            name: String,
            source_dir: String,
            field_count: u32,
            migrated: bool,
            row_count: Option<i64>,
        }
        let rows: Vec<Row> = stmt
            .query_map([], |r| {
                Ok(Row {
                    name: r.get(0)?,
                    source_dir: r.get(1)?,
                    field_count: r.get::<_, i64>(4)? as u32,
                    migrated: r.get::<_, i64>(5)? != 0,
                    row_count: r.get(6)?,
                })
            })
            .map_err(|e| format!("query: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("query: {e}"))?;
        drop(stmt);

        // Group tables by resolved section so we open one connection
        // per section instead of one per table.
        let mut by_section: HashMap<String, Vec<&Row>> = HashMap::new();
        for r in &rows {
            let section = section_from_source_dir(&r.source_dir).unwrap_or_default();
            by_section.entry(section).or_default().push(r);
        }

        // Health for the global connection (so the dashboard can render
        // a banner when nothing's reachable).
        let global_cfg = resolve_config(&conn, None)?;
        out.backend = global_cfg.backend.clone();
        match sqlsrv::connect(&global_cfg) {
            Ok(_) => {
                out.connection_ok = Some(true);
                out.connection_message = format!("{} connection OK", global_cfg.backend);
            }
            Err(e) => {
                out.connection_ok = Some(false);
                out.connection_message = e;
            }
        }

        for (section, group) in by_section {
            let cfg = resolve_config(&conn, if section.is_empty() { None } else { Some(&section) })?;
            let mut sql = match sqlsrv::connect(&cfg) {
                Ok(c) => Some(c),
                Err(_) => None,
            };
            for r in group {
                let int_file = bschema::load_table(&conn, &r.name).ok();
                let has_schema = int_file
                    .as_ref()
                    .map(|f| !f.fields.is_empty())
                    .unwrap_or(false);

                let mut backend_exists = false;
                let mut missing = 0;
                let mut extra = 0;
                let mut mismatches = 0;
                let mut columns_match = false;
                if let (Some(f), Some(c)) = (int_file.as_ref(), sql.as_mut()) {
                    if let Ok(backend_cols) = fetch_columns(c, &cfg.backend, f) {
                        backend_exists = !backend_cols.is_empty();
                        if backend_exists && has_schema {
                            let project: HashMap<String, String> = f
                                .fields
                                .iter()
                                .map(|fld| {
                                    (
                                        fld.name.to_ascii_uppercase(),
                                        sqlsrv::sql_type_str(&cfg.backend, fld, ""),
                                    )
                                })
                                .collect();
                            let backend: HashMap<String, String> = backend_cols
                                .iter()
                                .map(|c| (c.0.to_ascii_uppercase(), c.1.clone()))
                                .collect();
                            for (name, expected) in &project {
                                match backend.get(name) {
                                    None => missing += 1,
                                    Some(actual) => {
                                        if !types_match(&cfg.backend, expected, actual) {
                                            mismatches += 1;
                                        }
                                    }
                                }
                            }
                            for (name, _) in &backend {
                                if !project.contains_key(name) {
                                    extra += 1;
                                }
                            }
                            columns_match = missing == 0 && extra == 0 && mismatches == 0;
                        }
                    }
                }

                let status: &'static str = if !has_schema {
                    "no_schema"
                } else if sql.is_none() {
                    "no_connection"
                } else if !backend_exists {
                    "missing"
                } else if columns_match {
                    "ok"
                } else {
                    "drift"
                };

                out.tables.push(TableHealth {
                    table_name: r.name.clone(),
                    source_dir: r.source_dir.clone(),
                    field_count: r.field_count,
                    has_schema,
                    backend_table_exists: backend_exists,
                    columns_match,
                    missing_in_backend: missing,
                    extra_in_backend: extra,
                    type_mismatches: mismatches,
                    migrated: r.migrated,
                    row_count: r.row_count,
                    section: section.clone(),
                    status,
                });
            }
        }

        out.tables
            .sort_by(|a, b| a.table_name.cmp(&b.table_name));
        for t in &out.tables {
            out.summary.total += 1;
            match t.status {
                "ok" => out.summary.in_sync += 1,
                "drift" => out.summary.drifted += 1,
                "missing" => out.summary.missing_table += 1,
                "no_schema" => out.summary.no_schema += 1,
                _ => {}
            }
            if t.migrated {
                out.summary.migrated += 1;
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| ApiError::internal(format!("join: {e}")))?
    .map_err(ApiError::bad_request)?;

    Ok(Json(result))
}

// ── Helpers (lifted from diff.rs / browser.rs) ────────────────────────

fn section_from_source_dir(source_dir: &str) -> Option<String> {
    if source_dir.is_empty() {
        return None;
    }
    let trimmed = source_dir.trim_end_matches(['/', '\\']);
    let basename = trimmed
        .rsplit_once(['/', '\\'])
        .map(|(_, b)| b)
        .unwrap_or(trimmed);
    if basename.is_empty() {
        None
    } else {
        Some(basename.to_ascii_uppercase())
    }
}

fn resolve_config(
    conn: &rusqlite::Connection,
    section: Option<&str>,
) -> Result<bschema::SqlConfig, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut load = |sec: &str| -> Result<(), String> {
        let mut stmt = conn
            .prepare("SELECT key, value FROM config WHERE section = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![sec], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (k, v) = row.map_err(|e| e.to_string())?;
            if !v.is_empty() {
                map.insert(k.to_ascii_uppercase(), v);
            }
        }
        Ok(())
    };
    load("config")?;
    if let Some(sec) = section {
        load(sec)?;
    }
    let backend = match map
        .get("BACKEND")
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("postgres") | Some("postgresql") | Some("pg") => "postgres".to_string(),
        Some("sqlite") | Some("sqlite3") => "sqlite".to_string(),
        _ => "mssql".to_string(),
    };
    Ok(bschema::SqlConfig {
        backend,
        server: map.remove("SERVER").unwrap_or_default(),
        database: map.remove("DATABASE").unwrap_or_default(),
        schema: map.remove("SCHEMA").unwrap_or_default(),
        user: map.remove("USER").unwrap_or_default(),
        password: map.remove("PASSWORD").unwrap_or_default(),
    })
}

fn fetch_columns(
    conn: &mut sqlsrv::SqlConnection,
    backend: &str,
    int_file: &btr_types::IntFile,
) -> Result<Vec<(String, String)>, String> {
    match (backend, conn) {
        ("sqlite", sqlsrv::SqlConnection::Sqlite(c)) => {
            let sql = format!(
                "PRAGMA table_info(\"{}\")",
                int_file.table_name.replace('"', "\"\"")
            );
            let mut stmt = c.prepare(&sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        }
        ("postgres", sqlsrv::SqlConnection::Postgres(c)) => {
            let schema = if int_file.schema_name.is_empty() {
                "public"
            } else {
                &int_file.schema_name
            };
            let rows = c
                .query(
                    "SELECT column_name, data_type FROM information_schema.columns
                     WHERE lower(table_name) = lower($1) AND lower(table_schema) = lower($2)",
                    &[&int_file.table_name, &schema],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    (
                        r.get::<_, String>(0),
                        r.get::<_, String>(1).to_ascii_uppercase(),
                    )
                })
                .collect())
        }
        ("mssql", sqlsrv::SqlConnection::Mssql(c)) => mssql_columns(c, int_file),
        _ => Err("backend mismatch".into()),
    }
}

fn mssql_columns(
    c: &mut odbc_api::Connection<'static>,
    int_file: &btr_types::IntFile,
) -> Result<Vec<(String, String)>, String> {
    use odbc_api::{Cursor, IntoParameter};
    let mut sql = String::from(
        "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE LOWER(TABLE_NAME) = LOWER(?)",
    );
    if !int_file.schema_name.is_empty() {
        sql.push_str(" AND LOWER(TABLE_SCHEMA) = LOWER(?)");
    }
    let mut params: Vec<Box<dyn odbc_api::parameter::InputParameter>> =
        vec![Box::new(int_file.table_name.clone().into_parameter())];
    if !int_file.schema_name.is_empty() {
        params.push(Box::new(int_file.schema_name.clone().into_parameter()));
    }
    let mut cursor = c
        .execute(&sql, params.as_slice())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no result set".to_string())?;
    let mut out = Vec::new();
    loop {
        let next = cursor.next_row().map_err(|e| e.to_string())?;
        let Some(mut row) = next else { break };
        let mut name = Vec::new();
        let mut dtype = Vec::new();
        let _ = row.get_text(1, &mut name);
        let _ = row.get_text(2, &mut dtype);
        out.push((
            String::from_utf8_lossy(&name).into_owned(),
            String::from_utf8_lossy(&dtype).into_owned().to_ascii_uppercase(),
        ));
    }
    Ok(out)
}

fn types_match(backend: &str, expected: &str, actual: &str) -> bool {
    let e = canon(backend, expected);
    let a = canon(backend, actual);
    e == a
}

fn canon(backend: &str, t: &str) -> String {
    let raw = t.trim().to_ascii_uppercase();
    let core = raw.split_once(' ').map(|(h, _)| h).unwrap_or(&raw);
    let stripped = core.split_once('(').map(|(h, _)| h).unwrap_or(core);
    match (backend, stripped) {
        ("sqlite", "INT") | ("sqlite", "TINYINT") | ("sqlite", "SMALLINT")
        | ("sqlite", "BIGINT") | ("sqlite", "BIT") => "INTEGER",
        ("sqlite", "VARCHAR") | ("sqlite", "CHAR") | ("sqlite", "NVARCHAR") => "TEXT",
        ("sqlite", "DATETIME") | ("sqlite", "DATE") => "TEXT",
        ("postgres", "DATETIME") => "TIMESTAMP",
        ("postgres", "VARCHAR") => "CHARACTER VARYING",
        ("postgres", "INT") | ("postgres", "INTEGER") => "INTEGER",
        ("postgres", "BIT") | ("postgres", "TINYINT") => "SMALLINT",
        _ => stripped,
    }
    .to_string()
}
