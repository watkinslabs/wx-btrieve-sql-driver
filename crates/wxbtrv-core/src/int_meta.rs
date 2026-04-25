use crate::state::{state, TableMeta};
use std::path::Path;

fn normalize_key(s: &str) -> String {
    s.trim().to_ascii_uppercase().replace(['\\', '/', '.'], "_")
}

/// Look up a TableMeta by table name, using the open path's directory to pick
/// the correct database variant. E.g. path "G:\PACIFIC\BKHELP.B" → dir "PACIFIC"
/// → tries "GPACIFIC:BKHELP" first, then falls back to plain "BKHELP".
/// Look up table metadata. The path determines which database to use via tiered config:
/// 1. If path given → resolve directory config → get DATABASE → search that db only
/// 2. If no path → use global config DATABASE → search that db only
/// Never cross databases. If not found in the resolved db, return None (caller can discover).
pub fn get_table_meta_for_path(name: &str, path: &str) -> Option<TableMeta> {
    let k = name.trim().to_ascii_uppercase();
    if k.is_empty() {
        return None;
    }

    // Resolve database: directory config first, then global fallback
    let dir = crate::state::dir_from_path(path);
    let db = crate::state::resolve_config(&dir, "DATABASE").to_ascii_uppercase();

    let Ok(st) = state().lock() else { return None };

    if !db.is_empty() {
        // Search ONLY in the resolved database
        let db_key = format!("{}:{}", db, k);
        if let Some(m) = st.tables.get(&db_key) {
            return Some(m.clone());
        }
        // Try normalized variant
        let db_key_n = format!("{}:{}", db, normalize_key(&k));
        if let Some(m) = st.tables.get(&db_key_n) {
            return Some(m.clone());
        }
        // Not found in the resolved database — return None, don't cross databases
        return None;
    }

    // No database resolved at all (empty config) — plain lookup as last resort
    if let Some(m) = st.tables.get(&k) {
        return Some(m.clone());
    }
    None
}

/// Legacy wrapper — plain name lookup using global config database.
pub fn get_table_meta(name: &str) -> Option<TableMeta> {
    get_table_meta_for_path(name, "")
}

