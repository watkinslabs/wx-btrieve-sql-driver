use btr_types::{IndexSegment, IntField, IntFile, IntIndex};
use rusqlite::{params, Connection, OptionalExtension, Result};

pub struct TableRow {
    pub id: i64,
    pub table_name: String,
    pub schema_name: String,
    pub db_name: String,
    pub source_dir: String,
    pub source_path: String,
    pub record_length: u32,
    pub field_count: u32,
    pub index_count: u32,
}

pub fn upsert_table(conn: &Connection, f: &IntFile) -> Result<()> {
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
    )?;

    let table_id: i64 = conn.query_row(
        "SELECT id FROM btr_tables WHERE table_name = ?1 AND db_name = ?2 AND source_dir = ?3",
        params![f.table_name, f.db_name, f.source_dir],
        |row| row.get(0),
    )?;

    conn.execute(
        "DELETE FROM btr_fields  WHERE table_id = ?1",
        params![table_id],
    )?;
    conn.execute(
        "DELETE FROM btr_indexes WHERE table_id = ?1",
        params![table_id],
    )?;

    for field in &f.fields {
        insert_field(conn, table_id, field)?;
    }
    for idx in &f.indexes {
        insert_index(conn, table_id, idx)?;
    }
    Ok(())
}

fn insert_field(conn: &Connection, table_id: i64, f: &IntField) -> Result<()> {
    conn.execute(
        "INSERT INTO btr_fields
            (table_id, field_number, field_name, native_type, native_length, native_offset,
             field_index, default_value)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            table_id,
            f.num,
            f.name,
            f.native_type,
            f.length,
            f.offset,
            f.field_index,
            f.default_value
        ],
    )?;
    Ok(())
}

fn insert_index(conn: &Connection, table_id: i64, idx: &IntIndex) -> Result<()> {
    conn.execute(
        "INSERT INTO btr_indexes (table_id, index_number, num_segments) VALUES (?1, ?2, ?3)",
        params![table_id, idx.num, idx.num_segments],
    )?;
    let idx_id = conn.last_insert_rowid();
    for (pos, seg) in idx.segments.iter().enumerate() {
        conn.execute(
            "INSERT INTO btr_index_segs (index_id, position, field_number, attrs, descending, null_value)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![idx_id, pos as i64, seg.field_num, seg.attrs, seg.descending as i32, seg.null_value],
        )?;
    }
    Ok(())
}

