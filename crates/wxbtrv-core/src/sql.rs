use crate::constants::{BTR_RECORD_LOCKED, ERR_CONTEXT_FAILURE, ERR_CONTEXT_SETUP, ERR_NOT_LOADED};
use crate::state::{set_err, state};
use crate::trace::{get_seq, trace};

macro_rules! strace {
    ($($arg:tt)*) => {
        trace(&format!("#{}   {}", get_seq(), format!($($arg)*)))
    };
}
use odbc_api::{Connection, ConnectionOptions, Cursor, Environment, ResultSetMetadata};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Global flag: there is an active SQL Server transaction on the shared
/// connection. Set by begin_txn(), cleared by commit_txn()/abort_txn().
/// Exposed so op_reset and friends can observe / rollback if needed.
pub static TXN_ACTIVE: AtomicBool = AtomicBool::new(false);

// ── Persistent ODBC environment + connection ──────────────────────────────────
//
// Environment is Send+Sync (odbc_api marks it so).  We create it once and keep
// it for the lifetime of the process.  Connection<'static> borrows &'static Env.
// Connection is Send but not Sync, so we wrap it in a Mutex.
//
// Having one connection per process means:
//   • All BTRCALL ops share a SQL Server session → SCOPE_IDENTITY(), @@SPID, and
//     future sp_getapplock calls all work correctly.
//   • Connection overhead (TCP handshake + auth) is paid once, not per BTRCALL.
//   • Multiple threads serialize on the mutex — acceptable because Btrieve itself
//     is fundamentally single-threaded per position-block.

static ODBC_ENV: OnceLock<Environment> = OnceLock::new();
static CONN: OnceLock<Mutex<Option<Connection<'static>>>> = OnceLock::new();

fn odbc_env() -> Result<&'static Environment, i32> {
    // Try to initialize the ODBC environment if it hasn't been yet.
    // If the driver manager is unavailable (e.g. unixODBC / msodbcsql not installed),
    // we must return an error — never panic, because this code runs inside NTVDM
    // and a panic would take the whole DOS process down with an illegal-instruction trap.
    if ODBC_ENV.get().is_none() {
        match Environment::new() {
            Ok(env) => {
                // Ignore the Err from set — another thread may have raced us.
                let _ = ODBC_ENV.set(env);
            }
            Err(e) => {
                crate::trace::trace(&format!(
                    "odbc_env: Environment::new() failed: {e:?} — ODBC driver manager unavailable"
                ));
                return Err(set_err(ERR_NOT_LOADED));
            }
        }
    }
    ODBC_ENV.get().ok_or_else(|| set_err(ERR_NOT_LOADED))
}

fn conn_cell() -> &'static Mutex<Option<Connection<'static>>> {
    CONN.get_or_init(|| Mutex::new(None))
}

/// Drop the cached connection so the next call re-connects.
/// Call this when server/database/credentials change (MDS.INI reload, op_stop).
pub fn reset_connection() {
    if let Ok(mut g) = conn_cell().lock() {
        *g = None;
    }
}

fn redact_password(cs: &str) -> String {
    // Replace Pwd=<value>; with Pwd=***; for safe logging
    let mut out = String::with_capacity(cs.len());
    let lower = cs.to_ascii_lowercase();
    let mut i = 0;
    while i < cs.len() {
        if lower[i..].starts_with("pwd=") {
            out.push_str("Pwd=***;");
            // skip past Pwd=<value>;
            i += 4; // skip "pwd="
            while i < cs.len() && cs.as_bytes()[i] != b';' {
                i += 1;
            }
            if i < cs.len() {
                i += 1;
            } // skip ';'
        } else {
            out.push(cs.as_bytes()[i] as char);
            i += 1;
        }
    }
    out
}

