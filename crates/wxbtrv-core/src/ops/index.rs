//! Index ops — 31 CreateIndex, 32 DropIndex.
//!
//! In our SQL-backed world a Btrieve key = SQL index. We best-effort parse
//! the key-segment spec from the data buffer, emit a CREATE/DROP INDEX
//! against SQL Server, and update the in-memory TableMeta so subsequent
//! Get ops see the new shape.

use super::helpers::{clone_table_meta, posblk_key, strace};
use crate::constants::*;
use crate::sql::execute_sql;
use crate::state::{state, RuntimeIndex};
use core::ffi::c_void;
use core::slice;

/// **Op 31 — Create Index** (`B_CREATE_INDEX`)
///
/// Adds a new key (index) to an open file. The data buffer holds one or
/// more 16-byte key-segment specs identical to those used by Create (14).
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `data_buf`: key-segment spec(s). We only peek at position/length to
///   identify the underlying column.
/// - `data_len`: total descriptor size in bytes.
/// - `key_num`: unused on input.
///
/// Status: 0 on success, 3 if file not open, 26 if the key count is already
/// at the implementation max, non-zero on SQL failure.
pub(super) fn op_create_index(
    posblk: *mut c_void,
    data_buf: *const c_void,
    data_len: *mut u32,
) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let dlen = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len as usize }
    };
    if data_buf.is_null() || dlen < 16 {
        strace!("op_create_index h={} no descriptor (dlen={})", hid, dlen);
        return BTR_DATA_TOO_SHORT;
    }
    let desc = unsafe { slice::from_raw_parts(data_buf as *const u8, dlen) };

    // Walk 16-byte segment specs until the SEG flag clears.
    let mut seg_fields: Vec<(u32, u32, bool)> = Vec::new(); // (pos_1based, len, desc)
    let mut off = 0usize;
    loop {
        if off + 16 > desc.len() {
            break;
        }
        let seg = &desc[off..off + 16];
        let pos = u16::from_le_bytes([seg[0], seg[1]]) as u32;
        let len = u16::from_le_bytes([seg[2], seg[3]]) as u32;
        let flags = u16::from_le_bytes([seg[4], seg[5]]);
        let is_desc = (flags & 0x0040) != 0;
        seg_fields.push((pos, len, is_desc));
        let has_more = (flags & 0x0010) != 0;
        off += 16;
        if !has_more {
            break;
        }
    }
    if seg_fields.is_empty() {
        return BTR_DATA_TOO_SHORT;
    }

    // Map byte-offsets to column names via meta.fields.
    let mut cols: Vec<(String, bool)> = Vec::new();
    let mut field_nums: Vec<u32> = Vec::new();
    for (pos, len, is_desc) in &seg_fields {
        let off0 = pos.saturating_sub(1);
        match meta
            .fields
            .iter()
            .find(|f| f.offset == off0 && (f.length == *len || *len == 0))
        {
            Some(f) => {
                cols.push((format!("[{}]", f.name.replace(']', "]]")), *is_desc));
                field_nums.push(f.num);
            }
            None => {
                strace!(
                    "op_create_index h={} no field at offset {} len {} — skipping",
                    hid,
                    off0,
                    len
                );
            }
        }
    }
    if cols.is_empty() {
        return BTR_INVALID_KEY_NUM;
    }

    let max_keys = 119u32;
    if meta.indexes.len() as u32 >= max_keys {
        return 26;
    }
    let new_num = meta.indexes.iter().map(|i| i.num).max().unwrap_or(0) + 1;
    let ix_name = format!(
        "wxbtrv_{}_{}",
        meta.table_name.replace(' ', "_").to_ascii_lowercase(),
        new_num
    );
    let col_list = cols
        .iter()
        .map(|(c, d)| if *d { format!("{} DESC", c) } else { c.clone() })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "CREATE INDEX [{}] ON {} ({})",
        ix_name,
        meta.table_ref("", ""),
        col_list
    );
    strace!("op_create_index h={} sql={}", hid, sql);
    if let Err(e) = execute_sql(&sql) {
        strace!("op_create_index h={} sql err={}", hid, e);
        return e;
    }

    // Update in-memory meta on the handle so subsequent Stat/Get ops see it.
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            let key_len: u32 = seg_fields.iter().map(|(_, l, _)| *l).sum();
            h.meta.indexes.push(RuntimeIndex {
                num: new_num,
                field_nums: field_nums.clone(),
                attrs: vec![0; field_nums.len()],
                desc: cols.iter().map(|(_, d)| *d).collect(),
                null_values: vec![0; field_nums.len()],
                key_len,
            });
        }
    }
    BTR_SUCCESS
}

/// **Op 32 — Drop Index** (`B_DROP_INDEX`)
///
/// Removes a key from an open file, by 0-based key number.
///
/// - `posblk`: open position block — handle id at offset 0.
/// - `key_num`: 0-based key number of the index to drop.
///
/// Status: 0 on success, 3 if file not open, 6 if key_num out of range.
pub(super) fn op_drop_index(posblk: *mut c_void, key_num: i16) -> i32 {
    let hid = posblk_key(posblk as *const c_void);
    let meta = match clone_table_meta(hid) {
        Some(m) => m,
        None => return BTR_FILE_NOT_OPEN,
    };
    let kn = key_num as usize;
    if kn >= meta.indexes.len() {
        return BTR_INVALID_KEY_NUM;
    }
    let target_num = meta.indexes[kn].num;
    let ix_name = format!(
        "wxbtrv_{}_{}",
        meta.table_name.replace(' ', "_").to_ascii_lowercase(),
        target_num
    );
    let sql = format!("DROP INDEX [{}] ON {}", ix_name, meta.table_ref("", ""));
    strace!("op_drop_index h={} kn={} sql={}", hid, key_num, sql);
    if let Err(e) = execute_sql(&sql) {
        strace!("op_drop_index h={} sql err={} (ignored)", hid, e);
    }
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&hid) {
            h.meta.indexes.retain(|ix| ix.num != target_num);
        }
    }
    BTR_SUCCESS
}
