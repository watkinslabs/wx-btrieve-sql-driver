//! Connections endpoints — Global defaults plus per-database overrides.
//!
//! Storage: `config` table.
//!   section = 'config'         — global defaults
//!   section = '<DIRNAME>'      — per-directory overrides
//!
//! Resolution at runtime: the runtime looks up overrides for the
//! opened-file's directory; missing keys fall back to global. The UI
//! exposes both layers and a "resolved" view that shows the effective
//! values for any given dirname.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::state::{ApiError, AppState};

const KEYS: &[&str] = &[
    "BACKEND",
    "SERVER",
    "DATABASE",
    "SCHEMA",
    "DRIVER",
    "NETWORK",
    "USER",
    "PASSWORD",
    "TRUSTED_CONNECTION",
    "ENCRYPT",
    "TRUST_SERVER_CERTIFICATE",
    "RECNUM_COLUMN",
];

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionFields {
    pub backend: Option<String>,
    pub server: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub driver: Option<String>,
    pub network: Option<String>,
    pub user: Option<String>,
    /// Always blank in responses. UI sends a non-null value to set/replace,
    /// or `""` to clear; missing/null leaves the existing value alone.
    pub password: Option<String>,
    pub has_password: Option<bool>,
    pub trusted_connection: Option<bool>,
    pub encrypt: Option<bool>,
    pub trust_server_certificate: Option<bool>,
    pub recnum_column: Option<String>,
}

