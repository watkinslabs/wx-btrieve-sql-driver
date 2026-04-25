/// schema.rs — read table schema and SQL Server connection config from wxbtrv.db.
use btr_types::{IndexSegment, IntField, IntFile, IntIndex};
use rusqlite::{params, Connection, OpenFlags};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SqlConfig {
    pub server: String,
    pub database: String,
    pub schema: String,
    pub user: String,
    pub password: String,
}

/// Find wxbtrv.db by searching CWD, then well-known install dirs.
pub fn find_db(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        return Err(format!("database not found: {}", p.display()));
    }
    let name = "wxbtrv.db";
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join(name)),
        Some(PathBuf::from(r"C:\WatkinsX\bin").join(name)),
        Some(PathBuf::from(r"C:\PVSW\bin").join(name)),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() {
            return Ok(c);
        }
    }
    Err(format!(
        "{} not found in current directory or C:\\WatkinsX\\bin. Run 'int-tool init' first.",
        name
    ))
}

pub fn open_db(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("cannot open {}: {}", path.display(), e))
}

/// Open wxbtrv.db in read-write mode (needed for --save-schema).
pub fn open_db_rw(path: &Path) -> Result<Connection, String> {
    Connection::open(path).map_err(|e| format!("cannot open {}: {}", path.display(), e))
}

