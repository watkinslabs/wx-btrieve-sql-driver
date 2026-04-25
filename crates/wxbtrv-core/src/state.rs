use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

pub static LAST_ERROR: AtomicI32 = AtomicI32::new(0);
pub static INIT_COUNT: AtomicU32 = AtomicU32::new(0);
pub static IS_INITIALIZED: AtomicBool = AtomicBool::new(false);
pub static CALL_COUNTER: AtomicU32 = AtomicU32::new(0);

// ── Schema types ──────────────────────────────────────────────────────────────

/// Runtime field metadata. Mirrors the shape of a parsed INT field but is
/// defined here so wxbtrv-core has no dependency on the INT parser crate.
#[derive(Clone, Debug)]
pub struct IntField {
    pub num: u32,
    pub name: String,
    pub native_type: i32,
    pub length: u32,
    pub offset: u32,
    pub field_index: Option<u32>,
    pub default_value: Option<String>,
}

/// Index in parallel-array form — field_nums[i]/attrs[i]/desc[i]/null_values[i] are co-indexed.
/// This is the hot-path representation used inside the DLL for fast key-length lookup.
#[derive(Clone, Debug, Default)]
pub struct RuntimeIndex {
    pub num: u32,
    pub field_nums: Vec<u32>,
    pub attrs: Vec<u16>,
    pub desc: Vec<bool>,
    /// Per-segment null-byte value (INDEX_SEGMENT_NULL_VALUE).
    /// A key segment whose bytes are all equal to this value is treated as a null wildcard
    /// when IGNORE_NULL_VALUES is set.
    pub null_values: Vec<u8>,
    pub key_len: u32,
}

#[derive(Clone, Debug)]
pub struct TableMeta {
    pub table_name: String,
    pub schema_name: String,
    pub db_name: String,
    pub record_length: u32,
    pub page_size: u16,
    pub file_flags: u16,
    pub fields: Vec<IntField>,
    pub indexes: Vec<RuntimeIndex>,
    pub recnum_col: String,
    pub ignore_null_values: bool,
    pub trim_string_fields: bool,
    pub translate_oem_to_ansi: bool,
    pub primary_index: Option<u32>,
    pub local_cache: bool,
}

/// Header fields for a TableMeta, passed to `TableMeta::new` as raw inputs.
/// Same shape as the SQLite `btr_tables` row projection, no INT file required.
#[derive(Clone, Debug, Default)]
pub struct TableHeader {
    pub table_name: String,
    pub schema_name: String,
    pub db_name: String,
    pub record_length: u32,
    pub page_size: u16,
    pub file_flags: u16,
    pub ignore_null_values: bool,
    pub trim_string_fields: bool,
    pub translate_oem_to_ansi: bool,
    pub primary_index: Option<u32>,
    pub local_cache: bool,
}

/// One index specification, raw form. Built directly from the
/// `btr_indexes`/`btr_index_segs` SQLite rows — no INT parser types involved.
#[derive(Clone, Debug, Default)]
pub struct IndexSpec {
    pub num: u32,
    pub field_nums: Vec<u32>,
    pub attrs: Vec<u16>,
    pub desc: Vec<bool>,
    pub null_values: Vec<u8>,
}

