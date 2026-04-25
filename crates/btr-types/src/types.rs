/// Parsed representation of a Btrieve INT file (table schema definition).
#[derive(Debug, Clone)]
pub struct IntFile {
    pub table_name: String,
    pub schema_name: String, // SCHEMA_NAME — empty means "use connection default"
    pub db_name: String,     // DATABASE_SPACE_NAME
    pub record_length: u32,  // LOGICAL_RECORD_LENGTH
    pub page_size: u16,      // PAGE_SIZE
    pub file_flags: u16,     // FILE_FLAGS
    pub ignore_null_values: bool, // IGNORE_NULL_VALUES
    pub trim_string_fields: bool, // TRIM_STRING_FIELDS
    pub translate_oem_to_ansi: bool, // TRANSLATE_OEM_TO_ANSI
    pub primary_index: Option<u32>,
    pub local_cache: bool, // LOCAL_CACHE YES/NO

    // ── Additional INT file metadata ─────────────────────────────────────────
    pub driver_name: String,   // DRIVER_NAME (e.g. "SQL_BTR")
    pub server_name: String,   // SERVER_NAME — per-table connection override
    pub permanent_int: bool,   // PERMANENT_INT YES/NO
    pub number_df_fields: u32, // NUMBER_DF_FIELDS declared field count

    // ── Source tracking ──────────────────────────────────────────────────────
    pub source_file: String,
    pub source_path: String,
    pub source_dir: String,

    pub fields: Vec<IntField>,
    pub indexes: Vec<IntIndex>,
}

#[derive(Debug, Clone)]
pub struct IntField {
    pub num: u32,
    pub name: String,
    pub native_type: i32,
    pub length: u32,
    pub offset: u32,
    pub field_index: Option<u32>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IntIndex {
    pub num: u32,
    pub num_segments: u32, // INDEX_NUMBER_SEGMENTS (declared segment count)
    pub segments: Vec<IndexSegment>,
}

impl IntIndex {
    /// Total key length in bytes (sum of participating field lengths).
    pub fn key_len(&self, fields: &[IntField]) -> u32 {
        self.segments
            .iter()
            .filter_map(|s| fields.iter().find(|f| f.num == s.field_num))
            .map(|f| f.length)
            .sum()
    }
}

#[derive(Debug, Clone)]
pub struct IndexSegment {
    pub field_num: u32,
    pub attrs: u16,
    pub descending: bool,
    pub null_value: u8, // INDEX_SEGMENT_NULL_VALUE — byte value meaning "null key" (usually 0)
}
