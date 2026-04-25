//! btr-test-harness — fixture infrastructure for running wxbtrv-core ops
//! against a real SQL Server backing store.
//!
//! Provides:
//!   * `reset_fixture()` — drops + recreates the WXBTRV_TEST database from
//!     `fixtures/schema.sql`, re-seeds the 10-row TEST_CUST table.
//!   * `install_fixture_config()` — builds a fresh `wxbtrv.db` in a temp dir
//!     that points wxbtrv-core at the test SQL Server + WXBTRV_TEST database,
//!     then resets wxbtrv-core's global state and bumps CWD so its
//!     `find_sqlite_db()` picks up this file.
//!   * `new_posblk()` — 128-byte zeroed position block.
//!   * `fixture_open()` — issue Btrieve op 0 (Open) with a path.
//!
//! The harness writes the wxbtrv.db directly via rusqlite (mirroring the
//! schema from `crates/db-config/src/db/schema.rs`) so we don't have to
//! shell out to int-tool or require an existing binary DB.

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const TEST_DB_NAME: &str = "WXBTRV_TEST";
pub const TEST_TABLE: &str = "TEST_CUST";

/// Backend the harness drives. Set via `BTR_TEST_BACKEND` env
/// (`mssql` | `sqlite` | `postgres`). Defaults to `mssql` for back-compat.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HarnessBackend {
    Mssql,
    Sqlite,
    Postgres,
}

pub fn current_backend() -> HarnessBackend {
    match std::env::var("BTR_TEST_BACKEND")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "sqlite" | "sqlite3" => HarnessBackend::Sqlite,
        "postgres" | "postgresql" | "pg" => HarnessBackend::Postgres,
        _ => HarnessBackend::Mssql,
    }
}

/// Path to the per-process SQLite fixture file (when running on the
/// SQLite backend). Stable across `reset_fixture()` calls within a single
/// test binary so that the wxbtrv.db config can point at it.
fn sqlite_fixture_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = tempdir();
        dir.join("wxbtrv-test.sqlite")
    })
    .clone()
}

/// ODBC conn string targeting `master` — used only for schema bootstrap.
pub fn bootstrap_connection_string() -> String {
    let server = std::env::var("BTR_TEST_SERVER").unwrap_or_else(|_| "localhost,1433".into());
    let user = std::env::var("BTR_TEST_USER").unwrap_or_else(|_| "sa".into());
    let pass = std::env::var("BTR_TEST_PASS").unwrap_or_else(|_| "WxTest!2024".into());
    format!(
        "Driver={{ODBC Driver 17 for SQL Server}};Server={server};Database=master;UID={user};PWD={pass};TrustServerCertificate=yes;"
    )
}