impl TableMeta {
    /// Build a TableMeta from raw inputs (header + fields + index specs).
    /// `recnum_default` is the SQL Server identity column name to use when the
    /// table has no PRIMARY_INDEX (e.g. "MDS_RECNUM" for legacy tables).
    pub fn new(
        header: TableHeader,
        fields: Vec<IntField>,
        index_specs: Vec<IndexSpec>,
        recnum_default: &str,
    ) -> Self {
        let indexes: Vec<RuntimeIndex> = index_specs
            .into_iter()
            .map(|ix| {
                let key_len: u32 = ix
                    .field_nums
                    .iter()
                    .filter_map(|fnum| fields.iter().find(|fld| fld.num == *fnum))
                    .map(|fld| fld.length)
                    .sum();
                RuntimeIndex {
                    num: ix.num,
                    field_nums: ix.field_nums,
                    attrs: ix.attrs,
                    desc: ix.desc,
                    null_values: ix.null_values,
                    key_len,
                }
            })
            .collect();

        let recnum_col = header
            .primary_index
            .and_then(|pi| indexes.iter().find(|ix| ix.num == pi + 1))
            .and_then(|ix| ix.field_nums.first().copied())
            .and_then(|fnum| fields.iter().find(|fld| fld.num == fnum))
            .map(|fld| fld.name.clone())
            .unwrap_or_else(|| recnum_default.to_string());

        TableMeta {
            table_name: header.table_name,
            schema_name: header.schema_name,
            db_name: header.db_name,
            record_length: header.record_length,
            page_size: header.page_size,
            file_flags: header.file_flags,
            fields,
            indexes,
            recnum_col,
            ignore_null_values: header.ignore_null_values,
            trim_string_fields: header.trim_string_fields,
            translate_oem_to_ansi: header.translate_oem_to_ansi,
            primary_index: header.primary_index,
            local_cache: header.local_cache,
        }
    }
}

impl TableMeta {
    pub fn index_for_key_len(&self, key_len: usize) -> Option<&RuntimeIndex> {
        if key_len == 0 {
            return None;
        }
        self.indexes
            .iter()
            .find(|ix| ix.key_len as usize == key_len)
            .or_else(|| self.indexes.first())
    }

    pub fn select_cols(&self) -> String {
        let d = crate::dialect::active();
        self.fields
            .iter()
            .map(|f| d.quote_ident(&f.name))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn recnum_is_field(&self) -> bool {
        self.fields
            .iter()
            .any(|f| f.name.eq_ignore_ascii_case(&self.recnum_col))
    }

    pub fn select_with_recnum(&self) -> String {
        if self.recnum_is_field() {
            self.select_cols()
        } else {
            let d = crate::dialect::active();
            format!("{}, {}", d.quote_ident(&self.recnum_col), self.select_cols())
        }
    }

    /// SQL expression for the row-number column used in WHERE / ORDER BY.
    pub fn recnum_sql_ref(&self) -> String {
        crate::dialect::active().quote_ident(&self.recnum_col)
    }

    pub fn table_ref(&self, db_override: &str, schema_override: &str) -> String {
        let d = crate::dialect::active();
        let backend = state().lock().map(|s| s.backend).unwrap_or_default();
        let t = d.quote_ident(&self.table_name);

        // SQLite has neither catalog nor schema — emit just the bare name.
        if backend == Backend::Sqlite {
            return t;
        }

        // Postgres has no cross-DB 3-part naming; the connection's dbname
        // is implicit. Drop the db component and emit at most schema.table.
        let allow_db = backend != Backend::Postgres;

        let db = if !allow_db {
            ""
        } else if db_override.is_empty() {
            self.db_name.as_str()
        } else {
            db_override
        };

        // Schema resolution order: explicit override > table's own schema_name >
        // global connection schema (from set-connection --schema) > omit
        let resolved;
        let sc: &str = if !schema_override.is_empty() {
            schema_override
        } else if !self.schema_name.is_empty() {
            &self.schema_name
        } else {
            resolved = state()
                .lock()
                .ok()
                .map(|s| s.schema.clone())
                .unwrap_or_default();
            &resolved
        };

        match (db.is_empty(), sc.is_empty()) {
            (true, true) => t,
            (true, false) => format!("{}.{}", d.quote_ident(sc), t),
            (false, true) => format!("{}.{}", d.quote_ident(db), t),
            (false, false) => format!("{}.{}.{}", d.quote_ident(db), d.quote_ident(sc), t),
        }
    }
}

// ── Handle types ──────────────────────────────────────────────────────────────

pub struct HandleEntry {
    pub meta: TableMeta,
    /// Path to the .B file this handle was opened from.
    pub b_path: String,

