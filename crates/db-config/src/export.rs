use btr_types::{IntField, IntFile, IntIndex};

/// Render an IntFile back to INT file text, compatible with the DLL parser.
/// Uses server/db from config if provided so the file is self-contained.
pub fn render_int(f: &IntFile, server: &str) -> String {
    let mut out = String::new();

    // Header
    if !server.is_empty() {
        out.push_str(&format!("DRIVER_NAME SQL_BTR\n"));
        out.push_str(&format!("SERVER_NAME {server}\n"));
    }
    if !f.db_name.is_empty() {
        out.push_str(&format!("DATABASE_SPACE_NAME {}\n", f.db_name));
    }
    out.push_str(&format!("TABLE_NAME {}\n", f.table_name));
    out.push_str(&format!("SCHEMA_NAME {}\n", f.schema_name));
    out.push_str(&format!("LOGICAL_RECORD_LENGTH {}\n", f.record_length));
    if f.page_size > 0 {
        out.push_str(&format!("PAGE_SIZE {}\n", f.page_size));
    }
    if f.file_flags > 0 {
        out.push_str(&format!("FILE_FLAGS {}\n", f.file_flags));
    }
    if f.ignore_null_values {
        out.push_str("IGNORE_NULL_VALUES 1\n");
    }
    if f.trim_string_fields {
        out.push_str("TRIM_STRING_FIELDS 1\n");
    }
    if f.translate_oem_to_ansi {
        out.push_str("TRANSLATE_OEM_TO_ANSI 1\n");
    }
    if f.local_cache {
        out.push_str("LOCAL_CACHE YES\n");
    }
    if let Some(pi) = f.primary_index {
        out.push_str(&format!("PRIMARY_INDEX {pi}\n"));
    }
    out.push('\n');

    // Fields
    for field in &f.fields {
        render_field(&mut out, field);
    }

    // Indexes
    for idx in &f.indexes {
        render_index(&mut out, idx);
    }

    out
}

fn render_field(out: &mut String, f: &IntField) {
    out.push_str(&format!("FIELD_NUMBER {}\n", f.num));
    out.push_str(&format!("FIELD_NAME {}\n", f.name));
    out.push_str(&format!("FIELD_NATIVE_TYPE {}\n", f.native_type));
    out.push_str(&format!("FIELD_NATIVE_LENGTH {}\n", f.length));
    out.push_str(&format!("FIELD_NATIVE_OFFSET {}\n", f.offset));
    if let Some(fi) = f.field_index {
        out.push_str(&format!("FIELD_INDEX {fi}\n"));
    }
    if let Some(ref d) = f.default_value {
        out.push_str(&format!("FIELD_DEFAULT_VALUE {d}\n"));
    }
    out.push('\n');
}

fn render_index(out: &mut String, idx: &IntIndex) {
    out.push_str(&format!("INDEX_NUMBER {}\n", idx.num));
    out.push_str(&format!("INDEX_NUMBER_SEGMENTS {}\n", idx.segments.len()));
    for seg in &idx.segments {
        out.push_str(&format!("INDEX_SEGMENT_FIELD {}\n", seg.field_num));
        if seg.descending {
            out.push_str("INDEX_SEGMENT_DIRECTION DESCENDING\n");
        }
        out.push_str(&format!("INDEX_SEGMENT_FLAG {}\n", seg.attrs));
    }
    // null terminator
    out.push_str("INDEX_SEGMENT_FIELD 0\n");
    out.push_str("INDEX_SEGMENT_FLAG -1\n");
    out.push('\n');
}

