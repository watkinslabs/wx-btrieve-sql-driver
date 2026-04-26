//! Connection-settings endpoints. Reuses db-config's command bodies
//! (`do_show_config`, `do_set_connection`, `do_test_connection`) where
//! possible; here we want JSON in/out so we read the config table
//! directly via rusqlite and call the runtime's connect path for the
//! "test" verb.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::state::{ApiError, AppState};

// ── GET /api/config ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ConnectionConfig {
    pub backend: String,
    pub server: String,
    pub database: String,
    pub schema: String,
    pub driver: String,
    pub network: String,
    pub user: String,
    /// Always blank in the response — we never echo passwords. Use a
    /// separate "has_password" boolean for UI affordances.
    pub password: String,
    pub has_password: bool,
    pub trusted_connection: bool,
    pub encrypt: bool,
    pub trust_server_certificate: bool,
    pub recnum_column: String,
}

pub async fn get_config(State(state): State<AppState>) -> Result<Json<ConnectionConfig>, ApiError> {
    let conn = state.open()?;
    let get = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM config WHERE section = 'config' AND key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    let get_bool = |key: &str| -> bool {
        matches!(
            get(key).to_ascii_lowercase().as_str(),
            "yes" | "true" | "1"
        )
    };

    let pw = get("PASSWORD");
    Ok(Json(ConnectionConfig {
        backend: normalize_backend(&get("BACKEND")),
        server: get("SERVER"),
        database: get("DATABASE"),
        schema: get("SCHEMA"),
        driver: get("DRIVER"),
        network: get("NETWORK"),
        user: get("USER"),
        password: String::new(),
        has_password: !pw.is_empty(),
        trusted_connection: get_bool("TRUSTED_CONNECTION"),
        encrypt: get_bool("ENCRYPT"),
        trust_server_certificate: get_bool("TRUST_SERVER_CERTIFICATE"),
        recnum_column: get("RECNUM_COLUMN"),
    }))
}

fn normalize_backend(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => "postgres".into(),
        "sqlite" | "sqlite3" => "sqlite".into(),
        "" | "mssql" | "sqlserver" | "sql_server" | "ms_sql" => "mssql".into(),
        other => other.to_string(),
    }
}

// ── POST /api/config/backend  { backend, ... } ────────────────────────────
//
// Mirror of `db_config set-connection`. Optional fields update the
// matching config key; null/missing leaves the existing value alone.
// Body shape matches GET /api/config so the UI can round-trip it.

#[derive(Deserialize)]
pub struct UpdateConnection {
    pub backend: Option<String>,
    pub server: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub driver: Option<String>,
    pub network: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub trusted_connection: Option<bool>,
    pub encrypt: Option<bool>,
    pub trust_server_certificate: Option<bool>,
    pub recnum_column: Option<String>,
}

pub async fn set_backend(
    State(state): State<AppState>,
    Json(req): Json<UpdateConnection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db_path = state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no wxbtrv.db found"))?
        .clone();
    // Delegate to the same library function `db_config set-connection` runs.
    db_config::commands::manage::do_set_connection(
        &db_path,
        req.backend.as_deref(),
        req.server.as_deref(),
        req.database.as_deref(),
        req.schema.as_deref(),
        req.driver.as_deref(),
        req.network.as_deref(),
        req.user.as_deref(),
        req.password.as_deref(),
        req.trusted_connection,
        req.encrypt,
        req.trust_server_certificate,
        req.recnum_column.as_deref(),
    )
    .map_err(ApiError::bad_request)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ── POST /api/test-connection ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct TestConnectionResult {
    pub ok: bool,
    pub backend: String,
    pub message: String,
}

pub async fn test_connection(
    State(state): State<AppState>,
) -> Result<Json<TestConnectionResult>, ApiError> {
    let db_path = state
        .db_path()
        .ok_or_else(|| ApiError::not_found("no wxbtrv.db found"))?
        .clone();
    // db-config's do_test_connection prints to stdout and returns Result.
    // For the API we'd rather return structured info; capture the result
    // and leave the verbose breakdown for a future /api/test-connection
    // streaming endpoint.
    let backend = {
        let conn = state.open()?;
        let raw: String = conn
            .query_row(
                "SELECT value FROM config WHERE section = 'config' AND key = 'BACKEND'",
                rusqlite::params![],
                |r| r.get(0),
            )
            .unwrap_or_default();
        normalize_backend(&raw)
    };
    let result = db_config::commands::manage::do_test_connection(&db_path);
    Ok(Json(match result {
        Ok(()) => TestConnectionResult {
            ok: true,
            backend: backend.clone(),
            message: format!("{backend} connection OK"),
        },
        Err(e) => TestConnectionResult {
            ok: false,
            backend: backend.clone(),
            message: e,
        },
    }))
}
