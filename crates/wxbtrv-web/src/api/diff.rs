//! Schema diff — compare a table's project-side definition (loaded from
//! wxbtrv.db) against the actual columns on the configured backend.
//! Useful as a migration-readiness signal: matches mean the runtime can
//! talk to the table; missing/extra/mistyped columns mean someone has
//! to fix one side or the other.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use btr_import::{schema as bschema, sqlsrv};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::state::{ApiError, AppState};

#[derive(Deserialize)]
pub struct DiffParams {
    /// Optional override of the section name (e.g. "PACIFIC"). Defaults
    /// to the basename of the table's stored source_dir.
    pub section: Option<String>,
}

#[derive(Serialize)]
pub struct ProjectColumn {
    pub name: String,
    pub native_type: i32,
    pub expected_sql_type: String,
}

#[derive(Serialize)]
pub struct BackendColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Serialize)]
pub struct TypeMismatch {
    pub name: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Serialize)]
pub struct DiffResult {
    pub backend: String,
    pub table_ref: String,
    pub project_columns: Vec<ProjectColumn>,
    pub backend_columns: Vec<BackendColumn>,
    pub matched: Vec<String>,
    pub missing_in_backend: Vec<String>,
    pub extra_in_backend: Vec<String>,
    pub type_mismatches: Vec<TypeMismatch>,
    pub backend_table_exists: bool,
}

