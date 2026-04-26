//! Data browser — read-only preview of rows from the configured backend
//! for a given table. Resolves the connection like the runtime does
//! (per-directory override falls back to global), opens via
//! `btr_import::sqlsrv`, and returns LIMIT N rows as stringified cells.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use btr_import::{schema as bschema, sqlsrv};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::state::{ApiError, AppState};

#[derive(Deserialize)]
pub struct BrowseParams {
    /// Default 50, max 1000.
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    /// Optional override of the source_dir section (e.g. "PACIFIC").
    /// If unset, the table's stored source_dir basename is used.
    pub section: Option<String>,
    /// Optional WHERE clause body — appended verbatim after `WHERE`.
    /// This is a developer/operator tool on localhost; the server does
    /// not sanitize the expression.
    #[serde(rename = "where")]
    pub where_clause: Option<String>,
}

#[derive(Serialize)]
pub struct BrowseResult {
    pub backend: String,
    pub table_ref: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

pub async fn rows(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<BrowseParams>,
) -> Result<Json<BrowseResult>, ApiError> {
    let db = state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no project open"))?;
    let limit = params.limit.unwrap_or(50).min(1000);
    let offset = params.offset.unwrap_or(0);

    let result = tokio::task::spawn_blocking(move || -> Result<BrowseResult, String> {
        let conn = bschema::open_db(&db)?;
        // schema for the table we're browsing
        let int_file = bschema::load_table(&conn, &name)?;
        let section = params
            .section
            .map(|s| s.to_ascii_uppercase())
            .or_else(|| section_from_source_dir(&int_file.source_dir));
        let cfg = resolve_config(&conn, section.as_deref())?;
        let backend = cfg.backend.clone();
        let mut sql = sqlsrv::connect(&cfg)?;
        let tref = sqlsrv::table_ref(&backend, &int_file);

        let columns: Vec<String> = int_file.fields.iter().map(|f| f.name.clone()).collect();
        let select_cols = if columns.is_empty() {
            "*".to_string()
        } else {
            columns
                .iter()
                .map(|c| quote_ident(&backend, c))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let where_part = params
            .where_clause
            .as_deref()
            .map(|w| w.trim())
            .filter(|w| !w.is_empty())
            .map(|w| format!(" WHERE {w}"))
            .unwrap_or_default();
        let sql_text = match backend.as_str() {
            "mssql" => format!(
                "SELECT TOP {} {} FROM {}{}",
                limit + offset,
                select_cols,
                tref,
                where_part,
            ),
            "postgres" | "sqlite" => format!(
                "SELECT {} FROM {}{} LIMIT {} OFFSET {}",
                select_cols, tref, where_part, limit, offset
            ),
            _ => format!("SELECT {} FROM {}{}", select_cols, tref, where_part),
        };

        let (rows, observed_cols) = run_query(&mut sql, &sql_text, columns.len())?;
        let columns = if columns.is_empty() {
            observed_cols
        } else {
            columns
        };
        let rows = if backend == "mssql" && offset > 0 && rows.len() > offset {
            rows.into_iter().skip(offset).collect()
        } else {
            rows
        };
        let truncated = rows.len() == limit;
        Ok(BrowseResult {
            backend,
            table_ref: tref,
            columns,
            rows,
            truncated,
        })
    })
    .await
    .map_err(|e| ApiError::internal(format!("join: {e}")))?
    .map_err(ApiError::bad_request)?;

    Ok(Json(result))
}

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

/// Build a SqlConfig from the global [config] section, then apply
/// per-directory overrides for `section` (uppercased dir name) on top.
/// Mirrors how the runtime resolves connection settings.
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

fn quote_ident(backend: &str, ident: &str) -> String {
    match backend {
        "mssql" => format!("[{}]", ident.replace(']', "]]")),
        "postgres" => format!("\"{}\"", ident.replace('"', "\"\"")),
        _ => format!("\"{}\"", ident.replace('"', "\"\"")),
    }
}

fn run_query(
    conn: &mut sqlsrv::SqlConnection,
    sql: &str,
    expected_cols: usize,
) -> Result<(Vec<Vec<Option<String>>>, Vec<String>), String> {
    match conn {
        sqlsrv::SqlConnection::Sqlite(c) => sqlite_query(c, sql),
        sqlsrv::SqlConnection::Postgres(c) => postgres_query(c, sql),
        sqlsrv::SqlConnection::Mssql(c) => mssql_query(c, sql, expected_cols),
    }
}

fn sqlite_query(
    c: &rusqlite::Connection,
    sql: &str,
) -> Result<(Vec<Vec<Option<String>>>, Vec<String>), String> {
    let mut stmt = c.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
    let cols: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    let n = cols.len();
    let mut out = Vec::new();
    let mut rows = stmt.query([]).map_err(|e| format!("query: {e}"))?;
    while let Some(row) = rows.next().map_err(|e| format!("row: {e}"))? {
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            let v: Option<String> = match row.get_ref(i) {
                Ok(rusqlite::types::ValueRef::Null) => None,
                Ok(rusqlite::types::ValueRef::Integer(i)) => Some(i.to_string()),
                Ok(rusqlite::types::ValueRef::Real(f)) => Some(format!("{f}")),
                Ok(rusqlite::types::ValueRef::Text(t)) => {
                    Some(String::from_utf8_lossy(t).into_owned())
                }
                Ok(rusqlite::types::ValueRef::Blob(b)) => Some(format!("<blob {} bytes>", b.len())),
                Err(_) => None,
            };
            vals.push(v);
        }
        out.push(vals);
    }
    Ok((out, cols))
}

fn postgres_query(
    c: &mut postgres::Client,
    sql: &str,
) -> Result<(Vec<Vec<Option<String>>>, Vec<String>), String> {
    let rows = c.query(sql, &[]).map_err(|e| format!("query: {e}"))?;
    let cols: Vec<String> = rows
        .first()
        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut vals = Vec::with_capacity(row.columns().len());
        for i in 0..row.columns().len() {
            let s: Option<String> = row
                .try_get::<_, Option<String>>(i)
                .ok()
                .flatten()
                .or_else(|| {
                    row.try_get::<_, Option<i64>>(i)
                        .ok()
                        .flatten()
                        .map(|n| n.to_string())
                })
                .or_else(|| {
                    row.try_get::<_, Option<f64>>(i)
                        .ok()
                        .flatten()
                        .map(|n| n.to_string())
                })
                .or_else(|| {
                    row.try_get::<_, Option<bool>>(i)
                        .ok()
                        .flatten()
                        .map(|b| b.to_string())
                });
            vals.push(s);
        }
        out.push(vals);
    }
    Ok((out, cols))
}