#[derive(Serialize)]
pub struct ConnectionEntry {
    /// "global" for the [config] section, otherwise the directory name
    /// (uppercased — same as wxbtrv-core's lookup).
    pub name: String,
    pub is_global: bool,
    pub fields: ConnectionFields,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ConnectionEntry>>, ApiError> {
    let conn = state.open()?;
    let global = read_section(&conn, "config")?;
    let mut out = vec![ConnectionEntry {
        name: "global".to_string(),
        is_global: true,
        fields: redact(&global),
    }];

    let mut stmt =
        conn.prepare("SELECT DISTINCT section FROM config WHERE section != 'config' ORDER BY section")?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    for name in names {
        let raw = read_section(&conn, &name)?;
        out.push(ConnectionEntry {
            name,
            is_global: false,
            fields: redact(&raw),
        });
    }
    Ok(Json(out))
}

pub async fn get_one(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ConnectionEntry>, ApiError> {
    let conn = state.open()?;
    let section = section_for(&name);
    let raw = read_section(&conn, &section)?;
    Ok(Json(ConnectionEntry {
        name: name.clone(),
        is_global: name == "global",
        fields: redact(&raw),
    }))
}

#[derive(Serialize)]
pub struct ResolvedConnection {
    pub name: String,
    pub fields: ConnectionFields,
}

/// Resolved view: per-directory overrides inherit unset values from
/// global. Uses the same lookup the runtime performs.
pub async fn get_resolved(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ResolvedConnection>, ApiError> {
    let conn = state.open()?;
    let global = read_section(&conn, "config")?;
    let merged = if name == "global" {
        global
    } else {
        let mut g = global;
        let over = read_section(&conn, &name.to_ascii_uppercase())?;
        for (k, v) in over {
            if !v.is_empty() {
                g.insert(k, v);
            }
        }
        g
    };
    Ok(Json(ResolvedConnection {
        name,
        fields: redact(&merged),
    }))
}

pub async fn upsert(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ConnectionFields>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let conn = state.open_rw()?;
    let section = section_for(&name);

    let pairs = fields_to_pairs(&req);
    for (k, v_opt) in pairs {
        match v_opt {
            None => continue, // missing → leave existing alone
            Some(v) => {
                if k == "PASSWORD" && v.is_empty() {
                    // Empty password explicitly clears.
                    let _ = conn.execute(
                        "DELETE FROM config WHERE section = ?1 AND key = 'PASSWORD'",
                        rusqlite::params![section],
                    );
                    continue;
                }
                conn.execute(
                    "INSERT INTO config (section, key, value) VALUES (?1, ?2, ?3)
                     ON CONFLICT(section, key) DO UPDATE SET value = excluded.value",
                    rusqlite::params![section, k, v],
                )?;
            }
        }
    }
    Ok(Json(serde_json::json!({"ok": true, "section": section})))
}

pub async fn delete_one(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if name == "global" {
        return Err(ApiError::bad_request("cannot delete global defaults"));
    }
    let conn = state.open_rw()?;
    let section = section_for(&name);
    let n = conn.execute(
        "DELETE FROM config WHERE section = ?1",
        rusqlite::params![section],
    )?;
    Ok(Json(serde_json::json!({"ok": true, "deleted_keys": n})))
}

// ── Test connection (with optional draft override) ────────────────────────

#[derive(Deserialize, Default)]
pub struct TestRequest {
    /// "global" or a directory name. Defaults to "global".
    pub name: Option<String>,
    /// Draft override applied on top of the resolved config. Lets the UI
    /// "Test" before saving.
    pub draft: Option<ConnectionFields>,
}

#[derive(Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub backend: String,
    pub message: String,
}

pub async fn test(
    State(state): State<AppState>,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResult>, ApiError> {
    let conn = state.open()?;
    let name = req.name.unwrap_or_else(|| "global".to_string());
    let mut effective = if name == "global" {
        read_section(&conn, "config")?
    } else {
        let mut g = read_section(&conn, "config")?;
        for (k, v) in read_section(&conn, &name.to_ascii_uppercase())? {
            if !v.is_empty() {
                g.insert(k, v);
            }
        }
        g
    };
    if let Some(draft) = req.draft {
        for (k, v_opt) in fields_to_pairs(&draft) {
            if let Some(v) = v_opt {
                if !v.is_empty() {
                    effective.insert(k.to_string(), v);
                }
            }
        }
    }

    // Hand off to the same connect-and-SELECT-1 logic db_config uses,
    // via a dispatcher that branches on BACKEND.
    let result = tokio::task::spawn_blocking(move || run_test(effective))
        .await
        .map_err(|e| ApiError::internal(format!("join: {e}")))?;
    Ok(Json(result))
}

fn run_test(cfg: std::collections::HashMap<String, String>) -> TestResult {
    let backend = normalize_backend(cfg.get("BACKEND").map(String::as_str).unwrap_or(""));
    let res = match backend.as_str() {
        "sqlite" => test_sqlite(&cfg),
        "postgres" => test_postgres(&cfg),
        _ => test_mssql(&cfg),
    };
    match res {
        Ok(()) => TestResult {
            ok: true,
            backend: backend.clone(),
            message: format!("{backend} connection OK"),
        },
        Err(e) => TestResult {
            ok: false,
            backend,
            message: e,
        },
    }
}

fn test_sqlite(cfg: &std::collections::HashMap<String, String>) -> Result<(), String> {
    let path = cfg.get("DATABASE").cloned().unwrap_or_default();
    if path.is_empty() {
        return Err("DATABASE (path to .sqlite file) not set".into());
    }
    let c = rusqlite::Connection::open(&path).map_err(|e| format!("open: {e}"))?;
    let _: i64 = c
        .query_row("SELECT 1", [], |r| r.get(0))
        .map_err(|e| format!("query: {e}"))?;
    Ok(())
}

fn test_postgres(cfg: &std::collections::HashMap<String, String>) -> Result<(), String> {
    let server = cfg.get("SERVER").cloned().unwrap_or_default();
    let database = cfg.get("DATABASE").cloned().unwrap_or_default();
    let user = cfg.get("USER").cloned().unwrap_or_default();
    let password = cfg.get("PASSWORD").cloned().unwrap_or_default();
    let (host, port) = match server.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), Some(p.to_string()))
        }
        _ => (server, None),
    };
    let mut parts = vec![format!("host={host}")];
    if let Some(p) = port {
        parts.push(format!("port={p}"));
    }
    if !user.is_empty() {
        parts.push(format!("user={user}"));
    }
    if !password.is_empty() {
        parts.push(format!("password={password}"));
    }
    if !database.is_empty() {
        parts.push(format!("dbname={database}"));
    }
    let cs = parts.join(" ");
    let mut c = postgres::Client::connect(&cs, postgres::NoTls)
        .map_err(|e| format!("connect: {e}"))?;
    c.query_one("SELECT 1", &[]).map_err(|e| format!("query: {e}"))?;
    Ok(())
}