pub async fn diff(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<DiffParams>,
) -> Result<Json<DiffResult>, ApiError> {
    let db = state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no project open"))?;

    let result = tokio::task::spawn_blocking(move || -> Result<DiffResult, String> {
        let conn = bschema::open_db(&db)?;
        let int_file = bschema::load_table(&conn, &name)?;
        let section = params
            .section
            .map(|s| s.to_ascii_uppercase())
            .or_else(|| section_from_source_dir(&int_file.source_dir));
        let cfg = resolve_config(&conn, section.as_deref())?;
        let backend = cfg.backend.clone();
        let mut sql = sqlsrv::connect(&cfg)?;

        let project_columns: Vec<ProjectColumn> = int_file
            .fields
            .iter()
            .map(|f| ProjectColumn {
                name: f.name.clone(),
                native_type: f.native_type,
                expected_sql_type: sqlsrv::sql_type_str(&backend, f, ""),
            })
            .collect();

        let backend_columns =
            fetch_backend_columns(&mut sql, &backend, &int_file).unwrap_or_default();
        let backend_table_exists = !backend_columns.is_empty();

        let mut matched = Vec::new();
        let mut missing = Vec::new();
        let mut extra = Vec::new();
        let mut mismatches = Vec::new();

        let backend_by_name: HashMap<String, &BackendColumn> = backend_columns
            .iter()
            .map(|c| (c.name.to_ascii_uppercase(), c))
            .collect();
        let project_by_name: HashMap<String, &ProjectColumn> = project_columns
            .iter()
            .map(|c| (c.name.to_ascii_uppercase(), c))
            .collect();

        for pc in &project_columns {
            let key = pc.name.to_ascii_uppercase();
            match backend_by_name.get(&key) {
                None => {
                    if backend_table_exists {
                        missing.push(pc.name.clone());
                    }
                }
                Some(bc) => {
                    if types_match(&backend, &pc.expected_sql_type, &bc.data_type) {
                        matched.push(pc.name.clone());
                    } else {
                        mismatches.push(TypeMismatch {
                            name: pc.name.clone(),
                            expected: pc.expected_sql_type.clone(),
                            actual: bc.data_type.clone(),
                        });
                    }
                }
            }
        }
        for bc in &backend_columns {
            let key = bc.name.to_ascii_uppercase();
            if !project_by_name.contains_key(&key) {
                extra.push(bc.name.clone());
            }
        }

        Ok(DiffResult {
            backend: backend.clone(),
            table_ref: sqlsrv::table_ref(&backend, &int_file),
            project_columns,
            backend_columns,
            matched,
            missing_in_backend: missing,
            extra_in_backend: extra,
            type_mismatches: mismatches,
            backend_table_exists,
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

fn fetch_backend_columns(
    conn: &mut sqlsrv::SqlConnection,
    backend: &str,
    int_file: &btr_types::IntFile,
) -> Result<Vec<BackendColumn>, String> {
    match (backend, conn) {
        ("sqlite", sqlsrv::SqlConnection::Sqlite(c)) => sqlite_columns(c, &int_file.table_name),
        ("postgres", sqlsrv::SqlConnection::Postgres(c)) => postgres_columns(
            c,
            &int_file.table_name,
            if int_file.schema_name.is_empty() {
                "public"
            } else {
                &int_file.schema_name
            },
        ),
        ("mssql", sqlsrv::SqlConnection::Mssql(c)) => mssql_columns(c, int_file),
        _ => Err("backend / connection mismatch".into()),
    }
}

fn sqlite_columns(c: &rusqlite::Connection, table: &str) -> Result<Vec<BackendColumn>, String> {
    let sql = format!("PRAGMA table_info({})", quote_sqlite(table));
    let mut stmt = c.prepare(&sql).map_err(|e| format!("prepare: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(BackendColumn {
                name: r.get::<_, String>(1)?,
                data_type: r.get::<_, String>(2)?,
                nullable: r.get::<_, i64>(3)? == 0,
            })
        })
        .map_err(|e| format!("query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("row: {e}"))
}

fn quote_sqlite(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn postgres_columns(
    c: &mut postgres::Client,
    table: &str,
    schema: &str,
) -> Result<Vec<BackendColumn>, String> {
    let rows = c
        .query(
            "SELECT column_name, data_type, is_nullable
             FROM information_schema.columns
             WHERE lower(table_name) = lower($1) AND lower(table_schema) = lower($2)
             ORDER BY ordinal_position",
            &[&table, &schema],
        )
        .map_err(|e| format!("query: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|row| BackendColumn {
            name: row.get::<_, String>(0),
            data_type: row.get::<_, String>(1).to_ascii_uppercase(),
            nullable: row.get::<_, String>(2).eq_ignore_ascii_case("YES"),
        })
        .collect())
}

fn mssql_columns(
    c: &mut odbc_api::Connection<'static>,
    int_file: &btr_types::IntFile,
) -> Result<Vec<BackendColumn>, String> {
    use odbc_api::{Cursor, ResultSetMetadata};
    let mut sql = String::from(
        "SELECT COLUMN_NAME, DATA_TYPE, CHARACTER_MAXIMUM_LENGTH, IS_NULLABLE \
         FROM INFORMATION_SCHEMA.COLUMNS WHERE LOWER(TABLE_NAME) = LOWER(?)",
    );
    if !int_file.schema_name.is_empty() {
        sql.push_str(" AND LOWER(TABLE_SCHEMA) = LOWER(?)");
    }
    if !int_file.db_name.is_empty() {
        sql.push_str(" AND LOWER(TABLE_CATALOG) = LOWER(?)");
    }
    sql.push_str(" ORDER BY ORDINAL_POSITION");

    use odbc_api::IntoParameter;
    let mut params: Vec<Box<dyn odbc_api::parameter::InputParameter>> =
        vec![Box::new(int_file.table_name.clone().into_parameter())];
    if !int_file.schema_name.is_empty() {
        params.push(Box::new(int_file.schema_name.clone().into_parameter()));
    }
    if !int_file.db_name.is_empty() {
        params.push(Box::new(int_file.db_name.clone().into_parameter()));
    }

    let mut cursor = c
        .execute(&sql, params.as_slice())
        .map_err(|e| format!("execute: {e}"))?
        .ok_or_else(|| "no result set".to_string())?;
    let n = cursor.num_result_cols().map_err(|e| e.to_string())? as u16;
    let mut out = Vec::new();
    loop {
        let next = cursor.next_row().map_err(|e| format!("next_row: {e}"))?;
        let Some(mut row) = next else { break };
        let get = |row: &mut odbc_api::CursorRow<'_>, col: u16| -> Option<String> {
            let mut v: Vec<u8> = Vec::new();
            if row.get_text(col, &mut v).unwrap_or(false) {
                Some(String::from_utf8_lossy(&v).into_owned())
            } else {
                None
            }
        };
        let name = get(&mut row, 1).unwrap_or_default();
        let dtype = get(&mut row, 2).unwrap_or_default().to_ascii_uppercase();
        let max_len = get(&mut row, 3);
        let nullable = get(&mut row, 4)
            .map(|s| s.eq_ignore_ascii_case("YES"))
            .unwrap_or(true);
        let dtype = match (dtype.as_str(), max_len.as_deref()) {
            ("VARCHAR" | "NVARCHAR" | "CHAR" | "NCHAR", Some(len)) if !len.is_empty() => {
                format!("{}({})", dtype, len)
            }
            _ => dtype,
        };
        // Skip the n-result-cols dance below: we already pulled all 4 cells.
        let _ = n;
        out.push(BackendColumn {
            name,
            data_type: dtype,
            nullable,
        });
    }
    Ok(out)
}

/// Loose type comparison — different backends report types in
/// different shapes, so we normalize before matching.
fn types_match(backend: &str, expected: &str, actual: &str) -> bool {
    let e = normalize_type(backend, expected);
    let a = normalize_type(backend, actual);
    e == a
}

fn normalize_type(backend: &str, t: &str) -> String {
    let raw = t.trim().to_ascii_uppercase();
    let core = raw.split_once(' ').map(|(h, _)| h).unwrap_or(&raw);
    let stripped = core.split_once('(').map(|(h, _)| h).unwrap_or(core);
    let canon = match (backend, stripped) {
        ("sqlite", "INT") | ("sqlite", "TINYINT") | ("sqlite", "SMALLINT")
        | ("sqlite", "BIGINT") | ("sqlite", "BIT") => "INTEGER",
        ("sqlite", "VARCHAR") | ("sqlite", "CHAR") | ("sqlite", "NVARCHAR") => "TEXT",
        ("sqlite", "DATETIME") | ("sqlite", "DATE") => "TEXT",
        ("postgres", "DATETIME") => "TIMESTAMP",
        ("postgres", "VARCHAR") => "CHARACTER VARYING",
        ("postgres", "INT") | ("postgres", "INTEGER") => "INTEGER",
        ("postgres", "BIT") | ("postgres", "TINYINT") => "SMALLINT",
        _ => stripped,
    };
    // For backends that report length, drop length specifiers when
    // checking equality so `VARCHAR(32)` == `VARCHAR(32)` works but
    // also `VARCHAR(32)` ~= `VARCHAR` doesn't false-positive.
    let mut keep_len = matches!(
        (backend, canon),
        ("mssql", "VARCHAR") | ("mssql", "CHAR") | ("mssql", "NVARCHAR") | ("mssql", "NCHAR")
    );
    // If the original had a length, keep it on supported types.
    if keep_len {
        if let Some(open) = raw.find('(') {
            return format!("{}{}", canon, &raw[open..raw.find(')').unwrap_or(raw.len()) + 1]);
        } else {
            keep_len = false;
        }
    }
    if !keep_len {
        canon.to_string()
    } else {
        canon.to_string()
    }
}
