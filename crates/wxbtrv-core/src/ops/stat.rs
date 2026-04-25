//! Op 15 — Stat. Op 65 — Stat Extended.

use super::helpers::{posblk_key, strace};
use crate::constants::*;
use crate::sql::fetch_with;
use crate::state::state;
use core::ffi::c_void;
use core::ptr;
use core::slice;

/// **Op 15 — Stat** (`B_STAT`)
///
/// Returns the file specification, per-key segment specs, and record
/// count for an open file. We synthesize the layout from INT metadata +
/// a live `COUNT(*)` against SQL Server.
///
/// **Parameters**:
/// - posblk: open file handle.
/// - data buffer: receives 16-byte file header + 16-byte segment specs
///   (one per key segment). Layout matches a freshly-created file.
/// - data len: capacity in / bytes written out.
/// - key number: 0 = file spec + key specs; -1 = also include version.
///
/// **Prerequisites**: file must be open.
///
/// **Returns**: 0 on success. Key statuses:
/// - 3: file not open.
/// - 22: data buffer too short; length set to required size.
///
/// **Notes**: does not affect currency. `page_size` and `file_flags` come
/// entirely from the INT file; record count uses `COUNT(*)` (sys.partitions
/// would be stale after deletes).
pub(super) fn op_stat(posblk: *mut c_void, data_buf: *mut c_void, data_len: *mut u32) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = {
        let Ok(st) = state().lock() else { return 0 };
        let Some(h) = st.handles.get(&hid) else {
            return 0;
        };
        h.meta.clone()
    };
    if data_buf.is_null() || data_len.is_null() {
        return 0;
    }
    let cap = unsafe { *data_len } as usize;
    if cap == 0 {
        return 0;
    }

    // page_size and file_flags come entirely from the INT file.
    let page_size: u16 = meta.page_size;
    let file_flags: u16 = meta.file_flags;

    // Record count: COUNT(*) for exact live row count.
    let rec_count: u32 = {
        let tref = meta.table_ref("", "");
        let sql = format!("SELECT COUNT(*) FROM {}", tref);
        fetch_with(&sql, &[], 1, 1)
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| row.into_iter().next())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|n| n as u32)
            .unwrap_or(0)
    };

    let reclen = meta.record_length as u16;
    let num_keys = meta.indexes.len() as u16;
    // Total key segments across all indexes
    let total_segs: usize = meta.indexes.iter().map(|ix| ix.field_nums.len()).sum();
    let full_size = 16 + 16 * total_segs;
    let out_size = full_size.min(cap);

    let mut buf = vec![0u8; out_size];

    // ── File spec header (16 bytes) ──────────────────────────────────────────
    buf[0..2].copy_from_slice(&reclen.to_le_bytes());
    buf[2..4].copy_from_slice(&page_size.to_le_bytes());
    buf[4..6].copy_from_slice(&num_keys.to_le_bytes());
    // bytes[6-9]: 32-bit record count (LE)
    buf[6..10].copy_from_slice(&rec_count.to_le_bytes());
    // bytes[10-11]: FILE_FLAGS as LE u16 (0x0200 for v7 FC, 0x1200 for older format)
    buf[10..12].copy_from_slice(&file_flags.to_le_bytes());
    // bytes[12-15]: reserved = 0

    // ── Key specifications (16 bytes each) ───────────────────────────────────
    let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
        meta.fields.iter().map(|f| (f.num, f)).collect();
    let mut off = 16usize;

    for (ki, ix) in meta.indexes.iter().enumerate() {
        for (si, &fnum) in ix.field_nums.iter().enumerate() {
            if off + 16 > out_size {
                break;
            }
            let Some(field) = field_map.get(&fnum) else {
                off += 16;
                continue;
            };
            let pos = (field.offset + 1) as u16; // 1-based
            let len = field.length as u16;
            // Use pre-computed attr from INDEX_SEGMENT_FLAG if available, else compute.
            let attr: u16 = if si < ix.attrs.len() {
                ix.attrs[si]
            } else {
                let is_last = si == ix.field_nums.len() - 1;
                let mut a: u16 = 0x0103;
                if field.native_type != 0 {
                    a |= 0x0004;
                }
                if !is_last {
                    a |= 0x0010;
                }
                a
            };

            buf[off..off + 2].copy_from_slice(&pos.to_le_bytes());
            buf[off + 2..off + 4].copy_from_slice(&len.to_le_bytes());
            buf[off + 4..off + 6].copy_from_slice(&attr.to_le_bytes());
            // bytes[6-9] of key spec: same 32-bit record count as header
            buf[off + 6..off + 10].copy_from_slice(&rec_count.to_le_bytes());
            buf[off + 10] = field.native_type as u8; // Btrieve data type
                                                     // bytes[11-13]: reserved = 0
            buf[off + 14] = ki as u8; // key number (0-based ordinal)
                                      // byte[15]: reserved = 0

            off += 16;
        }
    }

    unsafe {
        ptr::copy_nonoverlapping(buf.as_ptr(), data_buf as *mut u8, out_size);
        *data_len = out_size as u32;
    }
    strace!(
        "op_stat handle={} reclen={} nkeys={} page_size={} rec_count={}",
        hid,
        reclen,
        num_keys,
        page_size,
        rec_count
    );
    0
}

