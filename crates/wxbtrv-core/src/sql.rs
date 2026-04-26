use crate::constants::{BTR_RECORD_LOCKED, ERR_CONTEXT_FAILURE, ERR_CONTEXT_SETUP, ERR_NOT_LOADED};
use crate::sql_param::SqlValue;
use crate::state::{set_err, state, Backend};
use crate::trace::{get_seq, trace};

macro_rules! strace {
    ($($arg:tt)*) => {
        trace(&format!("#{}   {}", get_seq(), format!($($arg)*)))
    };
}

use odbc_api::{Connection as OdbcConn, ConnectionOptions, Cursor, Environment, ResultSetMetadata};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Global flag: there is an active transaction on the shared connection.
/// Set by begin_txn(), cleared by commit_txn()/abort_txn().
pub static TXN_ACTIVE: AtomicBool = AtomicBool::new(false);

// ── Backend-flexible connection holder ───────────────────────────────────────
//
// Each variant owns the native driver's connection type for that backend.
// One connection per process, behind a Mutex — Btrieve is fundamentally
// single-threaded per position-block, so serializing is fine.

// Variants vary widely in size (postgres::Client is much larger than the
// other two); allow the clippy hit since this enum lives behind a
// process-global Mutex and is allocated at most once.
#[allow(clippy::large_enum_variant)]
enum SqlConn {
    Mssql(OdbcConn<'static>),
    Postgres(postgres::Client),
    Sqlite(rusqlite::Connection),
}

static ODBC_ENV: OnceLock<Environment> = OnceLock::new();
static CONN: OnceLock<Mutex<Option<SqlConn>>> = OnceLock::new();

fn odbc_env() -> Result<&'static Environment, i32> {
    // Lazy-init the ODBC environment. Don't panic if the driver manager is
    // missing — we run inside NTVDM and a panic would crash the DOS app.
    if ODBC_ENV.get().is_none() {
        match Environment::new() {
            Ok(env) => {
                let _ = ODBC_ENV.set(env);
            }
            Err(e) => {
                trace(&format!(
                    "odbc_env: Environment::new() failed: {e:?} — ODBC driver manager unavailable"
                ));
                return Err(set_err(ERR_NOT_LOADED));
            }
        }
    }
    ODBC_ENV.get().ok_or_else(|| set_err(ERR_NOT_LOADED))
}

fn conn_cell() -> &'static Mutex<Option<SqlConn>> {
    CONN.get_or_init(|| Mutex::new(None))
}

/// Drop the cached connection so the next call re-connects. Called when
/// server/database/credentials change (e.g. MDS.INI reload, op_stop).
pub fn reset_connection() {
    if let Ok(mut g) = conn_cell().lock() {
        *g = None;
    }
}

fn redact_password(cs: &str) -> String {
    let mut out = String::with_capacity(cs.len());
    let lower = cs.to_ascii_lowercase();
    let mut i = 0;
    while i < cs.len() {
        if lower[i..].starts_with("pwd=") || lower[i..].starts_with("password=") {
            let key_len = if lower[i..].starts_with("pwd=") { 4 } else { 9 };
            out.push_str(&cs[i..i + key_len]);
            out.push_str("***;");
            i += key_len;
            while i < cs.len() && cs.as_bytes()[i] != b';' {
                i += 1;
            }
            if i < cs.len() {
                i += 1;
            }
        } else {
            out.push(cs.as_bytes()[i] as char);
            i += 1;
        }
    }
    out
}

// ── Connection-string builders ───────────────────────────────────────────────

fn build_mssql_conn_str(st: &crate::state::DriverState) -> String {
    let mut s = String::new();

    if !st.dsn.is_empty() {
        s.push_str(&format!("DSN={};", st.dsn));
    } else {
        s.push_str(&format!("Driver={{{}}};", st.driver));
        let srv = if st.server.is_empty() {
            "localhost"
        } else {
            &st.server
        };
        match st.network.to_ascii_uppercase().as_str() {
            "TCP:" | "TCP" => {
                let bare = srv.strip_prefix("tcp:").unwrap_or(srv);
                s.push_str(&format!("Server=tcp:{};", bare));
            }
            "DBMSSOCN" => {
                s.push_str(&format!("Server={};Network=DBMSSOCN;", srv));
            }
            _ => {
                s.push_str(&format!("Server={};", srv));
            }
        }
        if !st.database.is_empty() {
            s.push_str(&format!("Database={};", st.database));
        }
    }

    if st.trusted_connection {
        s.push_str("Trusted_Connection=Yes;");
    } else if !st.user.is_empty() {
        s.push_str(&format!("Uid={};Pwd={};", st.user, st.pass));
    } else {
        s.push_str("Trusted_Connection=No;");
    }
    s.push_str(if st.encrypt {
        "Encrypt=Yes;"
    } else {
        "Encrypt=No;"
    });
    s.push_str(if st.trust_server_certificate {
        "TrustServerCertificate=Yes;"
    } else {
        "TrustServerCertificate=No;"
    });
    s
}