fn build_conn_str(st: &crate::state::DriverState) -> String {
    let mut s = String::new();

    if !st.dsn.is_empty() {
        s.push_str(&format!("DSN={};", st.dsn));
    } else {
        // DRIVER must be set explicitly in wxbtrv.db [MDS] DRIVER=...
        s.push_str(&format!("Driver={{{}}};", st.driver));

        let srv = if st.server.is_empty() {
            "localhost"
        } else {
            &st.server
        };
        // NETWORK controls how the driver connects:
        //   ""         — driver default (usually Named Pipes for legacy drivers)
        //   "tcp:"     — prepend tcp: prefix to server (modern drivers)
        //   "DBMSSOCN" — append Network=DBMSSOCN (legacy TCP/IP for old drivers)
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

/// Execute `f` with a live connection.  If the connection is absent or dead,
/// reconnect first.  On connection-level errors, drop the connection so the
/// next call triggers a fresh reconnect.
fn with_conn<F, T>(f: F) -> Result<T, i32>
where
    F: FnOnce(&Connection<'static>) -> Result<T, i32>,
{
    let env = odbc_env()?;
    let mut g = conn_cell().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;

    // (Re-)connect if needed.
    if g.is_none() {
        let cs = {
            let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
            build_conn_str(&st)
        };
        let display = redact_password(&cs);
        trace(&format!(
            "sql: connecting — driver={:?} network={:?} server={:?}",
            {
                let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
                st.driver.clone()
            },
            {
                let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
                st.network.clone()
            },
            {
                let st = state().lock().map_err(|_| set_err(ERR_CONTEXT_SETUP))?;
                st.server.clone()
            }
        ));
        trace(&format!("sql: conn_str={}", display));
        match env.connect_with_connection_string(&cs, ConnectionOptions::default()) {
            Ok(conn) => {
                trace(&format!("sql: connected OK"));
                *g = Some(conn);
            }
            Err(e) => {
                // Log each ODBC diagnostic record for full error detail
                trace(&format!("sql: CONNECT FAILED"));
                trace(&format!("sql:   conn_str={}", display));
                trace(&format!("sql:   error={}", e));
                return Err(set_err(ERR_CONTEXT_SETUP));
            }
        }
    }

    let res = f(g.as_ref().unwrap());

    // Drop connection on errors that indicate the session is gone.
    if let Err(e) = &res {
        if *e == ERR_CONTEXT_FAILURE || *e == ERR_CONTEXT_SETUP {
            trace("sql: connection error — will reconnect on next call");
            *g = None;
        }
    }

    res
}

// ── Public SQL helpers ────────────────────────────────────────────────────────

/// Begin a SQL Server transaction on the shared connection.
///
/// `isolation` is one of: `READ COMMITTED`, `REPEATABLE READ`, `SERIALIZABLE`,
/// `SNAPSHOT`, `READ UNCOMMITTED`. For Btrieve op 19 (exclusive) we use
/// `SERIALIZABLE` (closest to file-level lock); for op 1019 (concurrent)
/// we use `READ COMMITTED` (page-level semantics).
///
/// Returns Err(37 BTR_TXN_ACTIVE) if a txn is already active.
pub fn begin_txn(isolation: &str) -> Result<(), i32> {
    if TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_TXN_ACTIVE);
    }
    // Keep the isolation level set narrow: validated against a whitelist so
    // a stray call can never splice arbitrary SQL.
    let iso = match isolation.to_ascii_uppercase().as_str() {
        "READ UNCOMMITTED" => "READ UNCOMMITTED",
        "READ COMMITTED" => "READ COMMITTED",
        "REPEATABLE READ" => "REPEATABLE READ",
        "SERIALIZABLE" => "SERIALIZABLE",
        "SNAPSHOT" => "SNAPSHOT",
        _ => "READ COMMITTED",
    };
    let sql = format!(
        "SET TRANSACTION ISOLATION LEVEL {}; BEGIN TRANSACTION;",
        iso
    );
    execute_sql(&sql)?;
    TXN_ACTIVE.store(true, Ordering::Release);
    trace(&format!("sql: BEGIN TRANSACTION iso={}", iso));
    Ok(())
}

/// Commit the current transaction. Returns Err(39 BTR_NO_TXN) if none active.
pub fn commit_txn() -> Result<(), i32> {
    if !TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_NO_TXN);
    }
    let rc = execute_sql("COMMIT TRANSACTION;");
    // Whatever the result, clear the flag so we don't get stuck "active".
    TXN_ACTIVE.store(false, Ordering::Release);
    trace("sql: COMMIT TRANSACTION");
    rc
}

/// Roll back the current transaction. Returns Err(39 BTR_NO_TXN) if none active.
pub fn abort_txn() -> Result<(), i32> {
    if !TXN_ACTIVE.load(Ordering::Acquire) {
        return Err(crate::constants::BTR_NO_TXN);
    }
    let rc = execute_sql("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION;");
    TXN_ACTIVE.store(false, Ordering::Release);
    trace("sql: ROLLBACK TRANSACTION");
    rc
}

/// Execute a non-query SQL statement (INSERT / UPDATE / DELETE / DDL).
pub fn execute_sql(sql: &str) -> Result<(), i32> {
    with_conn(|conn| {
        conn.execute(sql, ()).map(|_| ()).map_err(|e| {
            crate::trace::trace(&format!("execute_sql ERROR: {}", e));
            set_err(ERR_CONTEXT_FAILURE)
        })
    })
}

