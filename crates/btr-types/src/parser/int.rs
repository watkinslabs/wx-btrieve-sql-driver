use crate::types::{IndexSegment, IntField, IntFile, IntIndex};
use std::collections::HashMap;

pub fn parse(text: &str, source_file: &str) -> Result<IntFile, String> {
    let kv = parse_kv_lines(text);

    let table_name = kv
        .get("TABLE_NAME")
        .cloned()
        .ok_or_else(|| format!("{source_file}: missing TABLE_NAME"))?;

    // schema_name is NOT defaulted to "dbo" — empty means "use connection config schema"
    let schema_name = kv.get("SCHEMA_NAME").cloned().unwrap_or_default();
    let db_name = kv.get("DATABASE_SPACE_NAME").cloned().unwrap_or_default();
    let record_length: u32 = kv
        .get("LOGICAL_RECORD_LENGTH")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let page_size: u16 = kv
        .get("PAGE_SIZE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let file_flags: u16 = kv
        .get("FILE_FLAGS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let ignore_null_values = kv
        .get("IGNORE_NULL_VALUES")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let trim_string_fields = kv
        .get("TRIM_STRING_FIELDS")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let translate_oem_to_ansi = kv
        .get("TRANSLATE_OEM_TO_ANSI")
        .map(|s| s.trim() != "0")
        .unwrap_or(false);
    let primary_index: Option<u32> = kv.get("PRIMARY_INDEX").and_then(|s| s.trim().parse().ok());
    let local_cache = kv
        .get("LOCAL_CACHE")
        .map(|s| s.trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(false);

    let driver_name = kv.get("DRIVER_NAME").cloned().unwrap_or_default();
    let server_name = kv.get("SERVER_NAME").cloned().unwrap_or_default();
    let permanent_int = kv
        .get("PERMANENT_INT")
        .map(|s| s.trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(false);
    let number_df_fields: u32 = kv
        .get("NUMBER_DF_FIELDS")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    let mut fields: Vec<IntField> = Vec::new();
    let mut cur_num: Option<u32> = None;
    let mut cur_name = String::new();
    let mut cur_type: i32 = 0;
    let mut cur_len: u32 = 0;
    let mut cur_off: u32 = 0;
    let mut cur_fidx: Option<u32> = None;
    let mut cur_default: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let val: String = match line.find(char::is_whitespace) {
            Some(pos) => line[pos..].trim_start().to_string(),
            None => String::new(),
        };
        match key.to_ascii_uppercase().as_str() {
            "FIELD_NUMBER" => {
                flush_field(
                    &mut fields,
                    cur_num,
                    &cur_name,
                    cur_type,
                    cur_len,
                    cur_off,
                    cur_fidx,
                    cur_default.take(),
                );
                cur_num = val.parse().ok();
                cur_name = String::new();
                cur_type = 0;
                cur_len = 0;
                cur_off = 0;
                cur_fidx = None;
                cur_default = None;
            }
            "FIELD_NAME" => cur_name = val,
            "FIELD_NATIVE_TYPE" => cur_type = val.parse().unwrap_or(0),
            "FIELD_NATIVE_LENGTH" => cur_len = val.parse().unwrap_or(0),
            "FIELD_NATIVE_OFFSET" => cur_off = val.parse().unwrap_or(0),
            "FIELD_INDEX" => cur_fidx = val.parse().ok(),
            "FIELD_DEFAULT_VALUE" => cur_default = if val.is_empty() { None } else { Some(val) },
            _ => {}
        }
    }
    flush_field(
        &mut fields,
        cur_num,
        &cur_name,
        cur_type,
        cur_len,
        cur_off,
        cur_fidx,
        cur_default,
    );

    let field_map: HashMap<u32, &IntField> = fields.iter().map(|f| (f.num, f)).collect();
    let mut indexes = parse_indexes(text);
    if indexes.is_empty() {
        indexes = build_indexes_from_field_index(&fields);
    }
    indexes.sort_by_key(|ix| ix.num);
    let _ = field_map;

    Ok(IntFile {
        table_name,
        schema_name,
        db_name,
        record_length,
        page_size,
        file_flags,
        ignore_null_values,
        trim_string_fields,
        translate_oem_to_ansi,
        primary_index,
        local_cache,
        driver_name,
        server_name,
        permanent_int,
        number_df_fields,
        source_file: source_file.to_string(),
        source_path: String::new(),
        source_dir: String::new(),
        fields,
        indexes,
    })
}

#[allow(clippy::too_many_arguments)]
fn flush_field(
    fields: &mut Vec<IntField>,
    num: Option<u32>,
    name: &str,
    t: i32,
    len: u32,
    off: u32,
    fidx: Option<u32>,
    def: Option<String>,
) {
    if let Some(n) = num {
        if !name.is_empty() {
            fields.push(IntField {
                num: n,
                name: name.to_string(),
                native_type: t,
                length: len,
                offset: off,
                field_index: fidx,
                default_value: def,
            });
        }
    }
}

fn parse_indexes(text: &str) -> Vec<IntIndex> {
    let mut indexes: Vec<IntIndex> = Vec::new();
    let mut cur_idx_num: Option<u32> = None;
    let mut cur_num_segs: u32 = 0;
    let mut cur_segs: Vec<IndexSegment> = Vec::new();
    let mut pending_fnum: Option<u32> = None;
    let mut pending_desc = false;
    let mut pending_null_value: u8 = 0;

    for raw in text.lines() {
        let line = raw.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        let val: String = it.collect::<Vec<_>>().join(" ");
        match key.to_ascii_uppercase().as_str() {
            "INDEX_NUMBER" => {
                flush_index(&mut indexes, cur_idx_num, cur_num_segs, &cur_segs);
                cur_idx_num = val.parse().ok();
                cur_num_segs = 0;
                cur_segs = Vec::new();
                pending_fnum = None;
                pending_desc = false;
                pending_null_value = 0;
            }
            "INDEX_NUMBER_SEGMENTS" => {
                cur_num_segs = val.trim().parse().unwrap_or(0);
            }
            "INDEX_SEGMENT_FIELD" => {
                pending_fnum = val.parse().ok();
                pending_desc = false;
                pending_null_value = 0;
            }
            "INDEX_SEGMENT_DIRECTION" => {
                pending_desc = val.trim().eq_ignore_ascii_case("DESCENDING");
            }
            "INDEX_SEGMENT_NULL_VALUE" => {
                pending_null_value = val.trim().parse().unwrap_or(0);
            }
            "INDEX_SEGMENT_FLAG" => {
                let flag: i32 = val.parse().unwrap_or(-1);
                if let Some(fnum) = pending_fnum.take() {
                    if fnum != 0 && flag != -1 {
                        cur_segs.push(IndexSegment {
                            field_num: fnum,
                            attrs: flag as u16,
                            descending: pending_desc,
                            null_value: pending_null_value,
                        });
                    }
                }
                pending_desc = false;
                pending_null_value = 0;
            }
            _ => {}
        }
    }
    flush_index(&mut indexes, cur_idx_num, cur_num_segs, &cur_segs);
    indexes
}

fn flush_index(
    indexes: &mut Vec<IntIndex>,
    num: Option<u32>,
    num_segments: u32,
    segs: &[IndexSegment],
) {
    if let Some(n) = num {
        if !segs.is_empty() {
            indexes.push(IntIndex {
                num: n,
                num_segments,
                segments: segs.to_vec(),
            });
        }
    }
}

fn build_indexes_from_field_index(fields: &[IntField]) -> Vec<IntIndex> {
    let mut map: HashMap<u32, Vec<&IntField>> = HashMap::new();
    for f in fields {
        if let Some(idx_num) = f.field_index {
            map.entry(idx_num).or_default().push(f);
        }
    }
    let mut result: Vec<IntIndex> = map
        .into_iter()
        .map(|(num, flds)| {
            let segments = flds
                .iter()
                .map(|f| IndexSegment {
                    field_num: f.num,
                    attrs: 0,
                    descending: false,
                    null_value: 0,
                })
                .collect::<Vec<_>>();
            let n = segments.len() as u32;
            IntIndex {
                num,
                num_segments: n,
                segments,
            }
        })
        .collect();
    result.sort_by_key(|ix| ix.num);
    result
}

fn parse_kv_lines(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(pos) = line.find(char::is_whitespace) {
            let key = line[..pos].trim().to_ascii_uppercase();
            let val = line[pos..].trim().to_string();
            map.insert(key, val);
        }
    }
    map
}