pub fn list_tables(conn: &Connection) -> Result<Vec<TableRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.table_name, t.schema_name, t.db_name, t.source_dir, t.source_path,
                t.record_length,
                (SELECT COUNT(*) FROM btr_fields  WHERE table_id = t.id) AS fc,
                (SELECT COUNT(*) FROM btr_indexes WHERE table_id = t.id) AS ic
         FROM btr_tables t ORDER BY t.table_name, t.source_dir",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TableRow {
                id: row.get(0)?,
                table_name: row.get(1)?,
                schema_name: row.get(2)?,
                db_name: row.get(3)?,
                source_dir: row.get(4)?,
                source_path: row.get(5)?,
                record_length: row.get::<_, i64>(6)? as u32,
                field_count: row.get::<_, i64>(7)? as u32,
                index_count: row.get::<_, i64>(8)? as u32,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_table(
    conn: &Connection,
    name: &str,
    source_dir: Option<&str>,
) -> Result<Option<IntFile>> {
    let upper = name.to_ascii_uppercase();

    let rows: Vec<_> = {
        let mut stmt = conn.prepare(
            "SELECT id, table_name, schema_name, db_name, record_length, page_size, file_flags,
                    ignore_null_values, trim_string_fields, translate_oem_to_ansi,
                    primary_index, local_cache,
                    driver_name, server_name, permanent_int, number_df_fields,
                    source_file, source_path, source_dir
             FROM btr_tables WHERE UPPER(table_name) = ?1
             ORDER BY source_dir",
        )?;
        let mapped = stmt.query_map(params![upper], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)? as u32,
                r.get::<_, i64>(5)? as u16,
                r.get::<_, i64>(6)? as u16,
                r.get::<_, bool>(7)?,
                r.get::<_, bool>(8)?,
                r.get::<_, bool>(9)?,
                r.get::<_, Option<i64>>(10)?.map(|v| v as u32),
                r.get::<_, bool>(11)?,
                r.get::<_, String>(12)?,
                r.get::<_, String>(13)?,
                r.get::<_, bool>(14)?,
                r.get::<_, i64>(15)? as u32,
                r.get::<_, String>(16)?,
                r.get::<_, String>(17)?,
                r.get::<_, String>(18)?,
            ))
        })?;
        mapped.collect::<Result<Vec<_>>>()?
    };

    let row = if let Some(dir) = source_dir {
        rows.into_iter().find(|r| r.18 == dir)
    } else {
        if rows.len() > 1 {
            let dirs: Vec<String> = rows.iter().map(|r| r.18.clone()).collect();
            eprintln!(
                "warning: '{}' exists in {} profiles ({}); returning first. \
                       Use --source-dir to disambiguate.",
                name,
                dirs.len(),
                dirs.join(", ")
            );
        }
        rows.into_iter().next()
    };

    let Some((
        table_id,
        table_name,
        schema_name,
        db_name,
        record_length,
        page_size,
        file_flags,
        ignore_null_values,
        trim_string_fields,
        translate_oem_to_ansi,
        primary_index,
        local_cache,
        driver_name,
        server_name,
        permanent_int,
        number_df_fields,
        source_file,
        source_path,
        source_dir_val,
    )) = row
    else {
        return Ok(None);
    };

    let mut fstmt = conn.prepare(
        "SELECT field_number, field_name, native_type, native_length, native_offset,
                field_index, default_value
         FROM btr_fields WHERE table_id = ?1 ORDER BY field_number",
    )?;
    let fields = fstmt
        .query_map(params![table_id], |r| {
            Ok(IntField {
                num: r.get::<_, i64>(0)? as u32,
                name: r.get(1)?,
                native_type: r.get::<_, i64>(2)? as i32,
                length: r.get::<_, i64>(3)? as u32,
                offset: r.get::<_, i64>(4)? as u32,
                field_index: r.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                default_value: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut istmt = conn.prepare(
        "SELECT id, index_number, num_segments FROM btr_indexes WHERE table_id = ?1 ORDER BY index_number"
    )?;
    let idx_rows = istmt
        .query_map(params![table_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)? as u32,
                r.get::<_, i64>(2)? as u32,
            ))
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut indexes: Vec<IntIndex> = Vec::new();
    for (idx_id, idx_num, num_segments) in idx_rows {
        let mut sstmt = conn.prepare(
            "SELECT field_number, attrs, descending, null_value FROM btr_index_segs
             WHERE index_id = ?1 ORDER BY position",
        )?;
        let segments = sstmt
            .query_map(params![idx_id], |r| {
                Ok(IndexSegment {
                    field_num: r.get::<_, i64>(0)? as u32,
                    attrs: r.get::<_, i64>(1)? as u16,
                    descending: r.get::<_, bool>(2)?,
                    null_value: r.get::<_, i64>(3)? as u8,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        indexes.push(IntIndex {
            num: idx_num,
            num_segments,
            segments,
        });
    }

    Ok(Some(IntFile {
        table_name,
        schema_name,
        db_name,
        record_length,
        page_size,
        file_flags,
        ignore_null_values,
        trim_string_fields,
        translate_oem_to_ansi,
        primary_index,
        local_cache,
        driver_name,
        server_name,
        permanent_int,
        number_df_fields,
        source_file,
        source_path,
        source_dir: source_dir_val,
        fields,
        indexes,
    }))
}

pub fn delete_table(conn: &Connection, name: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM btr_tables WHERE UPPER(table_name) = UPPER(?1)",
        params![name],
    )?;
    Ok(n > 0)
}

pub fn update_table_prop(
    conn: &Connection,
    name: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let col = match key.to_ascii_lowercase().as_str() {
        "schema_name" | "schema" => "schema_name",
        "db_name" | "database" => "db_name",
        "record_length" | "reclen" => "record_length",
        "page_size" => "page_size",
        "file_flags" => "file_flags",
        "ignore_null_values" | "ignore_null" => "ignore_null_values",
        "trim_string_fields" | "trim" => "trim_string_fields",
        "translate_oem_to_ansi" | "oem" => "translate_oem_to_ansi",
        "primary_index" => "primary_index",
        "local_cache" => "local_cache",
        "server_name" | "server" => "server_name",
        "driver_name" | "driver" => "driver_name",
        "permanent_int" | "permanent" => "permanent_int",
        other => return Err(format!("unknown table property '{other}'")),
    };
    let sql = format!("UPDATE btr_tables SET {col} = ?1 WHERE UPPER(table_name) = UPPER(?2)");
    let n = conn
        .execute(&sql, params![value, name])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("table '{name}' not found"));
    }
    Ok(())
}

pub fn upsert_field_by_name(
    conn: &Connection,
    table_name: &str,
    f: &IntField,
) -> Result<(), String> {
    let table_id: i64 = conn
        .query_row(
            "SELECT id FROM btr_tables WHERE UPPER(table_name) = UPPER(?1)",
            params![table_name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e: rusqlite::Error| e.to_string())?
        .ok_or_else(|| format!("table '{table_name}' not found"))?;

    conn.execute(
        "INSERT INTO btr_fields
            (table_id, field_number, field_name, native_type, native_length, native_offset,
             field_index, default_value)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(table_id, field_number) DO UPDATE SET
            field_name    = excluded.field_name,
            native_type   = excluded.native_type,
            native_length = excluded.native_length,
            native_offset = excluded.native_offset,
            field_index   = excluded.field_index,
            default_value = excluded.default_value",
        params![
            table_id,
            f.num,
            f.name,
            f.native_type,
            f.length,
            f.offset,
            f.field_index,
            f.default_value
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_field_by_name(
    conn: &Connection,
    table_name: &str,
    field_num: u32,
) -> Result<(), String> {
    let table_id: i64 = conn
        .query_row(
            "SELECT id FROM btr_tables WHERE UPPER(table_name) = UPPER(?1)",
            params![table_name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e: rusqlite::Error| e.to_string())?
        .ok_or_else(|| format!("table '{table_name}' not found"))?;

    let n = conn
        .execute(
            "DELETE FROM btr_fields WHERE table_id = ?1 AND field_number = ?2",
            params![table_id, field_num],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("field #{field_num} not found in '{table_name}'"));
    }
    Ok(())
}

pub fn upsert_index_by_name(
    conn: &Connection,
    table_name: &str,
    idx: &IntIndex,
) -> Result<(), String> {
    let table_id: i64 = conn
        .query_row(
            "SELECT id FROM btr_tables WHERE UPPER(table_name) = UPPER(?1)",
            params![table_name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e: rusqlite::Error| e.to_string())?
        .ok_or_else(|| format!("table '{table_name}' not found"))?;

    conn.execute(
        "DELETE FROM btr_indexes WHERE table_id = ?1 AND index_number = ?2",
        params![table_id, idx.num],
    )
    .map_err(|e| e.to_string())?;

    insert_index(conn, table_id, idx).map_err(|e| e.to_string())
}

pub fn delete_index_by_name(
    conn: &Connection,
    table_name: &str,
    index_num: u32,
) -> Result<(), String> {
    let table_id: i64 = conn
        .query_row(
            "SELECT id FROM btr_tables WHERE UPPER(table_name) = UPPER(?1)",
            params![table_name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e: rusqlite::Error| e.to_string())?
        .ok_or_else(|| format!("table '{table_name}' not found"))?;

    let n = conn
        .execute(
            "DELETE FROM btr_indexes WHERE table_id = ?1 AND index_number = ?2",
            params![table_id, index_num],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("index #{index_num} not found in '{table_name}'"));
    }
    Ok(())
}