/// Resolve the "{DB_ID}.{TABLE_ID}" prefix for a table's lock resources.
/// Matches the format used by _adv_row_lock so all DLL instances coordinate.
/// Cached in HandleEntry.lock_prefix after first call.
pub fn resolve_lock_prefix(db: &str, schema: &str, table: &str) -> Result<String, i32> {
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

/// Acquire an exclusive row-level advisory lock via sp_getapplock (Session mode).
/// Resource key: "{DB_ID}.{TABLE_ID}.{row_id}" — matches _adv_row_lock SP format
/// so all DLL instances on different servers coordinate on the same SQL Server.
/// Session-mode: lock persists until unlock_row() or connection drop.
/// Returns Err(BTR_RECORD_LOCKED) if already locked by another session.
pub fn lock_row(lock_prefix: &str, row_id: i64) -> Result<(), i32> {
    let resource = format!("{}.{}", lock_prefix, row_id);
    let sql = format!(
        "DECLARE @r INT; \
         EXEC @r = sp_getapplock @Resource=N'{}', @LockMode=N'Exclusive', @LockOwner=N'Session', @LockTimeout=0; \
         IF @r < 0 RAISERROR('Row locked by another session',16,1);",
        resource.replace('\'', "''")
    );
    with_conn(|conn| {
        conn.execute(&sql, ())
            .map(|_| ())
            .map_err(|_| set_err(BTR_RECORD_LOCKED))
    })
}

/// Release the row-level advisory lock acquired by lock_row().
pub fn unlock_row(lock_prefix: &str, row_id: i64) -> Result<(), i32> {
    let resource = format!("{}.{}", lock_prefix, row_id);
    let sql = format!(
        "EXEC sp_releaseapplock @Resource=N'{}', @LockOwner=N'Session';",
        resource.replace('\'', "''")
    );
    with_conn(|conn| {
        conn.execute(&sql, ())
            .map(|_| ())
            .map_err(|_| set_err(ERR_CONTEXT_FAILURE))
    })
}

/// Fetch up to `limit` rows from `sql`, each row as a `Vec<String>` aligned
/// to the SELECT column list.
pub fn fetch_rows_positional(
    sql: &str,
    n_cols: usize,
    limit: usize,
) -> Result<Vec<Vec<String>>, i32> {
    trace(&format!("fetch_rows_positional sql={}", sql));
    with_conn(|conn| {
        let mut rows: Vec<Vec<String>> = Vec::new();
        match conn.execute(sql, ()) {
            Err(e) => {
                trace(&format!("sql execute error: {}", e));
                return Err(set_err(ERR_CONTEXT_FAILURE));
            }
            Ok(None) => {} // no result set (e.g. plain INSERT/UPDATE/DELETE)
            Ok(Some(mut cursor)) => {
                while rows.len() < limit {
                    let next = cursor.next_row().map_err(|e| {
                        trace(&format!("sql next_row error: {}", e));
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
    })
}

/// Convenience: fetch a single row or return Err(4) = Btrieve KEY_NOT_FOUND.
pub fn fetch_one_row(sql: &str, n_cols: usize) -> Result<Vec<String>, i32> {
    let mut rows = fetch_rows_positional(sql, n_cols, 1)?;
    rows.pop().ok_or(4)
}

/// Fetch up to `limit` rows, each as a pipe-delimited string of all columns.
/// Used by the DBU legacy interface.
pub fn fetch_rows_text(sql: &str, limit: usize) -> Result<Vec<String>, i32> {
    with_conn(|conn| {
        let mut rows = Vec::new();
        if let Some(mut cursor) = conn
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
    })
}

/// Auto-discover table schema from SQL Server when no INT file metadata exists.
/// Uses the open path's directory to determine the database (G:\PACIFIC → GPacific).
/// Falls back to global defaults from wxbtrv.db config.
pub fn discover_table_meta(
    table_name: &str,
    open_path: &str,
) -> Result<crate::state::TableMeta, i32> {
    use crate::state::{IntField, RuntimeIndex, TableMeta};

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

    let col_sql =
        format!(
        "SELECT COLUMN_NAME, DATA_TYPE, COALESCE(CHARACTER_MAXIMUM_LENGTH,0), ORDINAL_POSITION \
         FROM [{db}].INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA='{sc}' AND TABLE_NAME='{tbl}' ORDER BY ORDINAL_POSITION",
        db=db_name, sc=schema_name, tbl=table_name);

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
        let name = row.get(0).map(|s| s.trim().to_string()).unwrap_or_default();
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
            "varchar" | "nvarchar" => (0, maxlen.max(1).min(255)),
            "int" => (1, 4),
            "smallint" => (1, 2),
            "tinyint" => (14, 1),
            "bigint" => (1, 8),
            "decimal" | "numeric" => (5, 8),
            "float" | "real" => (2, 8),
            "bit" => (7, 1),
            "datetime" | "datetime2" => (3, 8),
            "date" => (3, 4),
            _ => (0, maxlen.max(1).min(255)),
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

    // No index discovery from SQL Server — indexes come from table configs only.
    // Auto-discovered tables have fields but no indexes.
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