fn build_postgres_conn_str(st: &crate::state::DriverState) -> String {
    // postgres crate accepts libpq-style key=value strings.
    let mut parts: Vec<String> = Vec::new();
    let (host, port) = match st.server.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h.to_string(), Some(p.to_string()))
        }
        _ => (st.server.clone(), None),
    };
    if !host.is_empty() {
        parts.push(format!("host={}", host));
    }
    if let Some(p) = port {
        parts.push(format!("port={}", p));
    }
    if !st.user.is_empty() {
        parts.push(format!("user={}", st.user));
    }
    if !st.pass.is_empty() {
        parts.push(format!("password={}", st.pass));
    }
    if !st.database.is_empty() {
        parts.push(format!("dbname={}", st.database));
    }
    // sslmode mirrors the encrypt + trust knobs:
    //   encrypt=no                    → sslmode=disable (no TlsConnector)
    //   encrypt=yes, trust_cert=no    → sslmode=verify-full (validates cert)
    //   encrypt=yes, trust_cert=yes   → sslmode=require (no validation)
    if st.encrypt {
        let mode = if st.trust_server_certificate {
            "require"
        } else {
            "verify-full"
        };
        parts.push(format!("sslmode={}", mode));
    } else {
        parts.push("sslmode=disable".to_string());
    }
    parts.join(" ")
}

// ── Connection establishment ─────────────────────────────────────────────────

fn connect_mssql() -> Result<SqlConn, i32> {
    let env = odbc_env()?;
    let cs = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        build_mssql_conn_str(&st)
    };
    let display = redact_password(&cs);
    trace(&format!("sql/mssql: connecting — conn_str={}", display));
    match env.connect_with_connection_string(&cs, ConnectionOptions::default()) {
        Ok(conn) => {
            trace("sql/mssql: connected OK");
            Ok(SqlConn::Mssql(conn))
        }
        Err(e) => {
            trace(&format!("sql/mssql: CONNECT FAILED: {e}"));
            Err(set_err(ERR_CONTEXT_SETUP))
        }
    }
}

fn connect_postgres() -> Result<SqlConn, i32> {
    let (cs, encrypt, accept_invalid_certs) = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        (
            build_postgres_conn_str(&st),
            st.encrypt,
            st.trust_server_certificate,
        )
    };
    let display = redact_password(&cs);
    trace(&format!(
        "sql/postgres: connecting tls={} trust_cert={} — conn_str={}",
        encrypt, accept_invalid_certs, display
    ));

    // [MDS] ENCRYPT=yes turns on TLS; TRUST_SERVER_CERTIFICATE=yes skips
    // hostname / cert validation (matches the MSSQL knob semantics so
    // operators don't have to learn two different vocabularies).
    let result = if encrypt {
        let mut builder = native_tls::TlsConnector::builder();
        if accept_invalid_certs {
            builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
        let connector = match builder.build() {
            Ok(c) => c,
            Err(e) => {
                trace(&format!("sql/postgres: TLS connector build failed: {e}"));
                return Err(set_err(ERR_CONTEXT_SETUP));
            }
        };
        let tls = postgres_native_tls::MakeTlsConnector::new(connector);
        postgres::Client::connect(&cs, tls)
    } else {
        postgres::Client::connect(&cs, postgres::NoTls)
    };

    match result {
        Ok(client) => {
            trace("sql/postgres: connected OK");
            Ok(SqlConn::Postgres(client))
        }
        Err(e) => {
            trace(&format!("sql/postgres: CONNECT FAILED: {e}"));
            Err(set_err(ERR_CONTEXT_SETUP))
        }
    }
}

fn connect_sqlite() -> Result<SqlConn, i32> {
    let path = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        if st.database.is_empty() {
            ":memory:".to_string()
        } else {
            st.database.clone()
        }
    };
    trace(&format!("sql/sqlite: opening {}", path));
    match rusqlite::Connection::open(&path) {
        Ok(conn) => {
            trace("sql/sqlite: opened OK");
            Ok(SqlConn::Sqlite(conn))
        }
        Err(e) => {
            trace(&format!("sql/sqlite: OPEN FAILED: {e}"));
            Err(set_err(ERR_CONTEXT_SETUP))
        }
    }
}

fn connect() -> Result<SqlConn, i32> {
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    match backend {
        Backend::Mssql => connect_mssql(),
        Backend::Postgres => connect_postgres(),
        Backend::Sqlite => connect_sqlite(),
    }
}

/// Run `f` with a live connection. Reconnects on demand and drops the
/// cached connection on connection-level errors so the next call retries.
fn with_conn<F, T>(f: F) -> Result<T, i32>
where
    F: FnOnce(&mut SqlConn) -> Result<T, i32>,
{
    let mut g = conn_cell().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
    if g.is_none() {
        *g = Some(connect()?);
    }
    let res = f(g.as_mut().unwrap());
    if let Err(e) = &res {
        if *e == ERR_CONTEXT_FAILURE || *e == ERR_CONTEXT_SETUP {
            trace("sql: connection error — will reconnect on next call");
            *g = None;
        }
    }
    res
}

