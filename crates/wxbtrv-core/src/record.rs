/// record.rs — pack SQL column values into a fixed-width Btrieve binary record,
/// and unpack key bytes for use in SQL WHERE clauses.
use crate::state::{IntField, RuntimeIndex as IntIndex};
use crate::trace::trace;

// ── Safe slice helpers ────────────────────────────────────────────────────────
//
// The decode path consumes byte slices ultimately sourced from a DOS V86
// caller. A panic here would trap the entire NTVDM process, so every
// slice-to-array conversion goes through one of these helpers, and every
// failure is reported through `trace::trace`.

/// Compute `[offset .. offset + length]` against `record`, clamping safely on
/// any out-of-range or overflowing input. Returns an empty slice (and traces)
/// if the requested window is unsatisfiable.
fn field_slice<'a>(record: &'a [u8], offset: u32, length: u32, field_name: &str) -> &'a [u8] {
    let start = offset as usize;
    let len = length as usize;
    let Some(end) = start.checked_add(len) else {
        trace(&format!(
            "record::field_slice: offset+length overflow for field '{}' (offset={} length={} record_len={})",
            field_name, offset, length, record.len()
        ));
        return &[];
    };
    if end > record.len() {
        trace(&format!(
            "record::field_slice: short record for field '{}' (need offset={}..{} but record_len={})",
            field_name, start, end, record.len()
        ));
        return &[];
    }
    &record[start..end]
}

/// Try to read a fixed-size little-endian array from the head of `slice`.
/// On short input, traces and returns `None` — callers fall back to a default.
fn try_le_array<const N: usize>(slice: &[u8], context: &str) -> Option<[u8; N]> {
    if slice.len() < N {
        trace(&format!(
            "record::try_le_array: short slice for {} (need {} bytes, have {})",
            context,
            N,
            slice.len()
        ));
        return None;
    }
    let mut buf = [0u8; N];
    buf.copy_from_slice(&slice[..N]);
    Some(buf)
}

// ── Native type constants (Btrieve) ───────────────────────────────────────────
const TYPE_STRING: i32 = 0;
const TYPE_INT: i32 = 1;
const TYPE_FLOAT: i32 = 2;
const TYPE_DATE: i32 = 3; // 4 bytes: [month, day, year_lo, year_hi]
const TYPE_TIME: i32 = 4; // 4 bytes: HHMMSSCC as LE u32
const TYPE_DECIMAL: i32 = 5; // BCD packed
const TYPE_MONEY: i32 = 6; // BCD, 4 implied decimal places
const TYPE_LOGICAL: i32 = 7; // 1 byte: 0=false, non-zero=true
const TYPE_LSTRING: i32 = 10; // length-prefixed string (1-byte len + data)
const TYPE_ZSTRING: i32 = 11; // null-terminated string (Btrieve type 11)
const TYPE_ZSTRING_12: i32 = 12; // alternate null-terminated encoding
const TYPE_AUTOINC: i32 = 14; // unsigned LE integer / autoincrement
const TYPE_AUTOINCREMENT: i32 = 15; // AUTOINCREMENT (auto-assign on insert)