fn test_mssql(cfg: &std::collections::HashMap<String, String>) -> Result<(), String> {
    let server = cfg.get("SERVER").cloned().unwrap_or_default();
    let database = cfg.get("DATABASE").cloned().unwrap_or_default();
    let driver = cfg.get("DRIVER").cloned().unwrap_or_else(|| "ODBC Driver 17 for SQL Server".into());
    let user = cfg.get("USER").cloned().unwrap_or_default();
    let password = cfg.get("PASSWORD").cloned().unwrap_or_default();
    let trusted = bool_val(cfg.get("TRUSTED_CONNECTION"));
    let encrypt = bool_val(cfg.get("ENCRYPT"));
    let trust_cert = bool_val(cfg.get("TRUST_SERVER_CERTIFICATE"));

    let mut cs = format!("Driver={{{driver}}};Server={server};");
    if !database.is_empty() {
        cs.push_str(&format!("Database={database};"));
    }
    if trusted {
        cs.push_str("Trusted_Connection=Yes;");
    } else if !user.is_empty() {
        cs.push_str(&format!("Uid={user};Pwd={password};"));
    } else {
        cs.push_str("Trusted_Connection=No;");
    }
    cs.push_str(if encrypt { "Encrypt=Yes;" } else { "Encrypt=No;" });
    cs.push_str(if trust_cert {
        "TrustServerCertificate=Yes;"
    } else {
        "TrustServerCertificate=No;"
    });

    let env = odbc_api::Environment::new().map_err(|e| format!("ODBC env: {e}"))?;
    env.connect_with_connection_string(&cs, odbc_api::ConnectionOptions::default())
        .map_err(|e| format!("connect: {e}"))?;
    Ok(())
}

fn bool_val(v: Option<&String>) -> bool {
    matches!(
        v.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("yes") | Some("true") | Some("1")
    )
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn section_for(name: &str) -> String {
    if name == "global" {
        "config".to_string()
    } else {
        name.to_ascii_uppercase()
    }
}

fn read_section(
    conn: &rusqlite::Connection,
    section: &str,
) -> Result<std::collections::HashMap<String, String>, ApiError> {
    let mut out = std::collections::HashMap::new();
    let mut stmt = conn.prepare("SELECT key, value FROM config WHERE section = ?1")?;
    let rows = stmt.query_map(rusqlite::params![section], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (k, v) = row?;
        out.insert(k.to_ascii_uppercase(), v);
    }
    let _ = KEYS; // silence unused-const warning if we ever drop the iteration
    Ok(out)
}

fn redact(raw: &std::collections::HashMap<String, String>) -> ConnectionFields {
    let pw = raw.get("PASSWORD").cloned().unwrap_or_default();
    ConnectionFields {
        backend: raw.get("BACKEND").cloned(),
        server: raw.get("SERVER").cloned(),
        database: raw.get("DATABASE").cloned(),
        schema: raw.get("SCHEMA").cloned(),
        driver: raw.get("DRIVER").cloned(),
        network: raw.get("NETWORK").cloned(),
        user: raw.get("USER").cloned(),
        password: None, // never echo
        has_password: Some(!pw.is_empty()),
        trusted_connection: raw.get("TRUSTED_CONNECTION").map(|v| bool_val(Some(v))),
        encrypt: raw.get("ENCRYPT").map(|v| bool_val(Some(v))),
        trust_server_certificate: raw.get("TRUST_SERVER_CERTIFICATE").map(|v| bool_val(Some(v))),
        recnum_column: raw.get("RECNUM_COLUMN").cloned(),
    }
}

fn fields_to_pairs(f: &ConnectionFields) -> Vec<(&'static str, Option<String>)> {
    fn b(v: Option<bool>) -> Option<String> {
        v.map(|b| if b { "yes".to_string() } else { "no".to_string() })
    }
    vec![
        ("BACKEND", f.backend.clone()),
        ("SERVER", f.server.clone()),
        ("DATABASE", f.database.clone()),
        ("SCHEMA", f.schema.clone()),
        ("DRIVER", f.driver.clone()),
        ("NETWORK", f.network.clone()),
        ("USER", f.user.clone()),
        ("PASSWORD", f.password.clone()),
        ("TRUSTED_CONNECTION", b(f.trusted_connection)),
        ("ENCRYPT", b(f.encrypt)),
        ("TRUST_SERVER_CERTIFICATE", b(f.trust_server_certificate)),
        ("RECNUM_COLUMN", f.recnum_column.clone()),
    ]
}

fn normalize_backend(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => "postgres".into(),
        "sqlite" | "sqlite3" => "sqlite".into(),
        "" | "mssql" | "sqlserver" | "sql_server" | "ms_sql" => "mssql".into(),
        other => other.to_string(),
    }
}