/// Short form used by wxbtrv-core's runtime connection.
pub fn test_connection_string() -> String {
    bootstrap_connection_string().replace("Database=master", "Database=WXBTRV_TEST")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn schema_sql_path() -> PathBuf {
    fixtures_dir().join("schema.sql")
}

fn schema_sqlite_path() -> PathBuf {
    fixtures_dir().join("schema_sqlite.sql")
}

fn schema_postgres_path() -> PathBuf {
    fixtures_dir().join("schema_postgres.sql")
}

/// Postgres connection settings, picked from env. Defaults match the
/// docker container in scripts/test-setup-postgres.sh:
///   host=localhost  port=15432  user=postgres  password=WxTest2024
///   db=wxbtrv_test
fn pg_conn_str() -> String {
    let host = std::env::var("BTR_PG_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("BTR_PG_PORT").unwrap_or_else(|_| "15432".into());
    let user = std::env::var("BTR_PG_USER").unwrap_or_else(|_| "postgres".into());
    let pass = std::env::var("BTR_PG_PASS").unwrap_or_else(|_| "WxTest2024".into());
    let db = std::env::var("BTR_PG_DB").unwrap_or_else(|_| "wxbtrv_test".into());
    format!("host={host} port={port} user={user} password={pass} dbname={db}")
}

/// Reset the Postgres fixture: apply schema_postgres.sql via the postgres
/// crate (with NoTls — local docker test container).
pub fn reset_postgres_fixture() {
    wxbtrv_core::sql::reset_connection();
    let mut client = postgres::Client::connect(&pg_conn_str(), postgres::NoTls)
        .expect("connect to test postgres");
    let sql = std::fs::read_to_string(schema_postgres_path())
        .expect("failed to read fixtures/schema_postgres.sql");
    client.batch_execute(&sql).expect("apply postgres fixture");
    drop(client);
}

/// Reset the SQL Server fixture: run schema.sql against master.
pub fn reset_sql_fixture() {
    // Drop any ODBC connection cached by wxbtrv-core from a prior test so the
    // DROP DATABASE in schema.sql isn't blocked by a lingering session.
    wxbtrv_core::sql::reset_connection();
    let cs = bootstrap_connection_string();
    let sql =
        std::fs::read_to_string(schema_sql_path()).expect("failed to read fixtures/schema.sql");

    // odbc-api Connection::execute runs one statement — split on "GO"
    // lines (case-insensitive, trimmed).
    let env = odbc_api::Environment::new().expect("ODBC env");

    // Connect with retry — the server may briefly be unavailable between runs.
    let mut conn = None;
    for attempt in 0..10 {
        match env.connect_with_connection_string(&cs, odbc_api::ConnectionOptions::default()) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) if attempt < 9 => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => panic!("connect to master failed: {e}"),
        }
    }
    let conn = conn.expect("connect to master");

    let mut batch = String::new();
    let mut batches: Vec<String> = Vec::new();
    for line in sql.lines() {
        if line.trim().eq_ignore_ascii_case("GO") {
            if !batch.trim().is_empty() {
                batches.push(std::mem::take(&mut batch));
            }
        } else {
            batch.push_str(line);
            batch.push('\n');
        }
    }
    if !batch.trim().is_empty() {
        batches.push(batch);
    }

    for b in batches {
        // SQL Server briefly reports "database may not be activated yet or may
        // be in transition" (err 913) right after a DROP + CREATE DATABASE.
        // Retry the batch a handful of times with a short sleep before panicking.
        // SQL Server briefly reports transient errors after DROP + CREATE
        // DATABASE (913 "in transition", 3701 "does not exist", connection
        // terminated). Retry the batch a handful of times before panicking.
        let mut last_err = None;
        for attempt in 0..15 {
            match conn.execute(&b, ()) {
                Ok(_) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    let msg = format!("{e}");
                    last_err = Some(msg.clone());
                    // Retry on any error — schema.sql batches are idempotent
                    // (drop-if-exists guards, CREATE DATABASE will fail if it
                    // already exists, which is itself caught next iteration).
                    let _ = msg;
                    if true {
                        std::thread::sleep(std::time::Duration::from_millis(
                            200 * (attempt + 1) as u64,
                        ));
                        continue;
                    }
                    break;
                }
            }
        }
        if let Some(e) = last_err {
            panic!("schema.sql batch failed: {e}\n--- sql ---\n{b}");
        }
    }
    drop(conn);
}