/// Render a mds.ini from the config key-values stored in the DB.
/// `rows` is a list of (section, key, value) triples.
pub fn render_mds(rows: &[(String, String, String)]) -> String {
    let mut out = String::new();
    let mut cur_section = String::new();
    for (section, key, value) in rows {
        if *section != cur_section {
            out.push_str(&format!("[{section}]\n"));
            cur_section = section.clone();
        }
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

/// File name to use when exporting a table's INT file.
/// Format: {db_name}.{schema_name}.{table_name}.INT
/// Falls back gracefully when db or schema are absent.
pub fn int_filename(f: &IntFile) -> String {
    let t = f.table_name.to_ascii_uppercase();
    match (f.db_name.is_empty(), f.schema_name.is_empty()) {
        (false, false) => format!(
            "{}.{}.{}.INT",
            f.db_name.to_ascii_uppercase(),
            f.schema_name.to_ascii_uppercase(),
            t
        ),
        (false, true) => format!("{}.{}.INT", f.db_name.to_ascii_uppercase(), t),
        (true, false) => format!("{}.{}.INT", f.schema_name.to_ascii_uppercase(), t),
        (true, true) => format!("{}.INT", t),
    }
}

// ── SQL Server DDL generation ────────────────────────────────────────────────

/// Generate a SQL Server CREATE TABLE statement from an IntFile.
/// `add_recnum` adds an MDS_RECNUM IDENTITY column (needed if no PRIMARY_INDEX).
pub fn render_ddl(f: &IntFile, add_recnum: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "-- Source: {}  reclen={} page_size={}\n",
        f.source_file, f.record_length, f.page_size
    ));
    let table_ref = if f.schema_name.is_empty() {
        format!("[{}]", f.table_name)
    } else {
        format!("[{}].[{}]", f.schema_name, f.table_name)
    };
    out.push_str(&format!("CREATE TABLE {table_ref} (\n"));

    let mut cols: Vec<String> = Vec::new();

    if add_recnum {
        cols.push("    [btrv_row] BIGINT IDENTITY(1,1) NOT NULL".to_string());
    }

    for field in &f.fields {
        let sql_type = btr_to_sql(field.native_type, field.length);
        let nullable = if field.default_value.is_some() {
            "NULL"
        } else {
            "NULL"
        };
        cols.push(format!("    [{}] {} {}", field.name, sql_type, nullable));
    }

    out.push_str(&cols.join(",\n"));
    out.push_str("\n);\n");

    // Indexes
    for idx in &f.indexes {
        if idx.segments.is_empty() {
            continue;
        }
        let allows_dups = idx.segments.iter().any(|s| s.attrs & 0x0001 != 0);
        let unique = if allows_dups { "" } else { "UNIQUE " };
        let field_names: Vec<String> = idx
            .segments
            .iter()
            .filter_map(|s| f.fields.iter().find(|fld| fld.num == s.field_num))
            .map(|fld| {
                let dir = if idx
                    .segments
                    .iter()
                    .find(|s| s.field_num == fld.num)
                    .map(|s| s.descending)
                    .unwrap_or(false)
                {
                    " DESC"
                } else {
                    ""
                };
                format!("[{}]{}", fld.name, dir)
            })
            .collect();
        if field_names.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "CREATE {unique}INDEX [IX_{}_{}] ON {table_ref} ({});\n",
            f.table_name,
            idx.num,
            field_names.join(", ")
        ));
    }

    out
}

/// Map a Btrieve native_type + field length to a SQL Server column type.
pub fn btr_to_sql(native_type: i32, length: u32) -> String {
    match native_type {
        0 => format!("VARCHAR({length})"), // STRING
        1 => match length {
            // INTEGER (signed LE)
            1 => "TINYINT".into(),
            2 => "SMALLINT".into(),
            4 => "INT".into(),
            8 => "BIGINT".into(),
            _ => format!("VARBINARY({length})"),
        },
        2 => match length {
            // FLOAT (IEEE 754)
            4 => "REAL".into(),
            _ => "FLOAT".into(),
        },
        3 => "DATE".into(),    // DATE
        4 => "TIME(0)".into(), // TIME
        5 => {
            // DECIMAL (BCD packed)
            let digits = (length * 2).saturating_sub(1).max(1);
            format!("DECIMAL({digits}, 0)")
        }
        6 => {
            // MONEY (BCD, 4dp)
            let digits = (length * 2).saturating_sub(1).max(5);
            format!("DECIMAL({digits}, 4)")
        }
        7 => "BIT".into(),                           // LOGICAL
        8 | 17 | 18 => format!("VARCHAR({length})"), // NUMERIC (ASCII digits)
        9 => match length {
            // BFLOAT (MS Binary)
            4 => "REAL".into(),
            _ => "FLOAT".into(),
        },
        10 | 11 => format!("VARCHAR({length})"), // LSTRING / ZSTRING
        14 => match length {
            // UNSIGNED_BINARY
            2 => "INT".into(),    // u16 → INT
            4 => "BIGINT".into(), // u32 → BIGINT
            _ => format!("VARBINARY({length})"),
        },
        15 => "INT".into(),                  // AUTOINCREMENT
        19 => "DECIMAL(19, 4)".into(),       // CURRENCY (8-byte scaled int)
        20 => "DATETIME2(7)".into(),         // TIMESTAMP (septa-seconds)
        _ => format!("VARBINARY({length})"), // unknown → raw bytes
    }
}
