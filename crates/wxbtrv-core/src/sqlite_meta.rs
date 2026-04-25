use crate::state::{state, IndexSpec, IntField, TableHeader, TableMeta};
use crate::trace::trace;
use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::path::PathBuf;

fn normalize_key(s: &str) -> String {
    s.trim().to_ascii_uppercase().replace(['\\', '/', '.'], "_")
}

fn table_key_variants(table_name: &str) -> Vec<String> {
    let t = table_name.trim().to_ascii_uppercase();
    let mut keys = vec![
        t.clone(),
        format!("{t}.B"),
        format!("{t}_B"),
        normalize_key(&t),
    ];
    keys.sort();
    keys.dedup();
    keys
}

pub fn find_sqlite_db() -> Option<PathBuf> {
    let filename = "wxbtrv.db";
    // Search order:
    //   1. Runtime WXBTRV_CONFIG_DIR env var (operators can override without rebuilding)
    //   2. Current working directory
    //   3. Directory containing wxbtrv.dll (Windows only)
    //   4. Compile-time WXBTRV_CONFIG_DIR fallback baked into the binary
    //   5. C:\WatkinsX\bin (our own production install location)
    let dirs: Vec<PathBuf> = [
        std::env::var("WXBTRV_CONFIG_DIR").ok().map(PathBuf::from),
        std::env::current_dir().ok(),
        crate::util::dll_dir(),
        Some(PathBuf::from(env!("WXBTRV_CONFIG_DIR"))),
        Some(PathBuf::from(r"C:\WatkinsX\bin")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in dirs {
        let p = dir.join(filename);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn load_fields(conn: &Connection, table_id: i64) -> rusqlite::Result<Vec<IntField>> {
    let mut stmt = conn.prepare(
        "SELECT field_number, field_name, native_type, native_length, native_offset,
                field_index, default_value
         FROM btr_fields WHERE table_id = ?1 ORDER BY field_number",
    )?;
    let rows = stmt.query_map(params![table_id], |row| {
        Ok(IntField {
            num: row.get::<_, u32>(0)?,
            name: row.get(1)?,
            native_type: row.get(2)?,
            length: row.get::<_, u32>(3)?,
            offset: row.get::<_, u32>(4)?,
            field_index: row.get(5)?,
            default_value: row.get(6)?,
        })
    })?;
    rows.collect()
}

fn load_indexes(conn: &Connection, table_id: i64) -> rusqlite::Result<Vec<IndexSpec>> {
    let mut stmt = conn.prepare(
        "SELECT id, index_number
         FROM btr_indexes WHERE table_id = ?1 ORDER BY index_number",
    )?;
    let index_rows: Vec<(i64, u32)> = stmt
        .query_map(params![table_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut indexes = Vec::new();
    let mut seg_stmt = conn.prepare(
        "SELECT field_number, attrs, descending, COALESCE(null_value, 0)
         FROM btr_index_segs WHERE index_id = ?1 ORDER BY position",
    )?;
    for (idx_id, num) in index_rows {
        let segments: Vec<(u32, u16, bool, u8)> = seg_stmt
            .query_map(params![idx_id], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u16>(1)?,
                    row.get::<_, i32>(2)? != 0,
                    row.get::<_, i32>(3)? as u8,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut spec = IndexSpec {
            num,
            field_nums: Vec::with_capacity(segments.len()),
            attrs: Vec::with_capacity(segments.len()),
            desc: Vec::with_capacity(segments.len()),
            null_values: Vec::with_capacity(segments.len()),
        };
        for (fnum, attrs, descending, null_value) in segments {
            spec.field_nums.push(fnum);
            spec.attrs.push(attrs);
            spec.desc.push(descending);
            spec.null_values.push(null_value);
        }
        indexes.push(spec);
    }
    Ok(indexes)
}

/// A raw row loaded from `btr_tables` plus its fields and index specs.
struct RawTable {
    header: TableHeader,
    fields: Vec<IntField>,
    indexes: Vec<IndexSpec>,
}

fn load_all_tables(conn: &Connection) -> rusqlite::Result<Vec<RawTable>> {
    let mut stmt = conn.prepare(
        "SELECT id, table_name, schema_name, db_name,
                record_length, page_size, file_flags,
                ignore_null_values, trim_string_fields, translate_oem_to_ansi,
                primary_index, local_cache
         FROM btr_tables ORDER BY table_name",
    )?;

    let table_rows: Vec<(i64, TableHeader)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                TableHeader {
                    table_name: row.get(1)?,
                    schema_name: row.get(2)?,
                    db_name: row.get(3)?,
                    record_length: row.get::<_, u32>(4)?,
                    page_size: row.get::<_, u16>(5)?,
                    file_flags: row.get::<_, u16>(6)?,
                    ignore_null_values: row.get::<_, i32>(7)? != 0,
                    trim_string_fields: row.get::<_, i32>(8)? != 0,
                    translate_oem_to_ansi: row.get::<_, i32>(9)? != 0,
                    primary_index: row.get(10)?,
                    local_cache: row.get::<_, i32>(11)? != 0,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out = Vec::new();
    for (id, header) in table_rows {
        let fields = load_fields(conn, id)?;
        let indexes = load_indexes(conn, id)?;
        out.push(RawTable {
            header,
            fields,
            indexes,
        });
    }
    Ok(out)
}

fn load_connection_from_sqlite(conn: &Connection) {
    let get = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM config WHERE section = 'config' AND key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    let get_bool = |key: &str| -> bool {
        matches!(get(key).to_ascii_lowercase().as_str(), "yes" | "true" | "1")
    };

    let Ok(mut st) = state().lock() else { return };
    st.server = get("SERVER");
    st.database = get("DATABASE");
    st.schema = get("SCHEMA");
    st.driver = get("DRIVER");
    st.network = get("NETWORK");
    st.user = get("USER");
    st.pass = get("PASSWORD");
    st.trusted_connection = get_bool("TRUSTED_CONNECTION");
    st.encrypt = get_bool("ENCRYPT");
    st.trust_server_certificate = get_bool("TRUST_SERVER_CERTIFICATE");
    let recnum = get("RECNUM_COLUMN");
    st.recnum_col_default = if recnum.is_empty() {
        "MDS_RECNUM".to_string()
    } else {
        recnum
    };

    // Load per-db overrides: keys matching "RECNUM_COLUMN.<DBNAME>"
    if let Ok(mut rows) = conn
        .prepare(
            "SELECT key, value FROM config WHERE section = 'config' AND key LIKE 'RECNUM_COLUMN.%'",
        )
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map(|iter| iter.flatten().collect::<Vec<_>>())
        })
    {
        for (key, val) in rows.drain(..) {
            if let Some(db) = key.strip_prefix("RECNUM_COLUMN.") {
                st.recnum_col_by_db.insert(db.to_ascii_uppercase(), val);
            }
        }
    }
    // Load per-directory config overrides (sections other than 'config')
    if let Ok(mut rows) = conn
        .prepare("SELECT section, key, value FROM config WHERE section != 'config'")
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map(|iter| iter.flatten().collect::<Vec<_>>())
        })
    {
        for (section, key, val) in rows.drain(..) {
            let dir = section.to_ascii_uppercase();
            st.dir_configs
                .entry(dir)
                .or_default()
                .insert(key.to_ascii_uppercase(), val);
        }
    }

    trace(&format!(
        "sqlite_meta: server={} db={} driver={:?} network={:?} user={} trusted={} encrypt={} trust_cert={}",
        st.server, st.database, st.driver, st.network, st.user,
        st.trusted_connection, st.encrypt, st.trust_server_certificate
    ));
    let dirs: Vec<_> = st.dir_configs.keys().cloned().collect();
    if !dirs.is_empty() {
        trace(&format!("sqlite_meta: dir_configs={:?}", dirs));
    }
}

/// Load all table metadata from the SQLite database into the driver state.
/// Returns Some(count) of unique table name variants registered, or None if no DB was found.
pub fn preload_from_sqlite() -> Option<usize> {
    let db_path = find_sqlite_db()?;
    trace(&format!(
        "sqlite_meta: opening {}",
        db_path.to_string_lossy()
    ));

    let conn = match Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            trace(&format!("sqlite_meta: open failed: {e}"));
            return None;
        }
    };

    load_connection_from_sqlite(&conn);

    let raw_tables = match load_all_tables(&conn) {
        Ok(f) => f,
        Err(e) => {
            trace(&format!("sqlite_meta: query failed: {e}"));
            return None;
        }
    };

    // Get the global schema from connection config — used as fallback for tables
    // whose INT file did not specify a SCHEMA_NAME.
    let global_schema = state()
        .lock()
        .ok()
        .map(|s| s.schema.clone())
        .unwrap_or_default();

    let (recnum_default, recnum_by_db) = state()
        .lock()
        .ok()
        .map(|s| (s.recnum_col_default.clone(), s.recnum_col_by_db.clone()))
        .unwrap_or_else(|| ("MDS_RECNUM".to_string(), HashMap::new()));
    trace(&format!(
        "sqlite_meta: recnum_col_default={:?} per_db_overrides={:?}",
        recnum_default, recnum_by_db
    ));

    let mut table_map: HashMap<String, TableMeta> = HashMap::new();
    for RawTable {
        mut header,
        fields,
        indexes,
    } in raw_tables
    {
        // Apply schema fallback: if the table has no schema, use the connection schema.
        if header.schema_name.is_empty() && !global_schema.is_empty() {
            header.schema_name = global_schema.clone();
        }
        // Resolve recnum column: per-db override > global default
        let recnum = recnum_by_db
            .get(&header.db_name.to_ascii_uppercase())
            .map(|s| s.as_str())
            .unwrap_or(&recnum_default);
        let tm = TableMeta::new(header, fields, indexes, recnum);
        let uname = tm.table_name.to_ascii_uppercase();
        let db_upper = tm.db_name.to_ascii_uppercase();
        // Insert db-qualified keys (e.g. "GPACIFIC:BKHELP") so we can pick the right
        // variant when the open path tells us the directory (PACIFIC → GPacific).
        for k in table_key_variants(&uname) {
            table_map.insert(format!("{}:{}", db_upper, k), tm.clone());
        }
        table_map.insert(format!("{}:{}", db_upper, &uname), tm.clone());
        // Also insert plain name — last variant wins as fallback
        for k in table_key_variants(&uname) {
            table_map.insert(k, tm.clone());
        }
        table_map.insert(uname, tm);
    }

    let count = table_map.len();
    if let Ok(mut st) = state().lock() {
        st.tables = table_map;
        st.sqlite_db = db_path.to_string_lossy().into_owned();
    }
    trace(&format!("sqlite_meta: loaded {} table variants", count));
    Some(count)
}
