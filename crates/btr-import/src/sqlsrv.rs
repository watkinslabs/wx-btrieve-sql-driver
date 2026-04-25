//! sqlsrv.rs — backend-agnostic connection + bulk INSERT for btr-import.
//!
//! Despite the name (legacy from when btr-import was MSSQL-only), this
//! module dispatches on `cfg.backend` to drive MSSQL via odbc-api,
//! Postgres via the postgres crate, or SQLite via rusqlite. Each path
//! emits the appropriate dialect's DDL and parameterized INSERT.
use crate::schema::SqlConfig;
use btr_types::{codec::TYPE_LSTRING, sql_type, IntField, IntFile};

pub enum SqlConnection {
    Mssql(odbc_api::Connection<'static>),
    Postgres(postgres::Client),
    Sqlite(rusqlite::Connection),
}

use std::sync::OnceLock;

static ODBC_ENV: OnceLock<odbc_api::Environment> = OnceLock::new();

fn odbc_env() -> &'static odbc_api::Environment {
    ODBC_ENV.get_or_init(|| odbc_api::Environment::new().expect("ODBC driver manager not available"))
}

/// Open a connection to the configured backend.
pub fn connect(cfg: &SqlConfig) -> Result<SqlConnection, String> {
    match cfg.backend.as_str() {
        "sqlite" => connect_sqlite(cfg),
        "postgres" => connect_postgres(cfg),
        _ => connect_mssql(cfg),
    }
}

fn connect_sqlite(cfg: &SqlConfig) -> Result<SqlConnection, String> {
    let path = if cfg.database.is_empty() {
        ":memory:".to_string()
    } else {
        cfg.database.clone()
    };
    rusqlite::Connection::open(&path)
        .map(SqlConnection::Sqlite)
        .map_err(|e| format!("SQLite open '{path}' failed: {e}"))
}

fn connect_postgres(cfg: &SqlConfig) -> Result<SqlConnection, String> {
    let (host, port) = match cfg.server.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h.to_string(), Some(p.to_string())),
        _ => (cfg.server.clone(), None),
    };
    let mut parts = Vec::new();
    if !host.is_empty() {
        parts.push(format!("host={host}"));
    }
    if let Some(p) = port {
        parts.push(format!("port={p}"));
    }
    if !cfg.user.is_empty() {
        parts.push(format!("user={}", cfg.user));
    }
    if !cfg.password.is_empty() {
        parts.push(format!("password={}", cfg.password));
    }
    if !cfg.database.is_empty() {
        parts.push(format!("dbname={}", cfg.database));
    }
    let cs = parts.join(" ");
    postgres::Client::connect(&cs, postgres::NoTls)
        .map(SqlConnection::Postgres)
        .map_err(|e| format!("Postgres connect failed: {e}"))
}