    // ── Step navigation (physical order via MDS_RECNUM) ──────────────────────
    pub step_last_recnum: Option<i64>,
    pub step_dir: i8,

    // ── Get navigation (index key order) ─────────────────────────────────────
    /// Which index (by RuntimeIndex.num) is active for Get navigation.
    pub get_index_num: Option<u32>,
    /// SQL literals for each key segment of the last returned row (parallel to index.field_nums).
    pub get_last_keys: Vec<crate::sql_param::SqlValue>,
    /// per-segment descending flags for the active index (parallel to get_last_keys).
    pub get_last_desc: Vec<bool>,
    pub get_last_recnum: Option<i64>,
    pub get_dir: i8,

    // ── Unified position for Update / Delete ──────────────────────────────────
    /// MDS_RECNUM of the most recently read row by ANY op (Step or Get).
    pub last_recnum: Option<i64>,

    // ── Step chunk cache (LOCAL_CACHE YES tables) ─────────────────────────────
    /// Buffered rows from the last chunk fetch: (recnum, packed_record).
    pub step_cache: std::collections::VecDeque<(i64, Vec<u8>)>,
    /// Direction the cache was filled: 1 = forward, -1 = backward.
    pub step_cache_dir: i8,

    // ── Row-level lock resource prefix ────────────────────────────────────────
    /// Cached "{DB_ID}.{TABLE_ID}" prefix for sp_getapplock resource strings.
    /// Matches the format used by the _adv_row_lock SP.
    /// None = not yet resolved; populated on first lock attempt.
    pub lock_prefix: Option<String>,

    // ── Btrieve owner name (op 29/30) ────────────────────────────────────────
    /// Owner name set by op 29 (SetOwner), cleared by op 30. SQL Server has no
    /// concept of file-level owner-password; we just stash it here for the log.
    pub owner_name: Option<String>,
}

// ── Backend selection ────────────────────────────────────────────────────────

/// Database backend the runtime targets. Picked from `wxbtrv.db` at startup
/// via the `[MDS] BACKEND=` key (defaults to MSSQL for back-compat).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    Mssql,
    Postgres,
    Sqlite,
}

impl Backend {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mssql" | "sqlserver" | "sql_server" | "ms_sql" | "" => Some(Backend::Mssql),
            "postgres" | "postgresql" | "pg" => Some(Backend::Postgres),
            "sqlite" | "sqlite3" => Some(Backend::Sqlite),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Mssql => "mssql",
            Backend::Postgres => "postgres",
            Backend::Sqlite => "sqlite",
        }
    }
}

// ── Driver state ──────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DriverState {
    // Backend selection (read from [MDS] BACKEND= in wxbtrv.db)
    pub backend: Backend,

    // Connection info (from wxbtrv.db [config] section — global defaults).
    // Field meaning depends on `backend`:
    //   Mssql:    server/database/driver/network/user/pass + tls flags + dsn
    //   Postgres: server (host:port) / database / user / pass + tls flags
    //   Sqlite:   `database` is the path to the .sqlite file; everything else ignored
    pub server: String,
    pub dsn: String,
    pub driver: String,  // ODBC driver name, e.g. "SQL Server Native Client 10.0"
    pub network: String, // Network library, e.g. "DBMSSOCN" or "tcp:" or "" (driver default)
    pub user: String,
    pub pass: String,
    pub database: String,
    pub schema: String,
    pub trusted_connection: bool,
    pub encrypt: bool,
    pub trust_server_certificate: bool,
    pub trim_strings: bool,
    /// Identity column name for tables without a PRIMARY_INDEX.
    /// Global fallback recnum column (default: "MDS_RECNUM").
    pub recnum_col_default: String,
    pub recnum_col_by_db: HashMap<String, String>,

    /// Per-directory config overrides. Key = uppercase directory name (e.g. "PACIFIC").
    /// Values override the global config for opens from that directory.
    pub dir_configs: HashMap<String, HashMap<String, String>>,

    /// Client current working directory as set by Btrieve op 17 (Set Directory).
    /// When an Open arrives with a relative path, we resolve config keys against
    /// this directory first before falling back to the global config. Populated
    /// by op_set_dir; read back by op_get_dir.
    pub client_cwd: Option<String>,

    // Schema cache (loaded from SQLite)
    pub sqlite_db: String,
    pub tables: HashMap<String, TableMeta>, // keyed by UPPER table name

    // Open file handles — keyed by handle ID written to posblk[0..4].
    pub handles: HashMap<u32, HandleEntry>,

    // Legacy row-cache for DBU* interface
    pub last_query: String,
    pub last_row_text: String,
    pub row_cache: Vec<String>,
    pub row_index: usize,
}

