use rusqlite::{params, Connection, OptionalExtension, Result};

pub fn upsert_config(conn: &Connection, section: &str, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO config (section, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(section, key) DO UPDATE SET value = excluded.value",
        params![section, key, value],
    )?;
    Ok(())
}

pub fn get_config(conn: &Connection, section: &str, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM config WHERE section = ?1 AND key = ?2",
        params![section, key],
        |r| r.get(0),
    )
    .optional()
}

pub fn get_all_config(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare("SELECT section, key, value FROM config ORDER BY section, key")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}