fn connect_mssql(cfg: &SqlConfig) -> Result<SqlConnection, String> {
    let drivers = [
        "ODBC Driver 18 for SQL Server",
        "ODBC Driver 17 for SQL Server",
        "ODBC Driver 13 for SQL Server",
        "SQL Server Native Client 11.0",
        "SQL Server",
    ];
    let mut last_err = String::new();
    for drv in &drivers {
        let cs = format!(
            "Driver={{{drv}}};Server={};Database={};Uid={};Pwd={};Encrypt=No;TrustServerCertificate=Yes;",
            cfg.server, cfg.database, cfg.user, cfg.password
        );
        match odbc_env().connect_with_connection_string(&cs, odbc_api::ConnectionOptions::default()) {
            Ok(c) => {
                eprintln!("Connected via {}", drv);
                return Ok(SqlConnection::Mssql(c));
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    Err(format!("cannot connect to SQL Server: {}", last_err))
}

/// Execute a non-query SQL statement.
pub fn execute(conn: &mut SqlConnection, sql: &str) -> Result<(), String> {
    match conn {
        SqlConnection::Mssql(c) => c
            .execute(sql, ())
            .map(|_| ())
            .map_err(|e| format!("MSSQL: {e}")),
        SqlConnection::Postgres(c) => c.batch_execute(sql).map_err(|e| {
            let detail = e
                .as_db_error()
                .map(|d| format!("{}: {}", d.code().code(), d.message()))
                .unwrap_or_else(|| format!("{e:?}"));
            format!("Postgres: {detail}")
        }),
        SqlConnection::Sqlite(c) => c
            .execute_batch(sql)
            .map_err(|e| format!("SQLite: {e}")),
    }
}

// ── DDL generation ─────────────────────────────────────────────────────────────

fn quote_ident(backend: &str, name: &str) -> String {
    match backend {
        "mssql" => format!("[{}]", name.replace(']', "]]")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

fn col_def(backend: &str, f: &IntField, collation: &str) -> String {
    let t = sql_type(f.native_type, f.length);
    let ident = quote_ident(backend, &f.name);
    let type_str = match (backend, t) {
        ("mssql", "VARCHAR") => {
            let max_len = match f.native_type {
                n if n == TYPE_LSTRING => f.length.saturating_sub(1),
                _ => f.length,
            };
            if collation.is_empty() {
                format!("VARCHAR({})", max_len.max(1))
            } else {
                format!("VARCHAR({}) COLLATE {}", max_len.max(1), collation)
            }
        }
        ("mssql", "VARBINARY") => format!("VARBINARY({})", f.length.max(1)),
        ("postgres", "VARCHAR") => {
            let max_len = match f.native_type {
                n if n == TYPE_LSTRING => f.length.saturating_sub(1),
                _ => f.length,
            };
            format!("VARCHAR({})", max_len.max(1))
        }
        ("postgres", "VARBINARY") => "BYTEA".to_string(),
        ("postgres", "DATETIME") => "TIMESTAMP".to_string(),
        ("postgres", "BIT") => "SMALLINT".to_string(),
        ("postgres", "TINYINT") => "SMALLINT".to_string(),
        // SQLite is type-affinity, not strict — accept the source type.
        ("sqlite", "VARCHAR") | ("sqlite", "CHAR") | ("sqlite", "NVARCHAR") => "TEXT".to_string(),
        ("sqlite", "VARBINARY") => "BLOB".to_string(),
        ("sqlite", "DATETIME") | ("sqlite", "DATE") => "TEXT".to_string(),
        ("sqlite", "BIT") | ("sqlite", "TINYINT") | ("sqlite", "SMALLINT") | ("sqlite", "INT")
        | ("sqlite", "BIGINT") => "INTEGER".to_string(),
        (_, other) => other.to_string(),
    };
    format!("    {} {} NULL", ident, type_str)
}

/// Generate a CREATE TABLE IF NOT EXISTS statement (or its MSSQL
/// equivalent OBJECT_ID guard).
pub fn gen_create_table(backend: &str, schema: &IntFile, collation: &str) -> String {
    let table_ref = table_ref(backend, schema);
    let cols: Vec<String> = schema
        .fields
        .iter()
        .map(|f| col_def(backend, f, collation))
        .collect();
    let body = format!("{} (\n{}\n)", table_ref, cols.join(",\n"));
    match backend {
        "mssql" => format!(
            "IF NOT EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_NAME='{}')\n\
             CREATE TABLE {body}",
            schema.table_name.replace('\'', "''")
        ),
        _ => format!("CREATE TABLE IF NOT EXISTS {body}"),
    }
}

/// Generate the qualified table reference per backend.
/// MSSQL: [db].[schema].[table] / [db].[table] / [schema].[table] / [table]
/// Postgres: schema.table / table (no cross-db references)
/// SQLite: bare table name (no namespaces)
pub fn table_ref(backend: &str, schema: &IntFile) -> String {
    let t = quote_ident(backend, &schema.table_name);
    if backend == "sqlite" {
        return t;
    }
    let allow_db = backend != "postgres";
    let db = if allow_db { schema.db_name.as_str() } else { "" };
    let sc = schema.schema_name.as_str();
    match (db.is_empty(), sc.is_empty()) {
        (true, true) => t,
        (true, false) => format!("{}.{}", quote_ident(backend, sc), t),
        (false, true) => format!("{}.{}", quote_ident(backend, db), t),
        (false, false) => format!(
            "{}.{}.{}",
            quote_ident(backend, db),
            quote_ident(backend, sc),
            t
        ),
    }
}

// ── Batch INSERT ───────────────────────────────────────────────────────────────

/// Insert a batch of decoded rows. `rows` is a slice of decoded records —
/// each row is a Vec<(field_name, sql_literal)>. The literal form is
/// historic; we re-render it to the active backend's parameterized form
/// before sending.
///
/// MSSQL still gets the multi-row VALUES form (legacy speed). Postgres
/// and SQLite use prepared parameterized INSERTs since they require
/// strict type matching that defeats string interpolation.
pub fn batch_insert(
    conn: &mut SqlConnection,
    schema: &IntFile,
    rows: &[Vec<(String, String)>],
    dry_run: bool,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Ok(0);
    }
    if dry_run {
        return Ok(rows.len());
    }
    match conn {
        SqlConnection::Mssql(_) => batch_insert_mssql(conn, schema, rows),
        SqlConnection::Postgres(_) => batch_insert_postgres(conn, schema, rows),
        SqlConnection::Sqlite(_) => batch_insert_sqlite(conn, schema, rows),
    }
}

fn batch_insert_mssql(
    conn: &mut SqlConnection,
    schema: &IntFile,
    rows: &[Vec<(String, String)>],
) -> Result<usize, String> {
    let tref = table_ref("mssql", schema);
    let col_list = rows[0]
        .iter()
        .map(|(n, _)| format!("[{}]", n.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!("INSERT INTO {tref} ({col_list}) VALUES\n");
    for (i, row) in rows.iter().enumerate() {
        let vals: Vec<&str> = row.iter().map(|(_, v)| v.as_str()).collect();
        sql.push_str(&format!("({})", vals.join(", ")));
        if i + 1 < rows.len() {
            sql.push_str(",\n");
        }
    }
    execute(conn, &sql).map_err(|e| {
        format!(
            "MSSQL INSERT failed: {e}\nSQL: {}...",
            &sql[..sql.len().min(400)]
        )
    })?;
    Ok(rows.len())
}

fn batch_insert_postgres(
    conn: &mut SqlConnection,
    schema: &IntFile,
    rows: &[Vec<(String, String)>],
) -> Result<usize, String> {
    let tref = table_ref("postgres", schema);
    let col_list = rows[0]
        .iter()
        .map(|(n, _)| format!("\"{}\"", n.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let n_cols = rows[0].len();
    let placeholders: String = (1..=n_cols)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("INSERT INTO {tref} ({col_list}) VALUES ({placeholders})");

    let SqlConnection::Postgres(c) = conn else {
        unreachable!()
    };
    let stmt = c
        .prepare(&sql)
        .map_err(|e| format!("Postgres prepare failed: {e}"))?;
    for row in rows {
        let owned: Vec<Box<dyn postgres::types::ToSql + Sync>> = row
            .iter()
            .zip(schema.fields.iter())
            .map(|((_, lit), f)| -> Box<dyn postgres::types::ToSql + Sync> {
                boxed_pg_value_from_literal(lit, f)
            })
            .collect();
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
            owned.iter().map(|b| b.as_ref()).collect();
        c.execute(&stmt, refs.as_slice()).map_err(|e| {
            let detail = e
                .as_db_error()
                .map(|d| format!("{}: {}", d.code().code(), d.message()))
                .unwrap_or_else(|| format!("{e:?}"));
            format!("Postgres INSERT failed: {detail}")
        })?;
    }
    Ok(rows.len())
}

fn batch_insert_sqlite(
    conn: &mut SqlConnection,
    schema: &IntFile,
    rows: &[Vec<(String, String)>],
) -> Result<usize, String> {
    let tref = table_ref("sqlite", schema);
    let col_list = rows[0]
        .iter()
        .map(|(n, _)| format!("\"{}\"", n.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let n_cols = rows[0].len();
    let placeholders: String = std::iter::repeat_n("?", n_cols)
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("INSERT INTO {tref} ({col_list}) VALUES ({placeholders})");

    let SqlConnection::Sqlite(c) = conn else {
        unreachable!()
    };
    let mut stmt = c.prepare(&sql).map_err(|e| format!("SQLite prepare failed: {e}"))?;
    for row in rows {
        let bound: Vec<rusqlite::types::Value> = row
            .iter()
            .zip(schema.fields.iter())
            .map(|((_, lit), f)| sqlite_value_from_literal(lit, f))
            .collect();
        stmt.execute(rusqlite::params_from_iter(bound.iter()))
            .map_err(|e| format!("SQLite INSERT failed: {e}"))?;
    }
    Ok(rows.len())
}

/// Convert a legacy SQL literal string back into a typed Postgres
/// parameter. Mirrors the type detection wxbtrv-core does in
/// unpack_row_typed but operates on the post-formatted string.
fn boxed_pg_value_from_literal(
    lit: &str,
    f: &IntField,
) -> Box<dyn postgres::types::ToSql + Sync> {
    if lit == "NULL" {
        return Box::new(Option::<i32>::None);
    }
    // 1=INT, 14/15=AUTOINC, 6=MONEY/DECIMAL — int-ish
    match f.native_type {
        1 | 5 | 14 | 15 => {
            let n: i64 = lit.trim().parse().unwrap_or(0);
            Box::new(n)
        }
        2 => {
            let v: f64 = lit.trim().parse().unwrap_or(0.0);
            Box::new(v)
        }
        7 => {
            let n: i16 = lit.trim().parse().unwrap_or(0);
            Box::new(n)
        }
        3 | 4 => {
            // dates/times come pre-quoted: 'YYYY-MM-DD' / 'HH:MM:SS'
            let s = lit.trim().trim_start_matches('\'').trim_end_matches('\'').to_string();
            Box::new(s)
        }
        _ => {
            // STRING / ZSTRING etc. — pre-quoted text
            let s = lit
                .trim()
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .map(|s| s.replace("''", "'"))
                .unwrap_or_else(|| lit.to_string());
            Box::new(s)
        }
    }
}

fn sqlite_value_from_literal(lit: &str, f: &IntField) -> rusqlite::types::Value {
    use rusqlite::types::Value;
    if lit == "NULL" {
        return Value::Null;
    }
    match f.native_type {
        1 | 5 | 14 | 15 => Value::Integer(lit.trim().parse().unwrap_or(0)),
        2 => Value::Real(lit.trim().parse().unwrap_or(0.0)),
        7 => Value::Integer(lit.trim().parse().unwrap_or(0)),
        _ => {
            let s = lit
                .trim()
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .map(|s| s.replace("''", "'"))
                .unwrap_or_else(|| lit.to_string());
            Value::Text(s)
        }
    }
}