/// Pack a row of SQL column values (in field-number order) into a binary record
/// of `record_length` bytes.  `columns[i]` is the string value for `fields[i]`.
pub fn pack_row(fields: &[IntField], columns: &[String], record_length: u32) -> Vec<u8> {
    let mut buf = vec![0u8; record_length as usize];

    for (f, val) in fields.iter().zip(columns.iter()) {
        let start = f.offset as usize;
        let len = f.length as usize;
        let Some(end) = start.checked_add(len) else {
            trace(&format!(
                "record::pack_row: offset+length overflow for field '{}' (offset={} length={} buf_len={})",
                f.name, f.offset, f.length, buf.len()
            ));
            continue;
        };
        if end > buf.len() {
            trace(&format!(
                "record::pack_row: field '{}' overflows record buffer (need {}..{} buf_len={})",
                f.name,
                start,
                end,
                buf.len()
            ));
            continue;
        }
        if len == 0 {
            continue;
        }
        let slot = &mut buf[start..end];

        match f.native_type {
            TYPE_STRING => {
                // Fixed-length ASCII, right-pad with spaces
                let bytes = val.as_bytes();
                let n = bytes.len().min(slot.len());
                slot[..n].copy_from_slice(&bytes[..n]);
                for b in &mut slot[n..] {
                    *b = b' ';
                }
            }
            TYPE_LSTRING => {
                // 1-byte length prefix, then data, right-pad with spaces
                if slot.len() >= 2 {
                    let bytes = val.as_bytes();
                    let data_cap = slot.len() - 1;
                    let n = bytes.len().min(data_cap);
                    slot[0] = n as u8;
                    slot[1..=n].copy_from_slice(&bytes[..n]);
                    for b in &mut slot[n + 1..] {
                        *b = b' ';
                    }
                }
            }
            TYPE_ZSTRING | TYPE_ZSTRING_12 => {
                // Null-terminated string
                let bytes = val.as_bytes();
                let n = bytes.len().min(slot.len().saturating_sub(1));
                slot[..n].copy_from_slice(&bytes[..n]);
                slot[n] = 0;
                for b in &mut slot[n + 1..] {
                    *b = 0;
                }
            }
            TYPE_INT | TYPE_AUTOINC | TYPE_AUTOINCREMENT => {
                // Little-endian signed integer
                let v: i64 = val.trim().parse().unwrap_or(0);
                let le = v.to_le_bytes();
                let n = slot.len().min(8);
                slot[..n].copy_from_slice(&le[..n]);
            }
            TYPE_FLOAT => {
                let v: f64 = val.trim().parse().unwrap_or(0.0);
                match slot.len() {
                    4 => slot.copy_from_slice(&(v as f32).to_le_bytes()),
                    8 => slot.copy_from_slice(&v.to_le_bytes()),
                    _ => {}
                }
            }
            TYPE_MONEY => {
                // 8-byte signed LE, 4 implied decimal places
                let v: f64 = val.trim().parse().unwrap_or(0.0);
                let units = (v * 10000.0) as i64;
                let le = units.to_le_bytes();
                let n = slot.len().min(8);
                slot[..n].copy_from_slice(&le[..n]);
            }
            TYPE_DECIMAL => {
                // For now treat as integer — full BCD encoding is complex
                let v: i64 = val.trim().parse().unwrap_or(0);
                let le = v.to_le_bytes();
                let n = slot.len().min(8);
                slot[..n].copy_from_slice(&le[..n]);
            }
            TYPE_LOGICAL => {
                let v = val.trim();
                slot[0] = if v == "1"
                    || v.eq_ignore_ascii_case("true")
                    || v.eq_ignore_ascii_case("yes")
                {
                    0xFF
                } else {
                    0
                };
            }
            TYPE_DATE => {
                // Btrieve DATE: [day, month, year_lo, year_hi]  (encoding confirmed from live data)
                // SQL returns YYYY-MM-DD (from date column) or NULL
                if val.trim().eq_ignore_ascii_case("NULL") { /* leave as zeros */
                } else {
                    let digits: String =
                        val.trim().chars().filter(|c| c.is_ascii_digit()).collect();
                    if slot.len() >= 4 && digits.len() >= 8 {
                        let year: u16 = digits[0..4].parse().unwrap_or(0);
                        let month: u8 = digits[4..6].parse().unwrap_or(0);
                        let day: u8 = digits[6..8].parse().unwrap_or(0);
                        slot[0] = day;
                        slot[1] = month;
                        slot[2] = year as u8;
                        slot[3] = (year >> 8) as u8;
                    }
                }
            }
            TYPE_TIME => {
                // Btrieve TIME: packed HHMMSSCC as LE u32
                // SQL returns HH:MM:SS or HH:MM:SS.fff
                if val.trim().eq_ignore_ascii_case("NULL") { /* leave as zeros */
                } else {
                    let digits: String =
                        val.trim().chars().filter(|c| c.is_ascii_digit()).collect();
                    if slot.len() >= 4 && digits.len() >= 6 {
                        let hh: u32 = digits[0..2].parse().unwrap_or(0);
                        let mm: u32 = digits[2..4].parse().unwrap_or(0);
                        let ss: u32 = digits[4..6].parse().unwrap_or(0);
                        let packed = hh * 1_000_000 + mm * 10_000 + ss * 100;
                        slot[..4].copy_from_slice(&packed.to_le_bytes());
                    }
                }
            }
            _ => {
                // Unknown: treat as string, right-pad with spaces
                let bytes = val.as_bytes();
                let n = bytes.len().min(slot.len());
                slot[..n].copy_from_slice(&bytes[..n]);
                for b in &mut slot[n..] {
                    *b = b' ';
                }
            }
        }
    }

    buf
}

