/// sqlsrv.rs — SQL Server connection and bulk INSERT for btr-import.
use crate::schema::SqlConfig;
use btr_types::{codec::TYPE_LSTRING, sql_type, IntField, IntFile};
use odbc_api::{Connection, ConnectionOptions, Environment};
use std::sync::OnceLock;

pub type SqlConnection = Connection<'static>;

static ENV: OnceLock<Environment> = OnceLock::new();

fn env() -> &'static Environment {
    ENV.get_or_init(|| Environment::new().expect("ODBC driver manager not available"))
}

/// Build a connection string from config.
fn conn_str(cfg: &SqlConfig, driver: &str) -> String {
    format!(
        "Driver={{{driver}}};Server={};Database={};Uid={};Pwd={};Encrypt=No;TrustServerCertificate=Yes;",
        cfg.server, cfg.database, cfg.user, cfg.password
    )
}

/// Open an ODBC connection, trying multiple driver versions.
pub fn connect(cfg: &SqlConfig) -> Result<Connection<'static>, String> {
    let drivers = [
        "ODBC Driver 18 for SQL Server",
        "ODBC Driver 17 for SQL Server",
        "ODBC Driver 13 for SQL Server",
        "SQL Server Native Client 11.0",
        "SQL Server",
    ];

    let mut last_err = String::new();
    for drv in &drivers {
        let cs = conn_str(cfg, drv);
        match env().connect_with_connection_string(&cs, ConnectionOptions::default()) {
            Ok(c) => {
                eprintln!("Connected via {}", drv);
                return Ok(c);
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    Err(format!("cannot connect to SQL Server: {}", last_err))
}

/// Execute a DDL/DML statement (no result set expected).
pub fn execute(conn: &SqlConnection, sql: &str) -> Result<(), String> {
    conn.execute(sql, ())
        .map_err(|e| format!("SQL error: {}", e))?;
    Ok(())
}

// ── DDL generation ─────────────────────────────────────────────────────────────

fn col_def(f: &IntField, collation: &str) -> String {
    let t = sql_type(f.native_type, f.length);
    let type_str = match t {
        "VARCHAR" => {
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
        "VARBINARY" => format!("VARBINARY({})", f.length.max(1)),
        other => other.to_string(),
    };
    format!("    [{}] {} NULL", f.name, type_str)
}

/// Generate a CREATE TABLE IF NOT EXISTS statement.
///
/// `collation` is applied to all VARCHAR columns. Pass `""` to use the database default.
/// Common values: `"Latin1_General_CI_AS"`, `"Latin1_General_BIN"`,
/// `"Latin1_General_CS_AS"`, `"SQL_Latin1_General_CP1_CI_AS"`,
/// `"Latin1_General_100_CI_AS_SC_UTF8"`.
pub fn gen_create_table(schema: &IntFile, collation: &str) -> String {
    let table_ref = table_ref(schema);
    let cols: Vec<String> = schema
        .fields
        .iter()
        .map(|f| col_def(f, collation))
        .collect();
    format!(
        "IF NOT EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_NAME='{}')\n\
         CREATE TABLE {} (\n{}\n)",
        schema.table_name,
        table_ref,
        cols.join(",\n")
    )
}

/// Generate the qualified table reference.
/// Resolution: [db].[schema].[table] / [db].[table] / [schema].[table] / [table]
pub fn table_ref(schema: &IntFile) -> String {
    match (schema.db_name.is_empty(), schema.schema_name.is_empty()) {
        (true, true) => format!("[{}]", schema.table_name),
        (true, false) => format!("[{}].[{}]", schema.schema_name, schema.table_name),
        (false, true) => format!("[{}].[{}]", schema.db_name, schema.table_name),
        (false, false) => format!(
            "[{}].[{}].[{}]",
            schema.db_name, schema.schema_name, schema.table_name
        ),
    }
}

// ── Batch INSERT ───────────────────────────────────────────────────────────────

/// Insert a batch of decoded rows.
///
/// `rows` is a slice of decoded records — each row is a vec of (field_name, sql_literal).
/// Returns the number of rows inserted.
pub fn batch_insert(
    conn: &SqlConnection,
    schema: &IntFile,
    rows: &[Vec<(String, String)>],
    dry_run: bool,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Ok(0);
    }

    let tref = table_ref(schema);

    // Build column list from first row (field order is consistent)
    let col_names: Vec<&str> = rows[0].iter().map(|(n, _)| n.as_str()).collect();
    let col_list = col_names
        .iter()
        .map(|c| format!("[{}]", c))
        .collect::<Vec<_>>()
        .join(", ");

    // Build VALUES clause: one VALUES(...) row per record, joined with commas
    let mut sql = format!("INSERT INTO {} ({}) VALUES\n", tref, col_list);
    for (i, row) in rows.iter().enumerate() {
        let vals: Vec<&str> = row.iter().map(|(_, v)| v.as_str()).collect();
        sql.push_str(&format!("({})", vals.join(", ")));
        if i + 1 < rows.len() {
            sql.push_str(",\n");
        }
    }

    if dry_run {
        return Ok(rows.len());
    }

    conn.execute(&sql, ()).map_err(|e| {
        format!(
            "INSERT failed: {}\nSQL: {}...",
            e,
            &sql[..sql.len().min(400)]
        )
    })?;

    Ok(rows.len())
}