/// **Op 65 — Stat Extended** (`B_STAT_EXTENDED`)
///
/// Returns extended file statistics. The data buffer input is a small
/// subfunction descriptor: byte 0 selects subfunction 0 (File Statistics)
/// or 1 (System Data Status).
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: subfunction byte on input, stats blob on output.
/// - `data_len`: capacity on input, bytes written on output.
///
/// Subfunction 0: same layout as Stat (15), with a note in the trace that
/// per-key unique counts are zeroed (not computed) in this implementation.
/// Subfunction 1: tiny blob indicating "system data not enabled".
///
/// Status: 0 success, 3 file not open, 22 buffer too short, 62 unknown
/// subfunction.
pub(super) fn op_stat_extended(
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    {
        let Ok(st) = state().lock() else {
            return BTR_FILE_NOT_OPEN;
        };
        if !st.handles.contains_key(&hid) {
            return BTR_FILE_NOT_OPEN;
        }
    }
    if data_buf.is_null() || data_len.is_null() {
        return BTR_DATA_TOO_SHORT;
    }
    let cap = unsafe { *data_len } as usize;
    if cap == 0 {
        return BTR_DATA_TOO_SHORT;
    }
    // Read subfunction byte from data buffer.
    let subfn = {
        let bytes = unsafe { slice::from_raw_parts(data_buf as *const u8, cap) };
        bytes[0]
    };
    strace!("op_stat_extended h={} subfn={}", hid, subfn);

    match subfn {
        0 => {
            // Fill the basic Stat layout first, then overwrite each key
            // spec's "unique key values" field (offset +0x06) with the
            // per-index COUNT(DISTINCT ...) value.
            let rc = op_stat(posblk, data_buf, data_len);
            if rc != BTR_SUCCESS {
                return rc;
            }
            let meta = {
                let Ok(st) = state().lock() else {
                    return BTR_FILE_NOT_OPEN;
                };
                let Some(h) = st.handles.get(&hid) else {
                    return BTR_FILE_NOT_OPEN;
                };
                h.meta.clone()
            };
            let written = unsafe { *data_len } as usize;
            let tref = meta.table_ref("", "");
            let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
                meta.fields.iter().map(|f| (f.num, f)).collect();
            let mut seg_off = 16usize;
            for ix in meta.indexes.iter() {
                // Compute unique key-value count for this index.
                let distinct_cols: Vec<String> = ix
                    .field_nums
                    .iter()
                    .filter_map(|fnum| {
                        field_map
                            .get(fnum)
                            .map(|f| crate::dialect::active().quote_ident(&f.name))
                    })
                    .collect();
                let unique_count: u32 = if distinct_cols.is_empty() {
                    0
                } else {
                    let sql = if distinct_cols.len() == 1 {
                        format!("SELECT COUNT(DISTINCT {}) FROM {}", distinct_cols[0], tref)
                    } else {
                        format!(
                            "SELECT COUNT(*) FROM (SELECT DISTINCT {} FROM {}) x",
                            distinct_cols.join(", "),
                            tref
                        )
                    };
                    fetch_with(&sql, &[], 1, 1)
                        .ok()
                        .and_then(|rows| rows.into_iter().next())
                        .and_then(|row| row.into_iter().next())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|n| n as u32)
                        .unwrap_or(0)
                };
                strace!(
                    "op_stat_extended idx={} segs={} unique={}",
                    ix.num,
                    ix.field_nums.len(),
                    unique_count
                );
                // Patch offset +0x06 of each segment spec that belongs to
                // this index.
                for _seg in &ix.field_nums {
                    if seg_off + 10 <= written {
                        let buf =
                            unsafe { slice::from_raw_parts_mut(data_buf as *mut u8, written) };
                        buf[seg_off + 6..seg_off + 10].copy_from_slice(&unique_count.to_le_bytes());
                    }
                    seg_off += 16;
                }
            }
            BTR_SUCCESS
        }
        1 => {
            // System data status — a small fixed blob:
            // byte0 = enabled(0), byte1 = log key(0xFF), bytes 2..9 = zero system data.
            let mut blob = [0u8; 10];
            blob[1] = 0xFF;
            let n = blob.len().min(cap);
            unsafe {
                ptr::copy_nonoverlapping(blob.as_ptr(), data_buf as *mut u8, n);
                *data_len = n as u32;
            }
            BTR_SUCCESS
        }
        _ => {
            strace!("op_stat_extended h={} unknown subfn={}", hid, subfn);
            62
        }
    }
}