// ── Public SQL helpers ────────────────────────────────────────────────────────

/// Begin a transaction. `isolation` is honored only on backends that
/// support per-txn isolation levels (currently MSSQL); other backends use
/// their default isolation.
pub fn begin_txn(isolation: &str) -> Result<(), i32> {
    if TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_TXN_ACTIVE);
    }
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    let dialect = crate::dialect::for_backend(backend);
    let sql = match backend {
        Backend::Mssql => {
            let iso = match isolation.to_ascii_uppercase().as_str() {
                "READ UNCOMMITTED" => "READ UNCOMMITTED",
                "READ COMMITTED" => "READ COMMITTED",
                "REPEATABLE READ" => "REPEATABLE READ",
                "SERIALIZABLE" => "SERIALIZABLE",
                "SNAPSHOT" => "SNAPSHOT",
                _ => "READ COMMITTED",
            };
            format!("SET TRANSACTION ISOLATION LEVEL {iso}; BEGIN TRANSACTION;")
        }
        _ => dialect.begin_txn().to_string(),
    };
    execute_sql(&sql)?;
    TXN_ACTIVE.store(true, Ordering::Release);
    trace(&format!("sql: BEGIN TRANSACTION ({})", backend.as_str()));
    Ok(())
}

pub fn commit_txn() -> Result<(), i32> {
    if !TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_NO_TXN);
    }
    let dialect = crate::dialect::active();
    let rc = execute_sql(dialect.commit());
    TXN_ACTIVE.store(false, Ordering::Release);
    trace("sql: COMMIT");
    rc
}

pub fn abort_txn() -> Result<(), i32> {
    if !TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_NO_TXN);
    }
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    let sql = match backend {
        Backend::Mssql => "IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION;".to_string(),
        _ => crate::dialect::for_backend(backend).rollback().to_string(),
    };
    let rc = execute_sql(&sql);
    TXN_ACTIVE.store(false, Ordering::Release);
    trace("sql: ROLLBACK");
    rc
}

/// Execute a non-query statement (INSERT / UPDATE / DELETE / DDL) with no
/// bind parameters. Equivalent to `execute_with(sql, &[])` but kept as the
/// idiomatic shorthand for DDL and other static SQL.
pub fn execute_sql(sql: &str) -> Result<(), i32> {
    execute_with(sql, &[])
}

/// Execute a non-query statement with bound parameters.
pub fn execute_with(sql: &str, params: &[SqlValue]) -> Result<(), i32> {
    with_conn(|conn| match conn {
        SqlConn::Mssql(c) => exec_mssql(c, sql, params),
        SqlConn::Postgres(c) => exec_postgres(c, sql, params),
        SqlConn::Sqlite(c) => exec_sqlite(c, sql, params),
    })
}

/// Acquire an exclusive row-level advisory lock on MSSQL via sp_getapplock.
/// Postgres equivalent (`pg_advisory_lock`) and SQLite no-op are wired in
/// step 4/5 of the multi-backend rollout.
pub fn resolve_lock_prefix(db: &str, schema: &str, table: &str) -> Result<String, i32> {
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    if backend != Backend::Mssql {
        // Non-MSSQL backends use a simpler lock-key scheme TBD.
        return Ok(format!("{}.{}.{}", db, schema, table));
    }
    let fq = format!("{}.{}.{}", db, schema, table);
    let sql = format!(
        "SELECT CAST(DB_ID(N'{}') AS VARCHAR(10)) + '.' + CAST(OBJECT_ID(N'{}') AS VARCHAR(20))",
        db.replace('\'', "''"),
        fq.replace('\'', "''")
    );
    let rows = fetch_rows_positional(&sql, 1, 1)?;
    rows.into_iter()
        .next()
        .and_then(|r| r.into_iter().next())
        .filter(|s| !s.is_empty() && s != "NULL.NULL" && !s.contains("NULL"))
        .ok_or_else(|| set_err(ERR_CONTEXT_SETUP))
}

pub fn lock_row(lock_prefix: &str, row_id: i64) -> Result<(), i32> {
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    match backend {
        Backend::Mssql => {
            let resource = format!("{}.{}", lock_prefix, row_id);
            let sql = format!(
                "DECLARE @r INT; \
                 EXEC @r = sp_getapplock @Resource=N'{}', @LockMode=N'Exclusive', @LockOwner=N'Session', @LockTimeout=0; \
                 IF @r < 0 RAISERROR('Row locked by another session',16,1);",
                resource.replace('\'', "''")
            );
            with_conn(|conn| match conn {
                SqlConn::Mssql(c) => c
                    .execute(&sql, ())
                    .map(|_| ())
                    .map_err(|_| set_err(BTR_RECORD_LOCKED)),
                _ => unreachable!("backend mismatch"),
            })
        }
        Backend::Postgres => {
            // pg_advisory_lock takes a bigint; hash the resource into one.
            let key = stable_lock_key(lock_prefix, row_id);
            let sql = format!("SELECT pg_advisory_lock({key})");
            with_conn(|conn| match conn {
                SqlConn::Postgres(c) => c
                    .batch_execute(&sql)
                    .map_err(|_| set_err(BTR_RECORD_LOCKED)),
                _ => unreachable!(),
            })
        }
        Backend::Sqlite => Ok(()), // SQLite is single-writer; no advisory locks.
    }
}

