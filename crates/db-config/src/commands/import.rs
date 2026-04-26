use crate::btrieve;
use crate::db;
use btr_types::parser::parse_int;
use btr_types::parser::parse_mds;
use std::path::{Path, PathBuf};

/// Collect all .INT file paths under `dir`, recursively if requested.
fn collect_int_files(dir: &Path, recursive: bool) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && recursive {
                stack.push(path);
            } else if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("int") {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

pub fn do_import_int(
    db_path: &Path,
    dirs: &[PathBuf],
    recursive: bool,
    db_name_override: Option<&str>,
    schema_override: Option<&str>,
) -> Result<(), String> {
    do_import_int_with_logger(
        db_path,
        dirs,
        recursive,
        db_name_override,
        schema_override,
        |line| println!("{line}"),
    )
    .map(|_| ())
}

/// Variant that drives every progress line through `log` instead of
/// stdout. Used by wxbtrv-web's SSE endpoint to stream import progress
/// to the UI.
pub fn do_import_int_with_logger<F: FnMut(&str)>(
    db_path: &Path,
    dirs: &[PathBuf],
    recursive: bool,
    db_name_override: Option<&str>,
    schema_override: Option<&str>,
    mut log: F,
) -> Result<ImportIntStats, String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let mut total_count = 0usize;
    let mut total_skipped = 0usize;

    for dir in dirs {
        let canonical_dir = std::fs::canonicalize(dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| dir.to_string_lossy().into_owned());

        let files = collect_int_files(dir, recursive);
        log(&format!(
            "importing from: {canonical_dir}  ({} .INT files)",
            files.len()
        ));

        let mut count = 0;
        let mut skipped = 0;
        for path in files {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let source_path = std::fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned());
            let file_dir = path
                .parent()
                .and_then(|p| std::fs::canonicalize(p).ok())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| canonical_dir.clone());
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    log(&format!("  skip {file_name}: {e}"));
                    skipped += 1;
                    continue;
                }
            };
            match parse_int(&text, &file_name) {
                Ok(mut parsed) => {
                    parsed.source_path = source_path;
                    parsed.source_dir = file_dir;
                    if let Some(db) = db_name_override {
                        if parsed.db_name.is_empty() {
                            parsed.db_name = db.to_string();
                        }
                    }
                    if let Some(schema) = schema_override {
                        if parsed.schema_name.is_empty() {
                            parsed.schema_name = schema.to_string();
                        }
                    }
                    db::upsert_table(&conn, &parsed).map_err(|e| e.to_string())?;
                    log(&format!(
                        "  {:30} → {}.{}.{}",
                        file_name, parsed.db_name, parsed.schema_name, parsed.table_name
                    ));
                    count += 1;
                }
                Err(e) => {
                    log(&format!("  skip {file_name}: {e}"));
                    skipped += 1;
                }
            }
        }
        log(&format!("  {count} imported, {skipped} skipped"));
        total_count += count;
        total_skipped += skipped;
    }

    if dirs.len() > 1 {
        log(&format!(
            "total: {total_count} imported, {total_skipped} skipped"
        ));
    }
    Ok(ImportIntStats {
        imported: total_count,
        skipped: total_skipped,
    })
}

#[derive(Default, Debug, Clone)]
pub struct ImportIntStats {
    pub imported: usize,
    pub skipped: usize,
}

pub fn do_import_mds(db_path: &Path, file: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let cfg = parse_mds(&text);

    let set = |section: &str, key: &str, val: &str| -> Result<(), String> {
        db::upsert_config(&conn, section, key, val).map_err(|e| e.to_string())
    };

    set("MDS", "SERVER", &cfg.server)?;
    set("MDS", "DATABASE", &cfg.database)?;
    set("MDS", "SCHEMA", &cfg.schema)?;
    set("MDS", "USER", &cfg.user)?;
    set("MDS", "PASSWORD", &cfg.password)?;
    set("INT", "PATH", &cfg.int_path)?;

    println!("config imported from {}", file.display());
    println!("  SERVER   = {}", cfg.server);
    println!("  DATABASE = {}", cfg.database);
    println!("  USER     = {}", cfg.user);
    println!("  INT PATH = {}", cfg.int_path);
    Ok(())
}

pub fn do_analyze_b(
    db_path: &Path,
    file: &Path,
    table_override: Option<&str>,
) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;

    let schema = btrieve::parse_schema(file).map_err(|e| e.to_string())?;

    let file_stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("UNKNOWN");
    let raw_name = file_stem.trim_end_matches("_B").trim_end_matches("_b");
    let table_name = table_override.unwrap_or(raw_name).to_ascii_uppercase();
    let source_file = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let int_file = btrieve::schema_to_int(&schema, &table_name, &source_file);

    println!("TABLE:     {}", int_file.table_name);
    println!("version:   {}", if schema.is_v6 { "v6" } else { "v5" });
    println!("rec_len:   {}", schema.record_length);
    println!("page_size: {}", schema.page_size);
    println!("pages:     {}", schema.page_count);
    println!("records:   {}", schema.record_count);
    println!("keys:      {}", schema.keys.len());
    println!("kind:      {:?}", schema.record_kind);
    println!("auto-fields: {}", int_file.fields.len());
    println!();
    println!("NOTE: only key-segment fields were auto-generated.");
    println!(
        "      Use add-field to fill in the remaining {} bytes of record space.",
        schema.record_length as usize
            - int_file
                .fields
                .iter()
                .map(|f| f.length as usize)
                .sum::<usize>()
    );

    db::upsert_table(&conn, &int_file).map_err(|e| e.to_string())?;
    println!("\nsaved to DB as '{}'", int_file.table_name);
    Ok(())
}