/// Persist an auto-derived IntFile schema into wxbtrv.db.
pub fn upsert_table(conn: &Connection, f: &IntFile) -> Result<(), String> {
    conn.execute(
        "INSERT INTO btr_tables
            (table_name, schema_name, db_name, record_length, page_size, file_flags,
             ignore_null_values, trim_string_fields, translate_oem_to_ansi,
             primary_index, local_cache,
             driver_name, server_name, permanent_int, number_df_fields,
             source_file, source_path, source_dir)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
         ON CONFLICT(table_name, db_name, source_dir) DO UPDATE SET
            schema_name           = excluded.schema_name,
            record_length         = excluded.record_length,
            page_size             = excluded.page_size,
            file_flags            = excluded.file_flags,
            ignore_null_values    = excluded.ignore_null_values,
            trim_string_fields    = excluded.trim_string_fields,
            translate_oem_to_ansi = excluded.translate_oem_to_ansi,
            primary_index         = excluded.primary_index,
            local_cache           = excluded.local_cache,
            driver_name           = excluded.driver_name,
            server_name           = excluded.server_name,
            permanent_int         = excluded.permanent_int,
            number_df_fields      = excluded.number_df_fields,
            source_file           = excluded.source_file,
            source_path           = excluded.source_path",
        params![
            f.table_name,
            f.schema_name,
            f.db_name,
            f.record_length,
            f.page_size,
            f.file_flags,
            f.ignore_null_values as i32,
            f.trim_string_fields as i32,
            f.translate_oem_to_ansi as i32,
            f.primary_index,
            f.local_cache as i32,
            f.driver_name,
            f.server_name,
            f.permanent_int as i32,
            f.number_df_fields,
            f.source_file,
            f.source_path,
            f.source_dir
        ],
    )
    .map_err(|e| e.to_string())?;

    let table_id: i64 = conn
        .query_row(
            "SELECT id FROM btr_tables WHERE table_name=?1 AND db_name=?2 AND source_dir=?3",
            params![f.table_name, f.db_name, f.source_dir],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM btr_fields  WHERE table_id=?1",
        params![table_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM btr_indexes WHERE table_id=?1",
        params![table_id],
    )
    .map_err(|e| e.to_string())?;

    for field in &f.fields {
        conn.execute(
            "INSERT INTO btr_fields
                (table_id, field_number, field_name, native_type, native_length, native_offset,
                 field_index, default_value)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                table_id,
                field.num,
                field.name,
                field.native_type,
                field.length,
                field.offset,
                field.field_index,
                field.default_value
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for idx in &f.indexes {
        conn.execute(
            "INSERT INTO btr_indexes (table_id, index_number, num_segments) VALUES (?1,?2,?3)",
            params![table_id, idx.num, idx.num_segments],
        )
        .map_err(|e| e.to_string())?;
        let idx_id = conn.last_insert_rowid();
        for (pos, seg) in idx.segments.iter().enumerate() {
            conn.execute(
                "INSERT INTO btr_index_segs (index_id, position, field_number, attrs, descending, null_value)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![idx_id, pos as i64, seg.field_num, seg.attrs,
                        seg.descending as i32, seg.null_value],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Read a single config value from the config table.
pub fn get_config(conn: &Connection, section: &str, key: &str) -> String {
    conn.query_row(
        "SELECT value FROM config WHERE section=?1 AND key=?2",
        params![section, key],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_default()
}

/// Read SQL Server connection details from the config table.
pub fn load_config(conn: &Connection) -> Result<SqlConfig, String> {
    let get = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM config WHERE section='MDS' AND key=?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    };
    Ok(SqlConfig {
        server: get("SERVER"),
        database: get("DATABASE"),
        schema: get("SCHEMA"),
        user: get("USER"),
        password: get("PASSWORD"),
    })
}

/// Load the schema for a single table by name (case-insensitive).
/// Applies the connection schema as fallback if the table has no schema_name.
pub fn load_table(conn: &Connection, table_name: &str) -> Result<IntFile, String> {
    let cfg = load_config(conn).unwrap_or_else(|_| SqlConfig {
        server: String::new(),
        database: String::new(),
        schema: String::new(),
        user: String::new(),
        password: String::new(),
    });

    let upper = table_name.to_ascii_uppercase();
    let row = conn
        .query_row(
            "SELECT id, table_name, schema_name, db_name,
                record_length, page_size, file_flags,
                ignore_null_values, trim_string_fields, translate_oem_to_ansi,
                primary_index, local_cache,
                COALESCE(driver_name,''), COALESCE(server_name,''),
                COALESCE(permanent_int,0), COALESCE(number_df_fields,0),
                COALESCE(source_file,''), COALESCE(source_path,''), COALESCE(source_dir,'')
         FROM btr_tables WHERE upper(table_name)=?1",
            params![upper],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    IntFile {
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
                        driver_name: row.get(12)?,
                        server_name: row.get(13)?,
                        permanent_int: row.get::<_, i32>(14)? != 0,
                        number_df_fields: row.get::<_, u32>(15)?,
                        source_file: row.get(16)?,
                        source_path: row.get(17)?,
                        source_dir: row.get(18)?,
                        fields: Vec::new(),
                        indexes: Vec::new(),
                    },
                ))
            },
        )
        .map_err(|_| format!("table '{}' not found in wxbtrv.db", table_name))?;

    let (id, mut file) = row;
    // Apply connection schema as fallback
    if file.schema_name.is_empty() && !cfg.schema.is_empty() {
        file.schema_name = cfg.schema.clone();
    }
    file.fields = load_fields(conn, id)?;
    file.indexes = load_indexes(conn, id)?;
    Ok(file)
}

fn load_fields(conn: &Connection, table_id: i64) -> Result<Vec<IntField>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT field_number, field_name, native_type, native_length, native_offset,
                field_index, default_value
         FROM btr_fields WHERE table_id=?1 ORDER BY field_number",
        )
        .map_err(|e| e.to_string())?;

    let result = stmt
        .query_map(params![table_id], |row| {
            Ok(IntField {
                num: row.get::<_, u32>(0)?,
                name: row.get(1)?,
                native_type: row.get(2)?,
                length: row.get::<_, u32>(3)?,
                offset: row.get::<_, u32>(4)?,
                field_index: row.get(5)?,
                default_value: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string());
    result
}

fn load_indexes(conn: &Connection, table_id: i64) -> Result<Vec<IntIndex>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, index_number, COALESCE(num_segments,0)
         FROM btr_indexes WHERE table_id=?1 ORDER BY index_number",
        )
        .map_err(|e| e.to_string())?;

    let index_rows: Vec<(i64, u32, u32)> = stmt
        .query_map(params![table_id], |row| {
            Ok((
                row.get(0)?,
                row.get::<_, i64>(1)? as u32,
                row.get::<_, i64>(2)? as u32,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;

    let mut indexes = Vec::new();
    for (idx_id, num, num_segments) in index_rows {
        let mut seg_stmt = conn
            .prepare(
                "SELECT field_number, attrs, descending, COALESCE(null_value,0)
             FROM btr_index_segs WHERE index_id=?1 ORDER BY position",
            )
            .map_err(|e| e.to_string())?;

        let segments: Vec<IndexSegment> = seg_stmt
            .query_map(params![idx_id], |row| {
                Ok(IndexSegment {
                    field_num: row.get::<_, i64>(0)? as u32,
                    attrs: row.get::<_, i64>(1)? as u16,
                    descending: row.get::<_, i32>(2)? != 0,
                    null_value: row.get::<_, i64>(3)? as u8,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;

        indexes.push(IntIndex {
            num,
            num_segments,
            segments,
        });
    }
    Ok(indexes)
}

/// Derive a table name from a .B file path: "G:\data\BKGLTRAN.B" → "BKGLTRAN"
pub fn table_name_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_ascii_uppercase())
        .unwrap_or_default()
}