fn mssql_query(
    c: &mut odbc_api::Connection<'static>,
    sql: &str,
    expected_cols: usize,
) -> Result<(Vec<Vec<Option<String>>>, Vec<String>), String> {
    use odbc_api::{Cursor, ResultSetMetadata};

    let mut cursor = c
        .execute(sql, ())
        .map_err(|e| format!("execute: {e}"))?
        .ok_or_else(|| "no result set".to_string())?;
    let n = cursor.num_result_cols().map_err(|e| e.to_string())? as u16;
    let mut names = Vec::with_capacity(n as usize);
    for i in 1..=n {
        names.push(cursor.col_name(i).map_err(|e| e.to_string())?);
    }
    let cols = if names.is_empty() && expected_cols > 0 {
        (0..expected_cols).map(|i| format!("col{i}")).collect()
    } else {
        names
    };
    let mut out = Vec::new();
    loop {
        let next = cursor.next_row().map_err(|e| format!("next_row: {e}"))?;
        let Some(mut row) = next else { break };
        let mut vals = Vec::with_capacity(n as usize);
        for col in 1..=n {
            let mut v: Vec<u8> = Vec::new();
            let has = row.get_text(col, &mut v).unwrap_or(false);
            vals.push(if has {
                Some(String::from_utf8_lossy(&v).into_owned())
            } else {
                None
            });
        }
        out.push(vals);
    }
    Ok((out, cols))
}