static STATE: OnceLock<Mutex<DriverState>> = OnceLock::new();

pub fn state() -> &'static Mutex<DriverState> {
    STATE.get_or_init(|| Mutex::new(DriverState::default()))
}

/// Test-support: reset all global driver state so the next BTRCALL triggers a
/// fresh `preload_from_sqlite()`. Also drops any cached ODBC connection and
/// clears transaction/handle state. Intended for integration tests only.
pub fn reset_for_tests() {
    if let Ok(mut st) = state().lock() {
        *st = DriverState::default();
    }
    IS_INITIALIZED.store(false, Ordering::Release);
    INIT_COUNT.store(0, Ordering::Release);
    LAST_ERROR.store(0, Ordering::Release);
    CALL_COUNTER.store(0, Ordering::Release);
    crate::sql::reset_connection();
    crate::sql::TXN_ACTIVE.store(false, Ordering::Release);
}

/// Resolve a config value for a given directory path, with tiered fallback:
/// 1. Per-directory config (e.g. section "PACIFIC" for G:\PACIFIC\FOO.B)
/// 2. Global config (section "config")
pub fn resolve_config(dir: &str, key: &str) -> String {
    let Ok(st) = state().lock() else {
        return String::new();
    };
    let udir = dir.to_ascii_uppercase();
    let ukey = key.to_ascii_uppercase();

    // Check directory-specific config first
    if let Some(dir_cfg) = st.dir_configs.get(&udir) {
        if let Some(val) = dir_cfg.get(&ukey) {
            return val.clone();
        }
    }

    // Fall back to global
    match ukey.as_str() {
        "DATABASE" => st.database.clone(),
        "SCHEMA" => st.schema.clone(),
        "SERVER" => st.server.clone(),
        "USER" => st.user.clone(),
        "PASSWORD" => st.pass.clone(),
        "DRIVER" => st.driver.clone(),
        "NETWORK" => st.network.clone(),
        "RECNUM_COLUMN" => st.recnum_col_default.clone(),
        _ => String::new(),
    }
}

/// Extract the full directory path from an open path as the config key.
/// "G:\PACIFIC\FOO.B" → "G:\PACIFIC"
/// "J:\ADVDATA\BAR.B" → "J:\ADVDATA"
/// "G:\PACIFIC\SUBFOLDER\BAZ.B" → "G:\PACIFIC\SUBFOLDER"
pub fn dir_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_ascii_uppercase()
}

pub fn set_err(code: i32) -> i32 {
    LAST_ERROR.store(code, Ordering::Relaxed);
    code
}

/// Look up the .B file path for a handle by its posblk linear address.
/// Returns "?" if the handle is not open.
pub fn handle_name(posblk_key: u32) -> String {
    if let Ok(st) = state().lock() {
        if let Some(h) = st.handles.get(&posblk_key) {
            return h.b_path.clone();
        }
    }
    "?".into()
}