pub fn unlock_row(lock_prefix: &str, row_id: i64) -> Result<(), i32> {
    let backend = {
        let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
        st.backend
    };
    match backend {
        Backend::Mssql => {
            let resource = format!("{}.{}", lock_prefix, row_id);
            let sql = format!(
                "EXEC sp_releaseapplock @Resource=N'{}', @LockOwner=N'Session';",
                resource.replace('\'', "''")
            );
            with_conn(|conn| match conn {
                SqlConn::Mssql(c) => c
                    .execute(&sql, ())
                    .map(|_| ())
                    .map_err(|_| set_err(ERR_CONTEXT_FAILURE)),
                _ => unreachable!(),
            })
        }
        Backend::Postgres => {
            let key = stable_lock_key(lock_prefix, row_id);
            let sql = format!("SELECT pg_advisory_unlock({key})");
            with_conn(|conn| match conn {
                SqlConn::Postgres(c) => c
                    .batch_execute(&sql)
                    .map_err(|_| set_err(ERR_CONTEXT_FAILURE)),
                _ => unreachable!(),
            })
        }
        Backend::Sqlite => Ok(()),
    }
}

fn stable_lock_key(prefix: &str, row_id: i64) -> i64 {
    // FNV-1a over (prefix, row_id) → i64.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in prefix.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for b in row_id.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h as i64
}

/// Fetch up to `limit` rows from `sql` with no bind parameters.
/// Wrapper around [`fetch_with`] for static SQL.
pub fn fetch_rows_positional(
    sql: &str,
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    fetch_with(sql, &[], n_cols, limit)
}

/// Fetch up to `limit` rows from `sql` with bound parameters. Each row is
/// `Vec<String>` aligned to the SELECT column list. The single canonical
/// query helper for the multi-backend rollout.
pub fn fetch_with(
    sql: &str,
    params: &[SqlValue],
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    if params.is_empty() {
        trace(&format!("fetch sql={}", sql));
    } else {
        trace(&format!(
            "fetch sql={} params={}",
            sql,
            debug_params_short(params)
        ));
    }
    with_conn(|conn| match conn {
        SqlConn::Mssql(c) => fetch_mssql(c, sql, params, n_cols, limit),
        SqlConn::Postgres(c) => fetch_postgres(c, sql, params, n_cols, limit),
        SqlConn::Sqlite(c) => fetch_sqlite(c, sql, params, n_cols, limit),
    })
}

fn debug_params_short(params: &[SqlValue]) -> String {
    let mut out = String::with_capacity(params.len() * 8);
    out.push('[');
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&crate::sql_param::debug_literal(p));
    }
    out.push(']');
    out
}

// ── MSSQL marshalling ────────────────────────────────────────────────────────

fn exec_mssql(
    c: &mut OdbcConn<'static>,
    sql: &str,
    params: &[SqlValue],
) -> Result<(), i32> {
    let bound = bind_mssql(params);
    let res = if bound.is_empty() {
        c.execute(sql, ())
    } else {
        c.execute(sql, bound.as_slice())
    };
    res.map(|_| ()).map_err(|e| {
        trace(&format!("execute/mssql ERROR: {e}"));
        set_err(ERR_CONTEXT_FAILURE)
    })
}

