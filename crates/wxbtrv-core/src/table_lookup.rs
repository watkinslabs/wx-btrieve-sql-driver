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
