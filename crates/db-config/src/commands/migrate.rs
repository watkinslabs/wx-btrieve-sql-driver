use crate::db;
use std::path::Path;

pub fn do_migration_status(db_path: &Path) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::upgrade_schema(&conn).map_err(|e| e.to_string())?;
    let rows = db::migration_status(&conn).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        println!("(no tables)");
        return Ok(());
    }

    let done = rows.iter().filter(|r| r.migrated).count();
    let total = rows.len();
    println!("MIGRATION STATUS: {done}/{total} migrated\n");
    println!(
        "{:<30} {:<24} {:>7}  {:>6}  {:<16} MIGRATED_AT",
        "TABLE", "SOURCE_DIR", "ROWS", "FIELDS", "TARGET_DB"
    );
    println!("{}", "-".repeat(100));
    for r in &rows {
        let status = if r.migrated {
            r.migrated_at.as_deref().unwrap_or("yes").to_string()
        } else {
            "-".to_string()
        };
        let rows_str = r
            .row_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".to_string());
        let dir_short = std::path::Path::new(&r.source_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&r.source_dir);
        println!(
            "{:<30} {:<24} {:>7}  {:>6}  {:<16} {}",
            r.table_name, dir_short, rows_str, r.field_count, r.target_db, status
        );
    }
    Ok(())
}

pub fn do_mark_migrated(
    db_path: &Path,
    table: &str,
    rows: Option<i64>,
    server: &str,
    target_db: &str,
) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::upgrade_schema(&conn).map_err(|e| e.to_string())?;
    db::mark_migrated(&conn, table, rows, server, target_db)?;
    println!("marked {table} as migrated (rows={rows:?}, server={server}, db={target_db})");
    Ok(())
}

pub fn do_clear_migrated(db_path: &Path, table: &str) -> Result<(), String> {
    let conn = db::open(db_path).map_err(|e| e.to_string())?;
    db::upgrade_schema(&conn).map_err(|e| e.to_string())?;
    db::clear_migrated(&conn, table)?;
    println!("cleared migration flag for {table}");
    Ok(())
}