/// Unpack a binary Btrieve record into SQL column value strings (one per field, in field order).
/// Returns a vec of (field_name, sql_literal) pairs suitable for INSERT or UPDATE.
pub fn unpack_row(fields: &[IntField], record: &[u8]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|f| {
            let slice = field_slice(record, f.offset, f.length, &f.name);

            let val = match f.native_type {
                TYPE_STRING => {
                    let s: String = slice
                        .iter()
                        .map(|&b| b as char)
                        .collect::<String>()
                        .trim_end()
                        .to_string();
                    format!("'{}'", s.replace('\'', "''"))
                }
                TYPE_ZSTRING => {
                    let end_z = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                    let s = String::from_utf8_lossy(&slice[..end_z])
                        .trim_end()
                        .to_string();
                    format!("'{}'", s.replace('\'', "''"))
                }
                TYPE_INT | TYPE_AUTOINC => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    let v = match n {
                        1 => le[0] as i64,
                        2 => i16::from_le_bytes([le[0], le[1]]) as i64,
                        4 => i32::from_le_bytes([le[0], le[1], le[2], le[3]]) as i64,
                        8 => i64::from_le_bytes(le),
                        _ => 0,
                    };
                    v.to_string()
                }
                TYPE_FLOAT => {
                    let default = f.default_value.as_deref().unwrap_or("0");
                    if slice.is_empty() || slice.iter().all(|&b| b == 0) {
                        default.to_string()
                    } else {
                        match slice.len() {
                            4 => match try_le_array::<4>(slice, "TYPE_FLOAT f32") {
                                Some(buf) => {
                                    let v = f32::from_le_bytes(buf);
                                    if v.is_finite() && !v.is_subnormal() {
                                        format!("{:.6}", v)
                                    } else {
                                        default.to_string()
                                    }
                                }
                                None => default.to_string(),
                            },
                            8 => match try_le_array::<8>(slice, "TYPE_FLOAT f64") {
                                Some(buf) => {
                                    let v = f64::from_le_bytes(buf);
                                    if v.is_finite() && !v.is_subnormal() {
                                        format!("{:.10}", v)
                                    } else {
                                        default.to_string()
                                    }
                                }
                                None => default.to_string(),
                            },
                            _ => {
                                trace(&format!(
                                    "record::unpack_row: TYPE_FLOAT for '{}' has unsupported length {} (expected 4 or 8)",
                                    f.name,
                                    slice.len()
                                ));
                                default.to_string()
                            }
                        }
                    }
                }
                TYPE_MONEY => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    let units = i64::from_le_bytes(le);
                    format!("{:.4}", units as f64 / 10000.0)
                }
                TYPE_LOGICAL => {
                    if slice.first().copied().unwrap_or(0) != 0 {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                TYPE_DATE => {
                    // Btrieve DATE: [day, month, year_lo, year_hi]  (encoding confirmed from live data)
                    // Zero value → use FIELD_DEFAULT_VALUE if present, else NULL.
                    if slice.len() >= 4 && slice.iter().any(|&b| b != 0) {
                        let day = slice[0] as u16;
                        let month = slice[1] as u16;
                        let year = slice[2] as u16 | ((slice[3] as u16) << 8);
                        if year > 0 && (1..=12).contains(&month) && (1..=31).contains(&day) {
                            format!("'{:04}-{:02}-{:02}'", year, month, day)
                        } else {
                            f.default_value
                                .as_deref()
                                .map(|d| format!("'{}'", d))
                                .unwrap_or_else(|| "NULL".to_string())
                        }
                    } else {
                        f.default_value
                            .as_deref()
                            .map(|d| format!("'{}'", d))
                            .unwrap_or_else(|| "NULL".to_string())
                    }
                }
                TYPE_TIME => {
                    // Btrieve TIME: 4-byte LE u32, packed HHMMSSCC
                    // Zero = midnight (00:00:00), which is valid — never emit NULL for time.
                    match try_le_array::<4>(slice, "TYPE_TIME u32") {
                        Some(buf) => {
                            let t = u32::from_le_bytes(buf);
                            let hh = (t / 1_000_000) % 100;
                            let mm = (t / 10_000) % 100;
                            let ss = (t / 100) % 100;
                            format!("'{:02}:{:02}:{:02}'", hh, mm, ss)
                        }
                        None => "'00:00:00'".to_string(),
                    }
                }
                TYPE_DECIMAL => {
                    // Treated as a little-endian signed integer (full BCD decode TBD).
                    match try_le_array::<8>(slice, "TYPE_DECIMAL i64") {
                        Some(buf) => i64::from_le_bytes(buf).to_string(),
                        None => {
                            let mut le = [0u8; 8];
                            let n = slice.len().min(8);
                            le[..n].copy_from_slice(&slice[..n]);
                            i64::from_le_bytes(le).to_string()
                        }
                    }
                }
                _ => {
                    // Unknown/unsupported types: treat as string.
                    // All-zero → use FIELD_DEFAULT_VALUE if present, else NULL.
                    if slice.iter().all(|&b| b == 0) {
                        let null_val = f
                            .default_value
                            .as_deref()
                            .map(|d| format!("'{}'", d))
                            .unwrap_or_else(|| "NULL".to_string());
                        return (f.name.clone(), null_val);
                    }
                    let s: String = slice
                        .iter()
                        .map(|&b| b as char)
                        .collect::<String>()
                        .trim_end()
                        .to_string();
                    format!("'{}'", s.replace('\'', "''"))
                }
            };

            (f.name.clone(), val)
        })
        .collect()
}