/// Build a fresh wxbtrv.db SQLite file in a temp directory, with schema
/// populated to describe TEST_CUST against the fixture SQL Server.
pub fn build_fixture_wxbtrv_db() -> PathBuf {
    let dir = tempdir();
    let db_path = dir.join("wxbtrv.db");
    let _ = std::fs::remove_file(&db_path);
    let conn = Connection::open(&db_path).expect("open sqlite");
    conn.execute_batch(
        r#"
        CREATE TABLE config (
            id      INTEGER PRIMARY KEY,
            section TEXT NOT NULL,
            key     TEXT NOT NULL,
            value   TEXT NOT NULL DEFAULT '',
            UNIQUE(section, key)
        );
        CREATE TABLE btr_tables (
            id                    INTEGER PRIMARY KEY,
            table_name            TEXT NOT NULL,
            schema_name           TEXT NOT NULL DEFAULT '',
            db_name               TEXT NOT NULL DEFAULT '',
            record_length         INTEGER NOT NULL DEFAULT 0,
            page_size             INTEGER NOT NULL DEFAULT 4096,
            file_flags            INTEGER NOT NULL DEFAULT 0,
            ignore_null_values    INTEGER NOT NULL DEFAULT 0,
            trim_string_fields    INTEGER NOT NULL DEFAULT 0,
            translate_oem_to_ansi INTEGER NOT NULL DEFAULT 0,
            primary_index         INTEGER,
            local_cache           INTEGER NOT NULL DEFAULT 0,
            driver_name           TEXT NOT NULL DEFAULT '',
            server_name           TEXT NOT NULL DEFAULT '',
            permanent_int         INTEGER NOT NULL DEFAULT 0,
            number_df_fields      INTEGER NOT NULL DEFAULT 0,
            source_file           TEXT NOT NULL DEFAULT '',
            source_path           TEXT NOT NULL DEFAULT '',
            source_dir            TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE btr_fields (
            id            INTEGER PRIMARY KEY,
            table_id      INTEGER NOT NULL,
            field_number  INTEGER NOT NULL,
            field_name    TEXT NOT NULL,
            native_type   INTEGER NOT NULL DEFAULT 0,
            native_length INTEGER NOT NULL DEFAULT 0,
            native_offset INTEGER NOT NULL DEFAULT 0,
            field_index   INTEGER,
            default_value TEXT
        );
        CREATE TABLE btr_indexes (
            id            INTEGER PRIMARY KEY,
            table_id      INTEGER NOT NULL,
            index_number  INTEGER NOT NULL,
            num_segments  INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE btr_index_segs (
            id           INTEGER PRIMARY KEY,
            index_id     INTEGER NOT NULL,
            position     INTEGER NOT NULL,
            field_number INTEGER NOT NULL,
            attrs        INTEGER NOT NULL DEFAULT 0,
            descending   INTEGER NOT NULL DEFAULT 0,
            null_value   INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .expect("schema");

    // The harness still records a SERVER value in the table-row metadata
    // even on SQLite (it's just text in btr_tables.server_name). For
    // SQLite we use the .sqlite path; for MSSQL/Postgres the host string.
    let server = match current_backend() {
        HarnessBackend::Mssql => {
            std::env::var("BTR_TEST_SERVER").unwrap_or_else(|_| "localhost,1433".into())
        }
        HarnessBackend::Sqlite => sqlite_fixture_path().to_string_lossy().into_owned(),
        HarnessBackend::Postgres => {
            let host = std::env::var("BTR_PG_HOST").unwrap_or_else(|_| "localhost".into());
            let port = std::env::var("BTR_PG_PORT").unwrap_or_else(|_| "15432".into());
            format!("{host}:{port}")
        }
    };

    let cfg_owned: Vec<(&'static str, String)> = match current_backend() {
        HarnessBackend::Mssql => {
            let user = std::env::var("BTR_TEST_USER").unwrap_or_else(|_| "sa".into());
            let pass = std::env::var("BTR_TEST_PASS").unwrap_or_else(|_| "WxTest!2024".into());
            vec![
                ("BACKEND", "mssql".to_string()),
                ("DRIVER", "ODBC Driver 17 for SQL Server".to_string()),
                ("SERVER", server.clone()),
                ("DATABASE", TEST_DB_NAME.to_string()),
                ("SCHEMA", "dbo".to_string()),
                ("USER", user),
                ("PASSWORD", pass),
                ("TRUSTED_CONNECTION", "no".to_string()),
                ("ENCRYPT", "no".to_string()),
                ("TRUST_SERVER_CERTIFICATE", "yes".to_string()),
                ("NETWORK", "".to_string()),
                ("RECNUM_COLUMN", "MDS_RECNUM".to_string()),
            ]
        }
        HarnessBackend::Sqlite => {
            // SQLite has no logical database namespace; DATABASE is the
            // .sqlite file path.
            vec![
                ("BACKEND", "sqlite".to_string()),
                ("DATABASE", server.clone()),
                ("RECNUM_COLUMN", "MDS_RECNUM".to_string()),
            ]
        }
        HarnessBackend::Postgres => {
            let user = std::env::var("BTR_PG_USER").unwrap_or_else(|_| "postgres".into());
            let pass = std::env::var("BTR_PG_PASS").unwrap_or_else(|_| "WxTest2024".into());
            let db = std::env::var("BTR_PG_DB").unwrap_or_else(|_| "wxbtrv_test".into());
            vec![
                ("BACKEND", "postgres".to_string()),
                ("SERVER", server.clone()),
                ("DATABASE", db),
                ("USER", user),
                ("PASSWORD", pass),
                ("ENCRYPT", "no".to_string()),
                ("TRUST_SERVER_CERTIFICATE", "no".to_string()),
                ("RECNUM_COLUMN", "MDS_RECNUM".to_string()),
            ]
        }
    };
    for (k, v) in cfg_owned {
        conn.execute(
            "INSERT INTO config (section, key, value) VALUES ('config', ?1, ?2)",
            params![k, v],
        )
        .unwrap();
    }

    // Record layout (total 73 bytes):
    //  off  0  CUST_ID    STRING  8   (index 1, unique)
    //  off  8  CUST_NAME  STRING  30  (index 2, dup)
    //  off 38  CITY       STRING  20
    //  off 58  STATE      STRING  2
    //  off 60  BALANCE    DECIMAL 8   (i64 LE — wxbtrv-core treats DECIMAL as int)
    //  off 68  ACTIVE     LOGICAL 1
    //  off 69  CREATED    DATE    4
    let (schema_name, db_name) = match current_backend() {
        HarnessBackend::Mssql => ("dbo", TEST_DB_NAME),
        HarnessBackend::Sqlite => ("", ""),
        HarnessBackend::Postgres => ("public", ""),
    };
    conn.execute(
        "INSERT INTO btr_tables
            (table_name, schema_name, db_name, record_length, page_size, file_flags,
             ignore_null_values, trim_string_fields, translate_oem_to_ansi,
             primary_index, local_cache, driver_name, server_name,
             permanent_int, number_df_fields, source_file, source_path, source_dir)
         VALUES (?1, ?4, ?2, 73, 4096, 0, 1, 1, 0, NULL, 0, 'SQL_BTR', ?3, 0, 7, '', '', '')",
        params![TEST_TABLE, db_name, server, schema_name],
    )
    .unwrap();
    let table_id = conn.last_insert_rowid();

    // native_type constants (see wxbtrv-core/src/record.rs):
    //   0=STRING 3=DATE 5=DECIMAL 7=LOGICAL
    let fields: [(u32, &str, i32, u32, u32); 7] = [
        (1, "CUST_ID", 0, 8, 0),
        (2, "CUST_NAME", 0, 30, 8),
        (3, "CITY", 0, 20, 38),
        (4, "STATE", 0, 2, 58),
        (5, "BALANCE", 5, 8, 60),
        (6, "ACTIVE", 7, 1, 68),
        (7, "CREATED", 3, 4, 69),
    ];
    for (num, name, ty, len, off) in fields {
        conn.execute(
            "INSERT INTO btr_fields (table_id, field_number, field_name, native_type, native_length, native_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![table_id, num, name, ty, len, off],
        ).unwrap();
    }

    // Index 1: CUST_ID unique  (attrs bit: simple int 0 is fine)
    conn.execute(
        "INSERT INTO btr_indexes (table_id, index_number, num_segments) VALUES (?1, 1, 1)",
        params![table_id],
    )
    .unwrap();
    let ix1 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO btr_index_segs (index_id, position, field_number, attrs, descending, null_value)
         VALUES (?1, 0, 1, 0, 0, 0)",
        params![ix1],
    )
    .unwrap();

    // Index 2: CUST_NAME dup
    conn.execute(
        "INSERT INTO btr_indexes (table_id, index_number, num_segments) VALUES (?1, 2, 1)",
        params![table_id],
    )
    .unwrap();
    let ix2 = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO btr_index_segs (index_id, position, field_number, attrs, descending, null_value)
         VALUES (?1, 0, 2, 0, 0, 0)",
        params![ix2],
    )
    .unwrap();

    // ── extra fixture tables ────────────────────────────────────────────────
    add_test_multi(&conn, &server);
    add_test_autoinc(&conn, &server);
    add_test_types(&conn, &server);
    add_test_desc(&conn, &server);

    drop(conn);
    db_path
}

// Helper: insert a TableHeader row + return its row id.
#[allow(clippy::too_many_arguments)]
fn insert_table(
    conn: &Connection,
    table_name: &str,
    server: &str,
    record_length: u32,
    primary_index: Option<i64>,
    number_df_fields: i64,
) -> i64 {
    let (schema_name, db_name) = match current_backend() {
        HarnessBackend::Mssql => ("dbo", TEST_DB_NAME),
        HarnessBackend::Sqlite => ("", ""),
        HarnessBackend::Postgres => ("public", ""),
    };
    conn.execute(
        "INSERT INTO btr_tables
            (table_name, schema_name, db_name, record_length, page_size, file_flags,
             ignore_null_values, trim_string_fields, translate_oem_to_ansi,
             primary_index, local_cache, driver_name, server_name,
             permanent_int, number_df_fields, source_file, source_path, source_dir)
         VALUES (?1, ?7, ?2, ?3, 4096, 0, 1, 1, 0, ?4, 0, 'SQL_BTR', ?5, 0, ?6, '', '', '')",
        params![
            table_name,
            db_name,
            record_length,
            primary_index,
            server,
            number_df_fields,
            schema_name
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn insert_field(
    conn: &Connection,
    table_id: i64,
    num: u32,
    name: &str,
    native_type: i32,
    length: u32,
    offset: u32,
) {
    conn.execute(
        "INSERT INTO btr_fields (table_id, field_number, field_name, native_type, native_length, native_offset)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![table_id, num, name, native_type, length, offset],
    ).unwrap();
}

/// Insert a single index with N segments. `segs` is &[(field_number, descending)].
fn insert_index(conn: &Connection, table_id: i64, index_number: i64, segs: &[(u32, bool)]) {
    conn.execute(
        "INSERT INTO btr_indexes (table_id, index_number, num_segments) VALUES (?1, ?2, ?3)",
        params![table_id, index_number, segs.len() as i64],
    )
    .unwrap();
    let ix_id = conn.last_insert_rowid();
    for (pos, (fnum, desc)) in segs.iter().enumerate() {
        conn.execute(
            "INSERT INTO btr_index_segs (index_id, position, field_number, attrs, descending, null_value)
             VALUES (?1, ?2, ?3, 0, ?4, 0)",
            params![ix_id, pos as i64, fnum, if *desc { 1i64 } else { 0i64 }],
        )
        .unwrap();
    }
}

// TEST_MULTI layout: REGION(4) DEPT(4) SUB_CODE(6) VALUE(4) — total 18 bytes
fn add_test_multi(conn: &Connection, server: &str) {
    let id = insert_table(conn, "TEST_MULTI", server, 18, None, 4);
    insert_field(conn, id, 1, "REGION", 0, 4, 0);
    insert_field(conn, id, 2, "DEPT", 0, 4, 4);
    insert_field(conn, id, 3, "SUB_CODE", 0, 6, 8);
    insert_field(conn, id, 4, "VALUE", 1, 4, 14);
    // Index 1: compound (REGION, DEPT, SUB_CODE) ascending — 14 bytes
    insert_index(conn, id, 1, &[(1, false), (2, false), (3, false)]);
    // Index 2: VALUE descending — 4 bytes
    insert_index(conn, id, 2, &[(4, true)]);
}

// TEST_AUTOINC layout: AUTO_ID(4 autoinc) NAME(20) — total 24 bytes
fn add_test_autoinc(conn: &Connection, server: &str) {
    // primary_index = 0 → resolves to RuntimeIndex.num == 1 (the AUTO_ID index),
    // which causes TableMeta::new to set recnum_col = "AUTO_ID".
    let id = insert_table(conn, "TEST_AUTOINC", server, 24, Some(0), 2);
    insert_field(conn, id, 1, "AUTO_ID", 15, 4, 0);
    insert_field(conn, id, 2, "NAME", 0, 20, 4);
    insert_index(conn, id, 1, &[(1, false)]);
}

// TEST_TYPES layout:
//   STR_FIX  off  0 len 10 type  0
//   STR_Z    off 10 len 16 type 11
//   INT_VAL  off 26 len  4 type  1
//   DEC_VAL  off 30 len  8 type  5
//   LOG_VAL  off 38 len  1 type  7
//   DATE_VAL off 39 len  4 type  3
// total record_length = 43. SQL identity is MDS_RECNUM (default recnum_col).
fn add_test_types(conn: &Connection, server: &str) {
    let id = insert_table(conn, "TEST_TYPES", server, 43, None, 6);
    insert_field(conn, id, 1, "STR_FIX", 0, 10, 0);
    insert_field(conn, id, 2, "STR_Z", 11, 16, 10);
    insert_field(conn, id, 3, "INT_VAL", 1, 4, 26);
    insert_field(conn, id, 4, "DEC_VAL", 5, 8, 30);
    insert_field(conn, id, 5, "LOG_VAL", 7, 1, 38);
    insert_field(conn, id, 6, "DATE_VAL", 3, 4, 39);
    // Index 1: STR_FIX unique
    insert_index(conn, id, 1, &[(1, false)]);
    // Index 2: INT_VAL (duplicates allowed)
    insert_index(conn, id, 2, &[(3, false)]);
}

// TEST_DESC layout: RANK_VAL(4) LABEL(8) — total 12 bytes
fn add_test_desc(conn: &Connection, server: &str) {
    let id = insert_table(conn, "TEST_DESC", server, 12, None, 2);
    insert_field(conn, id, 1, "RANK_VAL", 1, 4, 0);
    insert_field(conn, id, 2, "LABEL", 0, 8, 4);
    // Index 1: RANK_VAL DESC
    insert_index(conn, id, 1, &[(1, true)]);
}

/// Reset the SQLite fixture: drop + recreate the per-process .sqlite file
/// from `fixtures/schema_sqlite.sql`. Idempotent.
pub fn reset_sqlite_fixture() {
    // Drop any cached connection so a previous test's open file handle
    // doesn't pin the schema.
    wxbtrv_core::sql::reset_connection();
    let path = sqlite_fixture_path();
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).expect("open sqlite fixture");
    let sql = std::fs::read_to_string(schema_sqlite_path())
        .expect("failed to read fixtures/schema_sqlite.sql");
    conn.execute_batch(&sql).expect("apply sqlite fixture");
    drop(conn);
}

/// Combined reset: (1) drop+recreate the backend-specific fixture,
/// (2) build wxbtrv.db, (3) install it into wxbtrv-core's state.
/// Returns the wxbtrv.db path.
pub fn reset_fixture() -> PathBuf {
    match current_backend() {
        HarnessBackend::Mssql => reset_sql_fixture(),
        HarnessBackend::Sqlite => reset_sqlite_fixture(),
        HarnessBackend::Postgres => reset_postgres_fixture(),
    }
    let db = build_fixture_wxbtrv_db();
    install_fixture_config(&db);
    db
}

/// Point wxbtrv-core at a pre-built wxbtrv.db. Because wxbtrv-core's
/// `find_sqlite_db()` searches CWD first, we chdir to the db's parent
/// directory (inside a process-global mutex to avoid races).
pub fn install_fixture_config(wxbtrv_db_path: &Path) {
    static CHDIR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _g = CHDIR_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    wxbtrv_core::state::reset_for_tests();
    if let Some(dir) = wxbtrv_db_path.parent() {
        std::env::set_current_dir(dir).expect("chdir to fixture dir");
    }
}

pub fn new_posblk() -> Box<[u8; 128]> {
    Box::new([0u8; 128])
}

fn tempdir() -> PathBuf {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = base.join(format!("btr-harness-{pid}-{nonce}"));
    std::fs::create_dir_all(&d).expect("mkdir temp");
    d
}

/// Issue Open (op 0) for a path. Returns the posblk and the rc.
pub fn fixture_open(path: &str) -> (Box<[u8; 128]>, i32) {
    let mut posblk = new_posblk();
    let mut key_buf = [0u8; 260];
    let bytes = path.as_bytes();
    key_buf[..bytes.len()].copy_from_slice(bytes);
    let mut dlen: u32 = 0;
    let rc = unsafe {
        wxbtrv_core::ops::btrcall_internal(
            0,
            posblk.as_mut_ptr() as *mut core::ffi::c_void,
            std::ptr::null_mut(),
            &mut dlen as *mut u32,
            key_buf.as_mut_ptr() as *mut core::ffi::c_void,
            0,
            std::ptr::null_mut(),
        )
    };
    (posblk, rc)
}

/// Thin wrapper around btrcall_internal — a single call with full args.
#[allow(clippy::too_many_arguments)]
pub fn btrcall(
    op: u16,
    posblk: &mut [u8; 128],
    data_buf: &mut [u8],
    data_len: &mut u32,
    key_buf: &mut [u8],
    key_num: i16,
) -> i32 {
    unsafe {
        wxbtrv_core::ops::btrcall_internal(
            op,
            posblk.as_mut_ptr() as *mut core::ffi::c_void,
            data_buf.as_mut_ptr() as *mut core::ffi::c_void,
            data_len as *mut u32,
            key_buf.as_mut_ptr() as *mut core::ffi::c_void,
            key_num,
            std::ptr::null_mut(),
        )
    }
}
