use rusqlite::{params, Connection, Result};

pub struct MigrationRow {
    pub table_name: String,
    pub source_dir: String,
    pub field_count: u32,
    pub migrated: bool,
    pub migrated_at: Option<String>,
    pub row_count: Option<i64>,
    pub target_db: String,
}

pub fn migration_status(conn: &Connection) -> Result<Vec<MigrationRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.table_name, t.source_dir,
                (SELECT COUNT(*) FROM btr_fields WHERE table_id = t.id),
                COALESCE(t.migrated, 0),
                t.migrated_at,
                t.row_count,
                COALESCE(t.target_db, '')
         FROM btr_tables t ORDER BY t.table_name, t.source_dir",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MigrationRow {
                table_name: r.get(0)?,
                source_dir: r.get(1)?,
                field_count: r.get::<_, i64>(2)? as u32,
                migrated: r.get::<_, i64>(3)? != 0,
                migrated_at: r.get(4)?,
                row_count: r.get(5)?,
                target_db: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn mark_migrated(
    conn: &Connection,
    table_name: &str,
    row_count: Option<i64>,
    target_server: &str,
    target_db: &str,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let dt = epoch_to_iso(secs);
    let n = conn
        .execute(
            "UPDATE btr_tables SET migrated=1, migrated_at=?1, row_count=?2,
                               target_server=?3, target_db=?4
         WHERE UPPER(table_name) = UPPER(?5)",
            params![dt, row_count, target_server, target_db, table_name],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("table '{table_name}' not found"));
    }
    Ok(())
}

pub fn clear_migrated(conn: &Connection, table_name: &str) -> Result<(), String> {
    let n = conn
        .execute(
            "UPDATE btr_tables SET migrated=0, migrated_at=NULL, row_count=NULL
         WHERE UPPER(table_name) = UPPER(?1)",
            params![table_name],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(format!("table '{table_name}' not found"));
    }
    Ok(())
}

pub fn epoch_to_iso(secs: u64) -> String {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

pub fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31u64,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &months {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}