/// Typed counterpart to [`unpack_row`]. Returns `(field_name, SqlValue)`
/// pairs ready for binding via the parameterized SQL pipeline.
///
/// Type mapping:
///   STRING / ZSTRING / unknown  → SqlValue::Text (trim trailing spaces)
///   INT / AUTOINC                → SqlValue::I64
///   FLOAT                        → SqlValue::F64 (with default_value fallback
///                                  for all-zero / non-finite values)
///   MONEY                        → SqlValue::F64 (units / 10000)
///   LOGICAL                      → SqlValue::Bool
///   DATE                         → SqlValue::Text "YYYY-MM-DD" or default/NULL
///   TIME                         → SqlValue::Text "HH:MM:SS"
///   DECIMAL                      → SqlValue::I64 (TODO: full BCD decode)
pub fn unpack_row_typed(fields: &[IntField], record: &[u8]) -> Vec<(String, crate::sql_param::SqlValue)> {
    use crate::sql_param::SqlValue;
    fields
        .iter()
        .map(|f| {
            let slice = field_slice(record, f.offset, f.length, &f.name);
            let v: SqlValue = match f.native_type {
                TYPE_STRING => {
                    // Preserve trailing spaces so the stored TEXT in SQLite
                    // matches the padded record bytes exactly. MSSQL CHAR
                    // pads on either side so retaining padding is safe.
                    let s: String = slice.iter().map(|&b| b as char).collect();
                    SqlValue::Text(s)
                }
                TYPE_ZSTRING => {
                    // ZSTRING is null-terminated; everything before the
                    // first 0 byte is the value. No trim — SQLite is exact
                    // and the producer never emits trailing spaces here.
                    let end_z = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                    SqlValue::Text(String::from_utf8_lossy(&slice[..end_z]).into_owned())
                }
                TYPE_INT | TYPE_AUTOINC => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    let v = match n {
                        1 => le[0] as i64,
                        2 => i16::from_le_bytes([le[0], le[1]]) as i64,
                        4 => i32::from_le_bytes([le[0], le[1], le[2], le[3]]) as i64,
                        8 => i64::from_le_bytes(le),
                        _ => 0,
                    };
                    SqlValue::I64(v)
                }
                TYPE_FLOAT => {
                    let default = f
                        .default_value
                        .as_deref()
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .unwrap_or(0.0);
                    if slice.is_empty() || slice.iter().all(|&b| b == 0) {
                        SqlValue::F64(default)
                    } else {
                        let v = match slice.len() {
                            4 => try_le_array::<4>(slice, "TYPE_FLOAT f32")
                                .map(|buf| f32::from_le_bytes(buf) as f64),
                            8 => try_le_array::<8>(slice, "TYPE_FLOAT f64")
                                .map(f64::from_le_bytes),
                            _ => None,
                        };
                        SqlValue::F64(
                            v.filter(|v| v.is_finite() && !v.is_subnormal())
                                .unwrap_or(default),
                        )
                    }
                }
                TYPE_MONEY => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    let units = i64::from_le_bytes(le);
                    SqlValue::F64(units as f64 / 10000.0)
                }
                TYPE_LOGICAL => SqlValue::Bool(slice.first().copied().unwrap_or(0) != 0),
                TYPE_DATE => {
                    if slice.len() >= 4 && slice.iter().any(|&b| b != 0) {
                        let day = slice[0] as u16;
                        let month = slice[1] as u16;
                        let year = slice[2] as u16 | ((slice[3] as u16) << 8);
                        if year > 0 && (1..=12).contains(&month) && (1..=31).contains(&day) {
                            SqlValue::Text(format!("{:04}-{:02}-{:02}", year, month, day))
                        } else {
                            f.default_value
                                .as_deref()
                                .map(|d| SqlValue::Text(d.to_string()))
                                .unwrap_or(SqlValue::Null)
                        }
                    } else {
                        f.default_value
                            .as_deref()
                            .map(|d| SqlValue::Text(d.to_string()))
                            .unwrap_or(SqlValue::Null)
                    }
                }
                TYPE_TIME => {
                    let buf = try_le_array::<4>(slice, "TYPE_TIME u32").unwrap_or([0; 4]);
                    let t = u32::from_le_bytes(buf);
                    let hh = (t / 1_000_000) % 100;
                    let mm = (t / 10_000) % 100;
                    let ss = (t / 100) % 100;
                    SqlValue::Text(format!("{:02}:{:02}:{:02}", hh, mm, ss))
                }
                TYPE_DECIMAL => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    SqlValue::I64(i64::from_le_bytes(le))
                }
                _ => {
                    if slice.iter().all(|&b| b == 0) {
                        f.default_value
                            .as_deref()
                            .map(|d| SqlValue::Text(d.to_string()))
                            .unwrap_or(SqlValue::Null)
                    } else {
                        let s: String = slice
                            .iter()
                            .map(|&b| b as char)
                            .collect::<String>()
                            .trim_end()
                            .to_string();
                        SqlValue::Text(s)
                    }
                }
            };
            (f.name.clone(), v)
        })
        .collect()
}

