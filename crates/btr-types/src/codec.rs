/// codec.rs — decode a raw Btrieve binary record into SQL-literal strings.
///
/// Used by both wxbtrv.dll (via re-export) and btr-import.
use crate::IntField;

// ── Native type constants ──────────────────────────────────────────────────────
pub const TYPE_STRING: i32 = 0;
pub const TYPE_INT: i32 = 1;
pub const TYPE_FLOAT: i32 = 2;
pub const TYPE_DATE: i32 = 3;
pub const TYPE_TIME: i32 = 4;
pub const TYPE_DECIMAL: i32 = 5;
pub const TYPE_MONEY: i32 = 6;
pub const TYPE_LOGICAL: i32 = 7;
pub const TYPE_LSTRING: i32 = 10;
pub const TYPE_ZSTRING: i32 = 11;
pub const TYPE_ZSTRING_12: i32 = 12;
pub const TYPE_AUTOINC: i32 = 14;
pub const TYPE_AUTOINCREMENT: i32 = 15;

/// Decode a binary Btrieve record into (field_name, sql_literal) pairs.
///
/// `record` is the raw bytes of one logical record (as read from a .B file or
/// passed through the BTRCALL data buffer).
/// Returns one pair per field, in field-definition order.
pub fn unpack_row(fields: &[IntField], record: &[u8]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|f| {
            let start = f.offset as usize;
            let end = (f.offset + f.length) as usize;
            let slice = if end <= record.len() {
                &record[start..end]
            } else {
                &[]
            };

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
                TYPE_LSTRING => {
                    if slice.is_empty() {
                        return (f.name.clone(), "''".to_string());
                    }
                    let len = slice[0] as usize;
                    let data = &slice[1..slice.len().min(1 + len)];
                    let s = String::from_utf8_lossy(data).trim_end().to_string();
                    format!("'{}'", s.replace('\'', "''"))
                }
                TYPE_ZSTRING | TYPE_ZSTRING_12 => {
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
                    let v: i64 = match n {
                        1 => le[0] as i64,
                        2 => i16::from_le_bytes([le[0], le[1]]) as i64,
                        4 => i32::from_le_bytes([le[0], le[1], le[2], le[3]]) as i64,
                        8 => i64::from_le_bytes(le),
                        _ => 0,
                    };
                    v.to_string()
                }
                TYPE_AUTOINCREMENT => {
                    // auto-assign; emit 0 so INSERT uses the file's stored value
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    let v: i64 = match n {
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
                    if slice.iter().all(|&b| b == 0) {
                        return (f.name.clone(), default.to_string());
                    }
                    match slice.len() {
                        4 => {
                            let v = f32::from_le_bytes(slice.try_into().unwrap_or([0; 4]));
                            if v.is_finite() && !v.is_subnormal() {
                                format!("{:.6}", v)
                            } else {
                                default.to_string()
                            }
                        }
                        8 => {
                            let v = f64::from_le_bytes(slice.try_into().unwrap_or([0; 8]));
                            if v.is_finite() && !v.is_subnormal() {
                                format!("{:.10}", v)
                            } else {
                                default.to_string()
                            }
                        }
                        _ => default.to_string(),
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
                    // Btrieve DATE: [day, month, year_lo, year_hi]  (confirmed from live data)
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
                    // Btrieve TIME: 4-byte LE u32, packed as HHMMSSCC
                    if slice.len() >= 4 {
                        let mut le = [0u8; 4];
                        le.copy_from_slice(&slice[..4]);
                        let t = u32::from_le_bytes(le);
                        let hh = (t / 1_000_000) % 100;
                        let mm = (t / 10_000) % 100;
                        let ss = (t / 100) % 100;
                        format!("'{:02}:{:02}:{:02}'", hh, mm, ss)
                    } else {
                        "'00:00:00'".to_string()
                    }
                }
                TYPE_DECIMAL => {
                    let mut le = [0u8; 8];
                    let n = slice.len().min(8);
                    le[..n].copy_from_slice(&slice[..n]);
                    i64::from_le_bytes(le).to_string()
                }
                _ => {
                    // Unknown: treat as string
                    if slice.iter().all(|&b| b == 0) {
                        return (
                            f.name.clone(),
                            f.default_value
                                .as_deref()
                                .map(|d| format!("'{}'", d))
                                .unwrap_or_else(|| "NULL".to_string()),
                        );
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

/// Return a human-readable name for a Btrieve native field type code.
pub fn type_name(native_type: i32) -> &'static str {
    match native_type {
        TYPE_STRING => "STRING",
        TYPE_INT => "INT",
        TYPE_FLOAT => "FLOAT",
        TYPE_DATE => "DATE",
        TYPE_TIME => "TIME",
        TYPE_DECIMAL => "DECIMAL",
        TYPE_MONEY => "MONEY",
        TYPE_LOGICAL => "LOGICAL",
        TYPE_LSTRING => "LSTRING",
        TYPE_ZSTRING => "ZSTRING",
        TYPE_ZSTRING_12 => "ZSTRING12",
        TYPE_AUTOINC => "AUTOINC",
        TYPE_AUTOINCREMENT => "AUTOINCREMENT",
        _ => "UNKNOWN",
    }
}

/// Map a Btrieve field type + length to a SQL Server column type string.
pub fn sql_type(native_type: i32, length: u32) -> &'static str {
    match native_type {
        TYPE_STRING => "VARCHAR", // caller appends (n)
        TYPE_LSTRING | TYPE_ZSTRING | TYPE_ZSTRING_12 => "VARCHAR",
        TYPE_INT => match length {
            1 => "TINYINT",
            2 => "SMALLINT",
            4 => "INT",
            _ => "BIGINT",
        },
        TYPE_AUTOINC | TYPE_AUTOINCREMENT => "INT",
        TYPE_FLOAT => {
            if length <= 4 {
                "REAL"
            } else {
                "FLOAT"
            }
        }
        TYPE_DATE => "DATE",
        TYPE_TIME => "TIME(0)",
        TYPE_DECIMAL => "DECIMAL(18,0)",
        TYPE_MONEY => "DECIMAL(18,4)",
        TYPE_LOGICAL => "TINYINT",
        _ => "VARBINARY",
    }
}