/// Extract a table name from a file path like "G:\CANADA\BKSYUSER.B".
pub fn table_name_from_path(path: &str) -> String {
    let p = Path::new(path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(path);
    stem.to_ascii_uppercase()
}

// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    const BKGLTRAN_INT: &str = r#"#DEV TEST FILE
DRIVER_NAME SQL_BTR
SERVER_NAME 10.0.0.231
DATABASE_SPACE_NAME GCanada
TABLE_NAME BKGLTRAN
SCHEMA_NAME dbo
NUMBER_DF_FIELDS 10
PERMANENT_INT NO
LOCAL_CACHE YES
FILE_FLAGS 512
PAGE_SIZE 4096
LOGICAL_RECORD_LENGTH 74
IGNORE_NULL_VALUES 1
TRIM_STRING_FIELDS 1

FIELD_NUMBER 1
FIELD_NAME BKGL_TRN_GLACCT
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 10
FIELD_NATIVE_OFFSET 0
FIELD_INDEX 1

FIELD_NUMBER 2
FIELD_NAME BKGL_TRN_KEY
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 18
FIELD_NATIVE_OFFSET 0

FIELD_NUMBER 3
FIELD_NAME BKGL_TRN_GLDPT
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 4
FIELD_NATIVE_OFFSET 10
FIELD_INDEX 1

FIELD_NUMBER 4
FIELD_NAME BKGL_TRN_DATE
FIELD_NATIVE_TYPE 3
FIELD_NATIVE_LENGTH 4
FIELD_NATIVE_OFFSET 14
FIELD_DEFAULT_VALUE 0001-01-01
FIELD_INDEX 1

FIELD_NUMBER 5
FIELD_NAME BKGL_TRN_CODE
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 10
FIELD_NATIVE_OFFSET 18
FIELD_INDEX 3

FIELD_NUMBER 6
FIELD_NAME BKGL_TRN_INVC
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 10
FIELD_NATIVE_OFFSET 28

FIELD_NUMBER 7
FIELD_NAME BKGL_TRN_DESC
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 25
FIELD_NATIVE_OFFSET 38

FIELD_NUMBER 8
FIELD_NAME BKGL_TRN_DC
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 1
FIELD_NATIVE_OFFSET 63

FIELD_NUMBER 9
FIELD_NAME BKGL_TRN_AMT
FIELD_NATIVE_TYPE 2
FIELD_NATIVE_LENGTH 8
FIELD_NATIVE_OFFSET 64
FIELD_DEFAULT_VALUE 0

FIELD_NUMBER 10
FIELD_NAME BKGL_TRN_TYPE
FIELD_NATIVE_TYPE 0
FIELD_NATIVE_LENGTH 2
FIELD_NATIVE_OFFSET 72

INDEX_NUMBER 1
INDEX_NUMBER_SEGMENTS 4
INDEX_SEGMENT_FIELD 1
INDEX_SEGMENT_FLAG 275
INDEX_SEGMENT_FIELD 3
INDEX_SEGMENT_FLAG 275
INDEX_SEGMENT_FIELD 4
INDEX_SEGMENT_FLAG 263
INDEX_SEGMENT_FIELD 0
INDEX_SEGMENT_FLAG -1
INDEX_SEGMENT_NULL_VALUE 0

INDEX_NUMBER 2
INDEX_NUMBER_SEGMENTS 2
INDEX_SEGMENT_FIELD 1
INDEX_SEGMENT_FLAG 259
INDEX_SEGMENT_FIELD 0
INDEX_SEGMENT_FLAG -1
INDEX_SEGMENT_NULL_VALUE 0

INDEX_NUMBER 3
INDEX_NUMBER_SEGMENTS 3
INDEX_SEGMENT_FIELD 4
INDEX_SEGMENT_FLAG 279
INDEX_SEGMENT_FIELD 5
INDEX_SEGMENT_FLAG 259
INDEX_SEGMENT_FIELD 0
INDEX_SEGMENT_FLAG -1
INDEX_SEGMENT_NULL_VALUE 0
"#;

    fn parse_tm(text: &str) -> Option<TableMeta> {
        btr_types::parser::parse_int(text, "test")
            .ok()
            .map(TableMeta::from)
    }

    #[test]
    fn parse_basic_metadata() {
        let tm = parse_tm(BKGLTRAN_INT).unwrap();
        assert_eq!(tm.table_name, "BKGLTRAN");

        assert_eq!(tm.db_name, "GCanada");
        assert_eq!(tm.record_length, 74);
        assert_eq!(tm.page_size, 4096);
        assert_eq!(tm.file_flags, 512);
    }

    #[test]
    fn parse_field_count_and_names() {
        let tm = parse_tm(BKGLTRAN_INT).unwrap();
        assert_eq!(tm.fields.len(), 10);
        assert_eq!(tm.fields[0].name, "BKGL_TRN_GLACCT");
        assert_eq!(tm.fields[1].name, "BKGL_TRN_KEY");
        assert_eq!(tm.fields[3].name, "BKGL_TRN_DATE");
    }

    #[test]
    fn parse_index_count_and_segments() {
        let tm = parse_tm(BKGLTRAN_INT).unwrap();
        assert_eq!(tm.indexes.len(), 3);
        let ix1 = &tm.indexes[0];
        assert_eq!(ix1.num, 1);
        assert_eq!(ix1.field_nums.len(), 3);
        assert_eq!(ix1.key_len, 18);
        let ix2 = &tm.indexes[1];
        assert_eq!(ix2.num, 2);
        assert_eq!(ix2.key_len, 10);
    }

    #[test]
    fn index_for_key_len_exact() {
        let tm = parse_tm(BKGLTRAN_INT).unwrap();
        let ix = tm.index_for_key_len(18).unwrap();
        assert_eq!(ix.num, 1);
        let ix = tm.index_for_key_len(10).unwrap();
        assert_eq!(ix.num, 2);
    }

    #[test]
    fn table_ref_with_db_and_schema() {
        let tm = parse_tm(BKGLTRAN_INT).unwrap();
        assert_eq!(tm.table_ref("", ""), "[GCanada].[dbo].[BKGLTRAN]");
        assert_eq!(tm.table_ref("OtherDB", ""), "[OtherDB].[dbo].[BKGLTRAN]");
    }
}
