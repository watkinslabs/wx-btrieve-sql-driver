//! Table-listing endpoints. Reads from wxbtrv.db's btr_tables /
//! btr_fields / btr_indexes / btr_index_segs schema directly so we
//! don't lock the DB through the runtime's connection cache.

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::state::{ApiError, AppState};

#[derive(Serialize)]
pub struct TableSummary {
    pub table_name: String,
    pub schema_name: String,
    pub db_name: String,
    pub source_dir: String,
    pub record_length: u32,
    pub field_count: u32,
    pub index_count: u32,
}

pub async fn list_tables(State(state): State<AppState>) -> Result<Json<Vec<TableSummary>>, ApiError> {
    let conn = state.open()?;
    let mut stmt = conn.prepare(
        "SELECT t.table_name, t.schema_name, t.db_name, t.source_dir, t.record_length,
                (SELECT COUNT(*) FROM btr_fields  WHERE table_id = t.id) AS fc,
                (SELECT COUNT(*) FROM btr_indexes WHERE table_id = t.id) AS ic
         FROM btr_tables t ORDER BY t.table_name, t.source_dir",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TableSummary {
                table_name: row.get(0)?,
                schema_name: row.get(1)?,
                db_name: row.get(2)?,
                source_dir: row.get(3)?,
                record_length: row.get::<_, i64>(4)? as u32,
                field_count: row.get::<_, i64>(5)? as u32,
                index_count: row.get::<_, i64>(6)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(rows))
}

#[derive(Serialize)]
pub struct FieldRow {
    pub num: u32,
    pub name: String,
    pub native_type: i32,
    pub length: u32,
    pub offset: u32,
    pub field_index: Option<u32>,
    pub default_value: Option<String>,
}

#[derive(Serialize)]
pub struct IndexRow {
    pub num: u32,
    pub segments: Vec<IndexSegmentRow>,
}

#[derive(Serialize)]
pub struct IndexSegmentRow {
    pub field_num: u32,
    pub attrs: u32,
    pub descending: bool,
}

#[derive(Serialize)]
pub struct TableDetail {
    pub summary: TableSummary,
    pub primary_index: Option<u32>,
    pub ignore_null_values: bool,
    pub trim_string_fields: bool,
    pub translate_oem_to_ansi: bool,
    pub local_cache: bool,
    pub fields: Vec<FieldRow>,
    pub indexes: Vec<IndexRow>,
}

pub async fn show_table(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<TableDetail>, ApiError> {
    let conn = state.open()?;

    let (id, summary, primary_index, ignore_null, trim, oem, local_cache): (
        i64,
        TableSummary,
        Option<u32>,
        bool,
        bool,
        bool,
        bool,
    ) = conn
        .query_row(
            "SELECT t.id, t.table_name, t.schema_name, t.db_name, t.source_dir,
                    t.record_length,
                    (SELECT COUNT(*) FROM btr_fields  WHERE table_id = t.id),
                    (SELECT COUNT(*) FROM btr_indexes WHERE table_id = t.id),
                    t.primary_index, t.ignore_null_values, t.trim_string_fields,
                    t.translate_oem_to_ansi, t.local_cache
             FROM btr_tables t WHERE UPPER(t.table_name) = UPPER(?1) LIMIT 1",
            rusqlite::params![name],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    TableSummary {
                        table_name: r.get(1)?,
                        schema_name: r.get(2)?,
                        db_name: r.get(3)?,
                        source_dir: r.get(4)?,
                        record_length: r.get::<_, i64>(5)? as u32,
                        field_count: r.get::<_, i64>(6)? as u32,
                        index_count: r.get::<_, i64>(7)? as u32,
                    },
                    r.get::<_, Option<i64>>(8)?.map(|n| n as u32),
                    r.get::<_, i64>(9)? != 0,
                    r.get::<_, i64>(10)? != 0,
                    r.get::<_, i64>(11)? != 0,
                    r.get::<_, i64>(12)? != 0,
                ))
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                ApiError::not_found(format!("table '{}' not found", name))
            }
            other => other.into(),
        })?;

    let mut stmt = conn.prepare(
        "SELECT field_number, field_name, native_type, native_length, native_offset,
                field_index, default_value
         FROM btr_fields WHERE table_id = ?1 ORDER BY field_number",
    )?;
    let fields = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok(FieldRow {
                num: r.get::<_, i64>(0)? as u32,
                name: r.get(1)?,
                native_type: r.get::<_, i64>(2)? as i32,
                length: r.get::<_, i64>(3)? as u32,
                offset: r.get::<_, i64>(4)? as u32,
                field_index: r.get::<_, Option<i64>>(5)?.map(|n| n as u32),
                default_value: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut idx_stmt = conn.prepare(
        "SELECT id, index_number FROM btr_indexes WHERE table_id = ?1 ORDER BY index_number",
    )?;
    let mut indexes = Vec::new();
    let raw: Vec<(i64, u32)> = idx_stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u32))
        })?
        .collect::<Result<_, _>>()?;
    for (idx_id, num) in raw {
        let mut seg_stmt = conn.prepare(
            "SELECT field_number, attrs, descending FROM btr_index_segs
             WHERE index_id = ?1 ORDER BY position",
        )?;
        let segments = seg_stmt
            .query_map(rusqlite::params![idx_id], |r| {
                Ok(IndexSegmentRow {
                    field_num: r.get::<_, i64>(0)? as u32,
                    attrs: r.get::<_, i64>(1)? as u32,
                    descending: r.get::<_, i64>(2)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        indexes.push(IndexRow { num, segments });
    }

    Ok(Json(TableDetail {
        summary,
        primary_index,
        ignore_null_values: ignore_null,
        trim_string_fields: trim,
        translate_oem_to_ansi: oem,
        local_cache,
        fields,
        indexes,
    }))
}
