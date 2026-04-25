use rusqlite::{Connection, Result};

pub fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS config (
            id      INTEGER PRIMARY KEY,
            section TEXT NOT NULL,
            key     TEXT NOT NULL,
            value   TEXT NOT NULL DEFAULT '',
            UNIQUE(section, key)
        );

        CREATE TABLE IF NOT EXISTS btr_tables (
            id                    INTEGER PRIMARY KEY,
            table_name            TEXT NOT NULL,
            schema_name           TEXT NOT NULL DEFAULT '',
            db_name               TEXT NOT NULL DEFAULT '',
            record_length         INTEGER NOT NULL DEFAULT 0,
            page_size             INTEGER NOT NULL DEFAULT 4096,
            file_flags            INTEGER NOT NULL DEFAULT 0,
            ignore_null_values    INTEGER NOT NULL DEFAULT 0,
            trim_string_fields    INTEGER NOT NULL DEFAULT 0,
            translate_oem_to_ansi INTEGER NOT NULL DEFAULT 0,
            primary_index         INTEGER,
            local_cache           INTEGER NOT NULL DEFAULT 0,
            driver_name           TEXT NOT NULL DEFAULT '',
            server_name           TEXT NOT NULL DEFAULT '',
            permanent_int         INTEGER NOT NULL DEFAULT 0,
            number_df_fields      INTEGER NOT NULL DEFAULT 0,
            source_file           TEXT NOT NULL DEFAULT '',
            source_path           TEXT NOT NULL DEFAULT '',
            source_dir            TEXT NOT NULL DEFAULT '',
            -- migration tracking
            migrated              INTEGER NOT NULL DEFAULT 0,
            migrated_at           TEXT,
            row_count             INTEGER,
            target_db             TEXT NOT NULL DEFAULT '',
            target_server         TEXT NOT NULL DEFAULT '',
            UNIQUE(table_name, db_name, source_dir)
        );

        CREATE TABLE IF NOT EXISTS btr_fields (
            id            INTEGER PRIMARY KEY,
            table_id      INTEGER NOT NULL REFERENCES btr_tables(id) ON DELETE CASCADE,
            field_number  INTEGER NOT NULL,
            field_name    TEXT NOT NULL,
            native_type   INTEGER NOT NULL DEFAULT 0,
            native_length INTEGER NOT NULL DEFAULT 0,
            native_offset INTEGER NOT NULL DEFAULT 0,
            field_index   INTEGER,
            default_value TEXT,
            UNIQUE(table_id, field_number)
        );

        CREATE TABLE IF NOT EXISTS btr_indexes (
            id            INTEGER PRIMARY KEY,
            table_id      INTEGER NOT NULL REFERENCES btr_tables(id) ON DELETE CASCADE,
            index_number  INTEGER NOT NULL,
            num_segments  INTEGER NOT NULL DEFAULT 0,
            UNIQUE(table_id, index_number)
        );

        CREATE TABLE IF NOT EXISTS btr_index_segs (
            id           INTEGER PRIMARY KEY,
            index_id     INTEGER NOT NULL REFERENCES btr_indexes(id) ON DELETE CASCADE,
            position     INTEGER NOT NULL,
            field_number INTEGER NOT NULL,
            attrs        INTEGER NOT NULL DEFAULT 0,
            descending   INTEGER NOT NULL DEFAULT 0,
            null_value   INTEGER NOT NULL DEFAULT 0
        );
    ",
    )?;
    Ok(())
}

/// Migrate an existing database to the current schema (idempotent).
pub fn upgrade_schema(conn: &Connection) -> Result<()> {
    // Add any missing columns via ALTER TABLE (safe to run repeatedly — errors are ignored)
    let table_cols = [
        "ALTER TABLE btr_tables ADD COLUMN source_path           TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN source_dir            TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN migrated              INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE btr_tables ADD COLUMN migrated_at           TEXT",
        "ALTER TABLE btr_tables ADD COLUMN row_count             INTEGER",
        "ALTER TABLE btr_tables ADD COLUMN target_db             TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN target_server         TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN driver_name           TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN server_name           TEXT    NOT NULL DEFAULT ''",
        "ALTER TABLE btr_tables ADD COLUMN permanent_int         INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE btr_tables ADD COLUMN number_df_fields      INTEGER NOT NULL DEFAULT 0",
    ];
    for ddl in &table_cols {
        let _ = conn.execute_batch(ddl);
    }

    let index_cols = ["ALTER TABLE btr_indexes ADD COLUMN num_segments INTEGER NOT NULL DEFAULT 0"];
    for ddl in &index_cols {
        let _ = conn.execute_batch(ddl);
    }

    let seg_cols = ["ALTER TABLE btr_index_segs ADD COLUMN null_value INTEGER NOT NULL DEFAULT 0"];
    for ddl in &seg_cols {
        let _ = conn.execute_batch(ddl);
    }

    // Fix legacy default 'dbo' → '' for schema_name on existing rows that were
    // never explicitly set (they will get the connection schema at runtime).
    // Only update rows where schema_name was set to the old hardcoded default.
    let _ = conn.execute_batch(
        "UPDATE btr_tables SET schema_name = '' WHERE schema_name = 'dbo'
         AND NOT EXISTS (
             SELECT 1 FROM config WHERE section='config' AND key='SCHEMA' AND value='dbo'
         )",
    );

    Ok(())
}