/// Extract the WHERE-clause key value(s) from a key_buffer, given the relevant index.
/// Returns Vec of (field_name, Option<SqlValue>) pairs ready for binding.
/// Returns `None` for a segment when its raw bytes equal the segment's
/// null_value — IGNORE_NULL_VALUES behavior — so the caller treats it as a
/// wildcard in WHERE clauses.
pub fn unpack_key_fields(
    fields: &[IntField],
    index: &IntIndex,
    key_buffer: &[u8],
    null_wildcard: bool,
) -> Vec<(String, Option<crate::sql_param::SqlValue>)> {
    use crate::sql_param::SqlValue;
    let field_map: std::collections::HashMap<u32, &IntField> =
        fields.iter().map(|f| (f.num, f)).collect();

    let mut result = Vec::new();
    let mut kb_offset = 0usize;

    for (seg_idx, &fnum) in index.field_nums.iter().enumerate() {
        let Some(f) = field_map.get(&fnum) else {
            continue;
        };
        let flen = f.length as usize;
        let end = kb_offset
            .checked_add(flen)
            .unwrap_or(key_buffer.len())
            .min(key_buffer.len());
        let slice = if kb_offset < key_buffer.len() {
            &key_buffer[kb_offset..end]
        } else {
            &[]
        };
        kb_offset = kb_offset.saturating_add(flen);

        // Null check: a segment is null when all its bytes equal the segment's null_value
        // (INDEX_SEGMENT_NULL_VALUE, default 0).  Null segments → wildcard for directional
        // ops (IGNORE_NULL_VALUES).  GetEqual passes null_wildcard=false.
        let null_byte = index.null_values.get(seg_idx).copied().unwrap_or(0);
        if null_wildcard && !slice.is_empty() && slice.iter().all(|&b| b == null_byte) {
            result.push((f.name.clone(), None));
            continue;
        }

        let val: SqlValue = match f.native_type {
            TYPE_STRING | TYPE_ZSTRING | TYPE_ZSTRING_12 | TYPE_LSTRING => {
                // Preserve trailing spaces so the bound parameter matches
                // the padded stored form exactly. SQLite TEXT comparison is
                // exact; MSSQL CHAR comparison pads on either side so this
                // works on both. Stop at the first null byte (Z-string
                // terminator).
                let data = if f.native_type == TYPE_LSTRING && !slice.is_empty() {
                    &slice[1..]
                } else {
                    slice
                };
                let s: String = data
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| b as char)
                    .collect();
                SqlValue::Text(s)
            }
            TYPE_INT | TYPE_AUTOINC | TYPE_AUTOINCREMENT => {
                let mut le = [0u8; 8];
                let n = slice.len().min(8);
                le[..n].copy_from_slice(&slice[..n]);
                let v = match n {
                    1 => le[0] as i64,
                    2 => i16::from_le_bytes([le[0], le[1]]) as i64,
                    4 => i32::from_le_bytes([le[0], le[1], le[2], le[3]]) as i64,
                    8 => i64::from_le_bytes(le),
                    _ => 0,
                };
                SqlValue::I64(v)
            }
            TYPE_DATE => {
                if slice.len() >= 4 && slice.iter().any(|&b| b != 0) {
                    let day = slice[0] as u16;
                    let month = slice[1] as u16;
                    let year = slice[2] as u16 | ((slice[3] as u16) << 8);
                    if year > 0 && (1..=12).contains(&month) && (1..=31).contains(&day) {
                        SqlValue::Text(format!("{:04}-{:02}-{:02}", year, month, day))
                    } else {
                        SqlValue::Null
                    }
                } else {
                    SqlValue::Null
                }
            }
            TYPE_TIME => {
                if slice.iter().any(|&b| b != 0) {
                    try_le_array::<4>(slice, "key TYPE_TIME u32")
                        .map(|buf| {
                            let t = u32::from_le_bytes(buf);
                            let hh = (t / 1_000_000) % 100;
                            let mm = (t / 10_000) % 100;
                            let ss = (t / 100) % 100;
                            SqlValue::Text(format!("{:02}:{:02}:{:02}", hh, mm, ss))
                        })
                        .unwrap_or(SqlValue::Null)
                } else {
                    SqlValue::Null
                }
            }
            TYPE_FLOAT => match slice.len() {
                4 => SqlValue::F64(
                    try_le_array::<4>(slice, "key TYPE_FLOAT f32")
                        .map(|buf| f32::from_le_bytes(buf) as f64)
                        .unwrap_or(0.0),
                ),
                8 => SqlValue::F64(
                    try_le_array::<8>(slice, "key TYPE_FLOAT f64")
                        .map(f64::from_le_bytes)
                        .unwrap_or(0.0),
                ),
                _ => SqlValue::F64(0.0),
            },
            TYPE_MONEY => {
                let mut le = [0u8; 8];
                let n = slice.len().min(8);
                le[..n].copy_from_slice(&slice[..n]);
                SqlValue::F64(i64::from_le_bytes(le) as f64 / 10000.0)
            }
            TYPE_DECIMAL => {
                let mut le = [0u8; 8];
                let n = slice.len().min(8);
                le[..n].copy_from_slice(&slice[..n]);
                SqlValue::I64(i64::from_le_bytes(le))
            }
            TYPE_LOGICAL => SqlValue::Bool(slice.first().copied().unwrap_or(0) != 0),
            _ => {
                let s: String = slice
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| b as char)
                    .collect::<String>()
                    .trim_end()
                    .to_string();
                SqlValue::Text(s)
            }
        };

        result.push((f.name.clone(), Some(val)));
    }

    result
}

/// Build a SQL WHERE clause from key fields.
pub fn build_where_clause(key_fields: &[(String, String)]) -> String {
    if key_fields.is_empty() {
        return "1=1".to_string();
    }
    key_fields
        .iter()
        .map(|(name, val)| format!("[{}] = {}", name.replace(']', "]]"), val))
        .collect::<Vec<_>>()
        .join(" AND ")
}

// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn f(native_type: i32, offset: u32, length: u32) -> IntField {
        IntField {
            num: 1,
            name: "col".into(),
            native_type,
            length,
            offset,
            field_index: None,
            default_value: None,
        }
    }
    fn fd(native_type: i32, offset: u32, length: u32, default: &str) -> IntField {
        IntField {
            num: 1,
            name: "col".into(),
            native_type,
            length,
            offset,
            field_index: None,
            default_value: Some(default.into()),
        }
    }

    // ── DATE ─────────────────────────────────────────────────────────────────

    #[test]
    fn date_pack_dmy_order() {
        // Btrieve DATE: [day, month, year_lo, year_hi]
        // Confirmed from live BKGLTRAN data: 2016-01-02 → 02 01 E0 07
        let packed = pack_row(&[f(TYPE_DATE, 0, 4)], &["2016-01-02".into()], 4);
        assert_eq!(
            packed,
            vec![0x02, 0x01, 0xE0, 0x07],
            "day=2 month=1 year=2016(LE)"
        );
    }

    #[test]
    fn date_unpack_dmy_order() {
        let record = vec![0x02, 0x01, 0xE0, 0x07];
        let rows = unpack_row(&[f(TYPE_DATE, 0, 4)], &record);
        assert_eq!(rows[0].1, "'2016-01-02'");
    }

    #[test]
    fn date_zero_emits_null() {
        let record = vec![0x00, 0x00, 0x00, 0x00];
        let rows = unpack_row(&[f(TYPE_DATE, 0, 4)], &record);
        assert_eq!(rows[0].1, "NULL");
    }

    #[test]
    fn date_zero_with_default() {
        let record = vec![0x00, 0x00, 0x00, 0x00];
        let rows = unpack_row(&[fd(TYPE_DATE, 0, 4, "0001-01-01")], &record);
        assert_eq!(rows[0].1, "'0001-01-01'");
    }

    #[test]
    fn date_pack_null_stays_zero() {
        let packed = pack_row(&[f(TYPE_DATE, 0, 4)], &["NULL".into()], 4);
        assert_eq!(packed, vec![0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn date_round_trip() {
        for date in &["2023-12-31", "1999-06-15", "2000-01-01"] {
            let packed = pack_row(&[f(TYPE_DATE, 0, 4)], &[date.to_string()], 4);
            let rows = unpack_row(&[f(TYPE_DATE, 0, 4)], &packed);
            assert_eq!(rows[0].1, format!("'{}'", date), "round-trip {}", date);
        }
    }

    // ── TIME ─────────────────────────────────────────────────────────────────

    #[test]
    fn time_pack_hhmmsscc() {
        // "13:45:30" → 13*1_000_000 + 45*10_000 + 30*100 = 13_453_000 = 0x00CD_3E08 LE
        let packed = pack_row(&[f(TYPE_TIME, 0, 4)], &["13:45:30".into()], 4);
        let t = u32::from_le_bytes(packed[..4].try_into().unwrap());
        assert_eq!(t, 13_453_000);
    }

    #[test]
    fn time_unpack_hhmmsscc() {
        let t: u32 = 13_453_000;
        let record = t.to_le_bytes().to_vec();
        let rows = unpack_row(&[f(TYPE_TIME, 0, 4)], &record);
        assert_eq!(rows[0].1, "'13:45:30'");
    }

    #[test]
    fn time_zero_emits_midnight() {
        let rows = unpack_row(&[f(TYPE_TIME, 0, 4)], &[0, 0, 0, 0]);
        assert_eq!(rows[0].1, "'00:00:00'");
    }

    // ── INTEGER ──────────────────────────────────────────────────────────────

    #[test]
    fn int_pack_positive() {
        let packed = pack_row(&[f(TYPE_INT, 0, 4)], &["42".into()], 4);
        assert_eq!(packed, vec![42, 0, 0, 0]);
    }

    #[test]
    fn int_pack_negative() {
        let packed = pack_row(&[f(TYPE_INT, 0, 4)], &["-1".into()], 4);
        assert_eq!(packed, vec![0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn int_round_trip_sizes() {
        for (val, size) in &[
            ("127", 1usize),
            ("-32768", 2),
            ("1000000", 4),
            ("-9000000000", 8),
        ] {
            let packed = pack_row(
                &[f(TYPE_INT, 0, *size as u32)],
                &[val.to_string()],
                *size as u32,
            );
            let rows = unpack_row(&[f(TYPE_INT, 0, *size as u32)], &packed);
            assert_eq!(rows[0].1, *val, "size={}", size);
        }
    }

    // ── STRING ───────────────────────────────────────────────────────────────

    #[test]
    fn string_pack_right_pads_spaces() {
        let packed = pack_row(&[f(TYPE_STRING, 0, 8)], &["hi".into()], 8);
        assert_eq!(packed, b"hi      ");
    }

    #[test]
    fn string_unpack_trims_trailing_spaces() {
        let rows = unpack_row(&[f(TYPE_STRING, 0, 8)], b"hi      ");
        assert_eq!(rows[0].1, "'hi'");
    }

    #[test]
    fn string_escapes_single_quotes() {
        let packed = pack_row(&[f(TYPE_STRING, 0, 8)], &["o'brien".into()], 8);
        let rows = unpack_row(&[f(TYPE_STRING, 0, 8)], &packed);
        assert_eq!(rows[0].1, "'o''brien'");
    }

    // ── ZSTRING ──────────────────────────────────────────────────────────────

    #[test]
    fn zstring_pack_null_terminates() {
        let packed = pack_row(&[f(TYPE_ZSTRING, 0, 6)], &["abc".into()], 6);
        assert_eq!(&packed[..4], b"abc\0");
    }

    #[test]
    fn zstring_unpack_stops_at_null() {
        let record = b"abc\0xx".to_vec();
        let rows = unpack_row(&[f(TYPE_ZSTRING, 0, 6)], &record);
        assert_eq!(rows[0].1, "'abc'");
    }

    // ── LSTRING ──────────────────────────────────────────────────────────────

    #[test]
    fn lstring_pack_length_prefix() {
        let packed = pack_row(&[f(TYPE_LSTRING, 0, 6)], &["abc".into()], 6);
        assert_eq!(packed[0], 3); // length byte
        assert_eq!(&packed[1..4], b"abc");
    }

    // ── FLOAT ────────────────────────────────────────────────────────────────

    #[test]
    fn float32_round_trip() {
        let packed = pack_row(&[f(TYPE_FLOAT, 0, 4)], &["3.14".into()], 4);
        let v = f32::from_le_bytes(packed[..4].try_into().unwrap());
        assert!((v - 3.14_f32).abs() < 0.001);
    }

    #[test]
    fn float64_round_trip() {
        let packed = pack_row(&[f(TYPE_FLOAT, 0, 8)], &["2.718281828".into()], 8);
        let v = f64::from_le_bytes(packed[..8].try_into().unwrap());
        assert!((v - 2.718281828_f64).abs() < 1e-9);
    }

    // ── MONEY ────────────────────────────────────────────────────────────────

    #[test]
    fn money_four_decimal_places() {
        let packed = pack_row(&[f(TYPE_MONEY, 0, 8)], &["1.2345".into()], 8);
        let units = i64::from_le_bytes(packed[..8].try_into().unwrap());
        assert_eq!(units, 12345); // 1.2345 * 10000
        let rows = unpack_row(&[f(TYPE_MONEY, 0, 8)], &packed);
        assert_eq!(rows[0].1, "1.2345");
    }

    // ── LOGICAL ──────────────────────────────────────────────────────────────

    #[test]
    fn logical_true_false() {
        for t in &["1", "true", "yes", "TRUE"] {
            let packed = pack_row(&[f(TYPE_LOGICAL, 0, 1)], &[t.to_string()], 1);
            assert_eq!(packed[0], 0xFF, "true input={}", t);
        }
        let packed = pack_row(&[f(TYPE_LOGICAL, 0, 1)], &["0".into()], 1);
        assert_eq!(packed[0], 0x00);
    }

    // ── Multi-field record ────────────────────────────────────────────────────

    #[test]
    fn multi_field_pack_unpack() {
        // BKGLTRAN-style: GLACCT(str,10) + GLDPT(str,4) + DATE(date,4)
        let fields = vec![
            IntField {
                num: 1,
                name: "GLACCT".into(),
                native_type: TYPE_STRING,
                length: 10,
                offset: 0,
                field_index: None,
                default_value: None,
            },
            IntField {
                num: 2,
                name: "GLDPT".into(),
                native_type: TYPE_STRING,
                length: 4,
                offset: 10,
                field_index: None,
                default_value: None,
            },
            IntField {
                num: 3,
                name: "DATE".into(),
                native_type: TYPE_DATE,
                length: 4,
                offset: 14,
                field_index: None,
                default_value: None,
            },
        ];
        let vals = vec![
            "10203     ".to_string(),
            "GRN ".to_string(),
            "2016-01-02".to_string(),
        ];
        let packed = pack_row(&fields, &vals, 18);
        // First 10 bytes: "10203     "
        assert_eq!(&packed[0..10], b"10203     ");
        // Bytes 10-13: "GRN "
        assert_eq!(&packed[10..14], b"GRN ");
        // Bytes 14-17: [day=2, month=1, 0xE0, 0x07]
        assert_eq!(&packed[14..18], &[0x02, 0x01, 0xE0, 0x07]);

        let rows = unpack_row(&fields, &packed);
        assert_eq!(rows[0].1, "'10203'");
        assert_eq!(rows[1].1, "'GRN'");
        assert_eq!(rows[2].1, "'2016-01-02'");
    }

    // ── unpack_key_fields with DATE ───────────────────────────────────────────

    #[test]
    fn key_fields_date_decodes() {
        let fields = vec![
            IntField {
                num: 1,
                name: "GLACCT".into(),
                native_type: TYPE_STRING,
                length: 10,
                offset: 0,
                field_index: None,
                default_value: None,
            },
            IntField {
                num: 2,
                name: "GLDPT".into(),
                native_type: TYPE_STRING,
                length: 4,
                offset: 10,
                field_index: None,
                default_value: None,
            },
            IntField {
                num: 3,
                name: "DATE".into(),
                native_type: TYPE_DATE,
                length: 4,
                offset: 14,
                field_index: None,
                default_value: None,
            },
        ];
        let index = IntIndex {
            num: 1,
            field_nums: vec![1, 2, 3],
            attrs: vec![],
            desc: vec![],
            null_values: vec![],
            key_len: 18,
        };
        // Build an 18-byte key: "10203     " + "GRN " + [02,01,E0,07]
        let mut key = b"10203     GRN ".to_vec();
        key.extend_from_slice(&[0x02, 0x01, 0xE0, 0x07]);
        let kf = unpack_key_fields(&fields, &index, &key, true);
        use crate::sql_param::SqlValue;
        // STRING values keep their trailing-space padding so the bound
        // parameter matches the stored CHAR/TEXT exactly.
        assert_eq!(
            kf[0],
            ("GLACCT".into(), Some(SqlValue::Text("10203     ".into())))
        );
        assert_eq!(kf[1], ("GLDPT".into(), Some(SqlValue::Text("GRN ".into()))));
        assert_eq!(
            kf[2],
            ("DATE".into(), Some(SqlValue::Text("2016-01-02".into())))
        );
    }

    // ── No-panic guarantees on malformed input ────────────────────────────────
    //
    // The decode path is fed by a DOS V86 caller via raw pointers; a panic
    // would trap NTVDM. Every TYPE_* branch must tolerate a record that is
    // shorter than the field schema claims, completely empty, or has
    // pathologically large offsets.

    fn fields_one(t: i32, len: u32) -> Vec<IntField> {
        vec![f(t, 0, len)]
    }

    #[test]
    fn unpack_truncated_float32_no_panic() {
        let rows = unpack_row(&fields_one(TYPE_FLOAT, 4), &[0x01, 0x02]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "0");
    }

    #[test]
    fn unpack_truncated_float64_no_panic() {
        let rows = unpack_row(&fields_one(TYPE_FLOAT, 8), &[0x01, 0x02, 0x03]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "0");
    }

    #[test]
    fn unpack_truncated_time_no_panic() {
        let rows = unpack_row(&fields_one(TYPE_TIME, 4), &[0x01]);
        assert_eq!(rows[0].1, "'00:00:00'");
    }

    #[test]
    fn unpack_truncated_decimal_no_panic() {
        let rows = unpack_row(&fields_one(TYPE_DECIMAL, 8), &[0x01, 0x02, 0x03]);
        // Falls back to a permissive partial decode rather than panicking.
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn unpack_truncated_date_no_panic() {
        let rows = unpack_row(&fields_one(TYPE_DATE, 4), &[0x01, 0x02]);
        assert_eq!(rows[0].1, "NULL");
    }

    #[test]
    fn unpack_empty_record_all_types_no_panic() {
        for t in &[
            TYPE_STRING,
            TYPE_INT,
            TYPE_FLOAT,
            TYPE_DATE,
            TYPE_TIME,
            TYPE_DECIMAL,
            TYPE_MONEY,
            TYPE_LOGICAL,
            TYPE_LSTRING,
            TYPE_ZSTRING,
            TYPE_ZSTRING_12,
            TYPE_AUTOINC,
            TYPE_AUTOINCREMENT,
            999, // unknown type
        ] {
            let _ = unpack_row(&fields_one(*t, 8), &[]);
            let _ = unpack_row(&fields_one(*t, 8), &[0u8; 4]);
        }
    }

    #[test]
    fn unpack_offset_beyond_record_no_panic() {
        let field = f(TYPE_INT, 100, 4);
        let rows = unpack_row(&[field], &[0x01, 0x02]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "0");
    }

    #[test]
    fn unpack_offset_length_overflow_no_panic() {
        let field = f(TYPE_INT, u32::MAX - 1, 100);
        let rows = unpack_row(&[field], &[0u8; 16]);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn pack_offset_length_overflow_no_panic() {
        let field = f(TYPE_INT, u32::MAX - 1, 100);
        let _ = pack_row(&[field], &["42".into()], 16);
    }

    #[test]
    fn pack_oversized_field_skipped_no_panic() {
        let field = f(TYPE_STRING, 0, 1000);
        let buf = pack_row(&[field], &["x".into()], 4);
        assert_eq!(buf.len(), 4);
    }

    #[test]
    fn unpack_key_fields_truncated_no_panic() {
        let fields = vec![
            f(TYPE_INT, 0, 4),
            f(TYPE_DATE, 0, 4),
            f(TYPE_TIME, 0, 4),
            f(TYPE_FLOAT, 0, 8),
        ];
        let index = IntIndex {
            num: 1,
            field_nums: vec![1, 1, 1, 1],
            attrs: vec![],
            desc: vec![],
            null_values: vec![],
            key_len: 20,
        };
        // Empty key
        let _ = unpack_key_fields(&fields, &index, &[], true);
        // Single byte
        let _ = unpack_key_fields(&fields, &index, &[0xAB], true);
        // Truncated mid-field
        let _ = unpack_key_fields(&fields, &index, &[1, 2, 3, 4, 5], true);
    }

    #[test]
    fn unpack_key_fields_offset_overflow_no_panic() {
        // A field declaring a length of u32::MAX should not blow up the
        // key offset accumulator.
        let fields = vec![f(TYPE_STRING, 0, u32::MAX)];
        let index = IntIndex {
            num: 1,
            field_nums: vec![1, 1],
            attrs: vec![],
            desc: vec![],
            null_values: vec![],
            key_len: 8,
        };
        let _ = unpack_key_fields(&fields, &index, &[1, 2, 3, 4], true);
    }
}