fn fetch_mssql(
    c: &mut OdbcConn<'static>,
    sql: &str,
    params: &[SqlValue],
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    let bound = bind_mssql(params);
    let res = if bound.is_empty() {
        c.execute(sql, ())
    } else {
        c.execute(sql, bound.as_slice())
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    match res {
        Err(e) => {
            trace(&format!("fetch/mssql execute error: {e}"));
            return Err(set_err(ERR_CONTEXT_FAILURE));
        }
        Ok(None) => {}
        Ok(Some(mut cursor)) => {
            while rows.len() < limit {
                let next = cursor.next_row().map_err(|e| {
                    trace(&format!("fetch/mssql next_row error: {e}"));
                    set_err(ERR_CONTEXT_FAILURE)
                })?;
                let Some(mut r) = next else { break };
                let mut row = Vec::with_capacity(n_cols);
                for col in 1..=(n_cols as u16) {
                    let mut v: Vec<u8> = Vec::new();
                    let has = r.get_text(col, &mut v).unwrap_or(false);
                    row.push(if has {
                        String::from_utf8_lossy(&v).into_owned()
                    } else {
                        String::new()
                    });
                }
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

/// MSSQL bind: convert each SqlValue to an `odbc_api::Box<dyn InputParameter>`
/// owned by the returned vec. The slice is `Send`+'static safe to pass to
/// `Connection::execute` for the duration of the call.
fn bind_mssql(params: &[SqlValue]) -> Vec<Box<dyn odbc_api::parameter::InputParameter>> {
    use odbc_api::IntoParameter;
    params
        .iter()
        .map(|v| -> Box<dyn odbc_api::parameter::InputParameter> {
            match v {
                SqlValue::Null => Box::new(Option::<i32>::None.into_parameter()),
                SqlValue::Bool(b) => Box::new((*b as i32).into_parameter()),
                SqlValue::I32(i) => Box::new((*i).into_parameter()),
                SqlValue::I64(i) => Box::new((*i).into_parameter()),
                SqlValue::F64(f) => Box::new((*f).into_parameter()),
                SqlValue::Text(s) => Box::new(s.clone().into_parameter()),
                SqlValue::Bytes(b) => Box::new(b.clone().into_parameter()),
            }
        })
        .collect()
}

// ── Postgres marshalling ─────────────────────────────────────────────────────

fn exec_postgres(
    c: &mut postgres::Client,
    sql: &str,
    params: &[SqlValue],
) -> Result<(), i32> {
    if params.is_empty() {
        c.batch_execute(sql).map_err(|e| {
            trace(&format!("execute/postgres ERROR: {e}"));
            set_err(ERR_CONTEXT_FAILURE)
        })
    } else {
        let bound = bind_postgres(params);
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
            bound.iter().map(|b| b.as_ref()).collect();
        c.execute(sql, refs.as_slice()).map(|_| ()).map_err(|e| {
            trace(&format!("execute/postgres ERROR: {e}"));
            set_err(ERR_CONTEXT_FAILURE)
        })
    }
}

fn fetch_postgres(
    c: &mut postgres::Client,
    sql: &str,
    params: &[SqlValue],
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    let bound = bind_postgres(params);
    let refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
        bound.iter().map(|b| b.as_ref()).collect();
    let pg_rows = c.query(sql, refs.as_slice()).map_err(|e| {
        let detail = e
            .as_db_error()
            .map(|d| format!("{}: {}", d.code().code(), d.message()))
            .unwrap_or_else(|| format!("{e:?}"));
        trace(&format!("fetch/postgres query error: {detail}"));
        set_err(ERR_CONTEXT_FAILURE)
    })?;
    let mut rows = Vec::with_capacity(pg_rows.len().min(limit));
    for r in pg_rows.into_iter().take(limit) {
        let mut row = Vec::with_capacity(n_cols);
        for col in 0..n_cols {
            row.push(pg_col_to_string(&r, col));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Read a postgres column as a String regardless of its underlying type.
/// Each branch matches the column's pg type and converts to text. Mirrors
/// the type list in PgInt / PgText so the round-trip stays lossless for
/// the column types the runtime cares about.
fn pg_col_to_string(row: &postgres::Row, col: usize) -> String {
    use postgres::types::Type;
    let cols = row.columns();
    let Some(c) = cols.get(col) else {
        return String::new();
    };
    let ty = c.type_();
    match *ty {
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => row
            .try_get::<_, Option<String>>(col)
            .ok()
            .flatten()
            .unwrap_or_default(),
        Type::INT2 => row
            .try_get::<_, Option<i16>>(col)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::INT4 => row
            .try_get::<_, Option<i32>>(col)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(col)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(col)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(col)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        Type::BOOL => row
            .try_get::<_, Option<bool>>(col)
            .ok()
            .flatten()
            .map(|v| if v { "1".to_string() } else { "0".to_string() })
            .unwrap_or_default(),
        Type::DATE => row
            .try_get::<_, Option<chrono::NaiveDate>>(col)
            .ok()
            .flatten()
            .map(|v| v.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        Type::TIME => row
            .try_get::<_, Option<chrono::NaiveTime>>(col)
            .ok()
            .flatten()
            .map(|v| v.format("%H:%M:%S").to_string())
            .unwrap_or_default(),
        Type::TIMESTAMP | Type::TIMESTAMPTZ => row
            .try_get::<_, Option<chrono::NaiveDateTime>>(col)
            .ok()
            .flatten()
            .map(|v| v.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Postgres ToSql adapter for SQL NULL that accepts any column type.
/// `Option::<T>::None` is type-checked in postgres, so binding NULL with
/// the wrong T (e.g. None::<i32> for a DATE column) fails. This adapter
/// accepts every type and always writes IsNull::Yes.
#[derive(Debug)]
struct PgNull;

impl postgres::types::ToSql for PgNull {
    fn to_sql(
        &self,
        _ty: &postgres::types::Type,
        _out: &mut bytes::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        Ok(postgres::types::IsNull::Yes)
    }
    fn accepts(_ty: &postgres::types::Type) -> bool {
        true
    }
    postgres::types::to_sql_checked!();
}

/// Postgres ToSql adapter that accepts any integer column type
/// (INT2/INT4/INT8) and serializes the held i64 in the target's width.
/// Built-in `i64::ToSql` only accepts `INT8`, so binding a Btrieve INT
/// to a SMALLINT/INTEGER column would fail without this.
#[derive(Debug)]
struct PgInt(i64);

impl postgres::types::ToSql for PgInt {
    fn to_sql(
        &self,
        ty: &postgres::types::Type,
        out: &mut bytes::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        use postgres::types::Type;
        match *ty {
            Type::INT2 => (self.0 as i16).to_sql(ty, out),
            Type::INT4 => (self.0 as i32).to_sql(ty, out),
            Type::INT8 => self.0.to_sql(ty, out),
            // Numeric / decimal: fall through to text representation.
            Type::FLOAT4 => (self.0 as f32).to_sql(ty, out),
            Type::FLOAT8 => (self.0 as f64).to_sql(ty, out),
            _ => Err(format!("PgInt: unsupported target type {:?}", ty).into()),
        }
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        use postgres::types::Type;
        matches!(
            *ty,
            Type::INT2 | Type::INT4 | Type::INT8 | Type::FLOAT4 | Type::FLOAT8
        )
    }

    postgres::types::to_sql_checked!();
}

/// Postgres ToSql adapter for our SqlValue::Text. Accepts the obvious
/// string column types and also parses ISO 8601 forms when the target
/// column is DATE / TIME / TIMESTAMP, since the runtime always emits
/// dates and times as Text values.
#[derive(Debug)]
struct PgText(String);

impl postgres::types::ToSql for PgText {
    fn to_sql(
        &self,
        ty: &postgres::types::Type,
        out: &mut bytes::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        use postgres::types::Type;
        match *ty {
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => {
                self.0.to_sql(ty, out)
            }
            Type::DATE => {
                let d = chrono::NaiveDate::parse_from_str(self.0.trim(), "%Y-%m-%d")
                    .map_err(|e| format!("PgText DATE parse '{}': {e}", self.0))?;
                d.to_sql(ty, out)
            }
            Type::TIME => {
                let t = chrono::NaiveTime::parse_from_str(self.0.trim(), "%H:%M:%S")
                    .or_else(|_| {
                        chrono::NaiveTime::parse_from_str(self.0.trim(), "%H:%M:%S%.f")
                    })
                    .map_err(|e| format!("PgText TIME parse '{}': {e}", self.0))?;
                t.to_sql(ty, out)
            }
            Type::TIMESTAMP | Type::TIMESTAMPTZ => {
                let s = self.0.trim();
                let dt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
                    .map_err(|e| format!("PgText TIMESTAMP parse '{}': {e}", s))?;
                dt.to_sql(ty, out)
            }
            _ => Err(format!("PgText: unsupported target type {:?}", ty).into()),
        }
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        use postgres::types::Type;
        matches!(
            *ty,
            Type::TEXT
                | Type::VARCHAR
                | Type::BPCHAR
                | Type::NAME
                | Type::UNKNOWN
                | Type::DATE
                | Type::TIME
                | Type::TIMESTAMP
                | Type::TIMESTAMPTZ
        )
    }

    postgres::types::to_sql_checked!();
}

/// Same idea as PgInt but for f64, accepts FLOAT4/FLOAT8/NUMERIC.
#[derive(Debug)]
struct PgFloat(f64);

impl postgres::types::ToSql for PgFloat {
    fn to_sql(
        &self,
        ty: &postgres::types::Type,
        out: &mut bytes::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        use postgres::types::Type;
        match *ty {
            Type::FLOAT4 => (self.0 as f32).to_sql(ty, out),
            Type::FLOAT8 => self.0.to_sql(ty, out),
            Type::INT2 => (self.0 as i16).to_sql(ty, out),
            Type::INT4 => (self.0 as i32).to_sql(ty, out),
            Type::INT8 => (self.0 as i64).to_sql(ty, out),
            _ => Err(format!("PgFloat: unsupported target type {:?}", ty).into()),
        }
    }

    fn accepts(ty: &postgres::types::Type) -> bool {
        use postgres::types::Type;
        matches!(
            *ty,
            Type::FLOAT4 | Type::FLOAT8 | Type::INT2 | Type::INT4 | Type::INT8
        )
    }

    postgres::types::to_sql_checked!();
}

fn bind_postgres(params: &[SqlValue]) -> Vec<Box<dyn postgres::types::ToSql + Sync>> {
    params
        .iter()
        .map(|v| -> Box<dyn postgres::types::ToSql + Sync> {
            match v {
                SqlValue::Null => Box::new(PgNull),
                // Convert Bool to int 0/1 so it matches SMALLINT/INTEGER
                // columns (Postgres won't auto-coerce bool -> int).
                SqlValue::Bool(b) => Box::new(PgInt(if *b { 1 } else { 0 })),
                SqlValue::I32(i) => Box::new(PgInt(*i as i64)),
                SqlValue::I64(i) => Box::new(PgInt(*i)),
                SqlValue::F64(f) => Box::new(PgFloat(*f)),
                SqlValue::Text(s) => Box::new(PgText(s.clone())),
                SqlValue::Bytes(b) => Box::new(b.clone()),
            }
        })
        .collect()
}

// ── SQLite marshalling ───────────────────────────────────────────────────────

fn exec_sqlite(
    c: &rusqlite::Connection,
    sql: &str,
    params: &[SqlValue],
) -> Result<(), i32> {
    if params.is_empty() {
        c.execute_batch(sql).map_err(|e| {
            trace(&format!("execute/sqlite ERROR: {e}"));
            set_err(ERR_CONTEXT_FAILURE)
        })
    } else {
        let bound = bind_sqlite(params);
        c.execute(sql, rusqlite::params_from_iter(bound.iter()))
            .map(|_| ())
            .map_err(|e| {
                trace(&format!("execute/sqlite ERROR: {e}"));
                set_err(ERR_CONTEXT_FAILURE)
            })
    }
}

fn fetch_sqlite(
    c: &rusqlite::Connection,
    sql: &str,
    params: &[SqlValue],
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    let bound = bind_sqlite(params);
    let mut stmt = c.prepare(sql).map_err(|e| {
        trace(&format!("fetch/sqlite prepare error: {e}"));
        set_err(ERR_CONTEXT_FAILURE)
    })?;
    let mut sql_rows = stmt
        .query(rusqlite::params_from_iter(bound.iter()))
        .map_err(|e| {
            trace(&format!("fetch/sqlite query error: {e}"));
            set_err(ERR_CONTEXT_FAILURE)
        })?;
    let mut rows = Vec::new();
    while rows.len() < limit {
        let next = sql_rows.next().map_err(|e| {
            trace(&format!("fetch/sqlite next error: {e}"));
            set_err(ERR_CONTEXT_FAILURE)
        })?;
        let Some(r) = next else { break };
        let mut row = Vec::with_capacity(n_cols);
        for col in 0..n_cols {
            let v: rusqlite::types::Value = r.get(col).unwrap_or(rusqlite::types::Value::Null);
            row.push(match v {
                rusqlite::types::Value::Null => String::new(),
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Real(f) => f.to_string(),
                rusqlite::types::Value::Text(s) => s,
                rusqlite::types::Value::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
            });
        }
        rows.push(row);
    }
    Ok(rows)
}

fn bind_sqlite(params: &[SqlValue]) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value;
    params
        .iter()
        .map(|v| match v {
            SqlValue::Null => Value::Null,
            SqlValue::Bool(b) => Value::Integer(if *b { 1 } else { 0 }),
            SqlValue::I32(i) => Value::Integer(*i as i64),
            SqlValue::I64(i) => Value::Integer(*i),
            SqlValue::F64(f) => Value::Real(*f),
            SqlValue::Text(s) => Value::Text(s.clone()),
            SqlValue::Bytes(b) => Value::Blob(b.clone()),
        })
        .collect()
}

/// Convenience: fetch a single row or return KEY_NOT_FOUND.
pub fn fetch_one_row(sql: &str, n_cols: usize) -> Result<Vec<String>, i32> {
    let mut rows = fetch_rows_positional(sql, n_cols, 1)?;
    rows.pop().ok_or(4)
}

/// Fetch up to `limit` rows, each as a pipe-delimited string of all columns
/// (DBU legacy interface).
pub fn fetch_rows_text(sql: &str, limit: usize) -> Result<Vec<String>, i32> {
    with_conn(|conn| match conn {
        SqlConn::Mssql(c) => fetch_text_mssql(c, sql, limit),
        SqlConn::Postgres(c) => {
            // Use the positional fetcher and join columns with '|'.
            let pg_rows = c.query(sql, &[]).map_err(|e| {
                trace(&format!("fetch_text/postgres query error: {e}"));
                set_err(ERR_CONTEXT_FAILURE)
            })?;
            let mut out = Vec::with_capacity(pg_rows.len().min(limit));
            for r in pg_rows.into_iter().take(limit) {
                let cols = r.columns().len();
                let parts: Vec<String> = (0..cols).map(|i| pg_col_to_string(&r, i)).collect();
                out.push(parts.join("|"));
            }
            Ok(out)
        }
        SqlConn::Sqlite(c) => {
            let mut stmt = c
                .prepare(sql)
                .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?;
            let n_cols = stmt.column_count();
            let mut sql_rows = stmt
                .query([])
                .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?;
            let mut out = Vec::new();
            while out.len() < limit {
                let Some(r) = sql_rows
                    .next()
                    .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?
                else {
                    break;
                };
                let parts: Vec<String> = (0..n_cols)
                    .map(|i| match r.get::<_, rusqlite::types::Value>(i) {
                        Ok(rusqlite::types::Value::Text(s)) => s,
                        Ok(rusqlite::types::Value::Integer(i)) => i.to_string(),
                        Ok(rusqlite::types::Value::Real(f)) => f.to_string(),
                        Ok(rusqlite::types::Value::Blob(b)) => {
                            String::from_utf8_lossy(&b).into_owned()
                        }
                        _ => String::new(),
                    })
                    .collect();
                out.push(parts.join("|"));
            }
            Ok(out)
        }
    })
}

fn fetch_text_mssql(
    c: &mut OdbcConn<'static>,
    sql: &str,
    limit: usize,
) -> Result<Vec<String>, i32> {
    let mut rows = Vec::new();
    if let Some(mut cursor) = c
        .execute(sql, ())
        .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?
    {
        let n_cols = cursor.num_result_cols().unwrap_or(8).max(0) as u16;
        while rows.len() < limit {
            let next = cursor
                .next_row()
                .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?;
            let Some(mut r) = next else { break };
            let mut out = String::new();
            for col in 1..=n_cols {
                let mut v = Vec::new();
                let has = r
                    .get_text(col, &mut v)
                    .map_err(|_| set_err(ERR_CONTEXT_FAILURE))?;
                if !has {
                    break;
                }
                if !out.is_empty() {
                    out.push('|');
                }
                out.push_str(String::from_utf8_lossy(&v).as_ref());
            }
            rows.push(out);
        }
    }
    Ok(rows)
}

/// Auto-discover table schema. Currently MSSQL-only via INFORMATION_SCHEMA.
/// Other backends return Err(12 NOT_FOUND); their discovery paths land in
/// step 3/4 of the multi-backend rollout.
pub fn discover_table_meta(
    table_name: &str,
    open_path: &str,
) -> Result<crate::state::TableMeta, i32> {
    use crate::state::{IntField, RuntimeIndex, TableMeta};

    let backend = {
        let st = state().lock().map_err(|_| 12i32)?;
        st.backend
    };
    if backend != Backend::Mssql {
        strace!("discover_table_meta: backend {} not yet supported", backend.as_str());
        return Err(12);
    }

    let dir = crate::state::dir_from_path(open_path);
    let db_name = crate::state::resolve_config(&dir, "DATABASE");
    let schema_name = {
        let s = crate::state::resolve_config(&dir, "SCHEMA");
        if s.is_empty() {
            "dbo".to_string()
        } else {
            s
        }
    };
    let recnum_col = crate::state::resolve_config(&dir, "RECNUM_COLUMN");
    let recnum_col = if recnum_col.is_empty() {
        "MDS_RECNUM".to_string()
    } else {
        recnum_col
    };
    let trim_strings = {
        let st = crate::state::state().lock().map_err(|_| 12i32)?;
        st.trim_strings
    };

    strace!(
        "discover_table_meta table={} db={} schema={}",
        table_name,
        db_name,
        schema_name
    );

    let col_sql = format!(
        "SELECT COLUMN_NAME, DATA_TYPE, COALESCE(CHARACTER_MAXIMUM_LENGTH,0), ORDINAL_POSITION \
         FROM [{db}].INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA='{sc}' AND TABLE_NAME='{tbl}' ORDER BY ORDINAL_POSITION",
        db = db_name,
        sc = schema_name,
        tbl = table_name
    );

    let col_rows = fetch_rows_positional(&col_sql, 4, 200)?;
    if col_rows.is_empty() {
        strace!(
            "discover_table_meta: no columns for {}.{}.{}",
            db_name,
            schema_name,
            table_name
        );
        return Err(12);
    }

    let mut fields = Vec::new();
    let mut offset: u32 = 0;
    let mut fnum: u32 = 1;
    for row in &col_rows {
        let name = row
            .first()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let dtype = row
            .get(1)
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        let maxlen: u32 = row.get(2).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        if name.eq_ignore_ascii_case("MDS_RECNUM") {
            continue;
        }

        let (nt, len) = match dtype.as_str() {
            "char" | "nchar" => (0i32, maxlen.max(1)),
            "varchar" | "nvarchar" => (0, maxlen.clamp(1, 255)),
            "int" => (1, 4),
            "smallint" => (1, 2),
            "tinyint" => (14, 1),
            "bigint" => (1, 8),
            "decimal" | "numeric" => (5, 8),
            "float" | "real" => (2, 8),
            "bit" => (7, 1),
            "datetime" | "datetime2" => (3, 8),
            "date" => (3, 4),
            _ => (0, maxlen.clamp(1, 255)),
        };
        fields.push(IntField {
            num: fnum,
            name,
            native_type: nt,
            length: len,
            offset,
            field_index: None,
            default_value: None,
        });
        fnum += 1;
        offset += len;
    }

    let record_length = offset;
    let indexes: Vec<RuntimeIndex> = Vec::new();

    strace!(
        "discover_table_meta OK table={} db={} fields={} indexes={} reclen={}",
        table_name,
        db_name,
        fields.len(),
        indexes.len(),
        record_length
    );

    Ok(TableMeta {
        table_name: table_name.to_string(),
        schema_name: schema_name.to_string(),
        db_name: db_name.to_string(),
        record_length,
        page_size: 4096,
        file_flags: 0,
        fields,
        indexes,
        recnum_col,
        ignore_null_values: true,
        trim_string_fields: trim_strings,
        translate_oem_to_ansi: false,
        primary_index: None,
        local_cache: false,
    })
}
