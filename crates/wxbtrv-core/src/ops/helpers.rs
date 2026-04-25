//! Shared helpers used by op handlers.

use crate::constants::BTR_FILE_NOT_OPEN;
use crate::sql::resolve_lock_prefix;
use crate::state::{state, HandleEntry, TableMeta};
use core::ffi::c_void;
use core::ptr;
use core::slice;

/// `strace!` macro — logs via `crate::trace::{get_seq, trace}`.
/// Defined here, re-exported under `crate::ops` so submodules can `use super::strace;`.
#[macro_export]
macro_rules! __wxbtrv_strace {
    ($($arg:tt)*) => {
        $crate::trace::trace(&format!(
            "#{}   {}",
            $crate::trace::get_seq(),
            format!($($arg)*)
        ))
    };
}
pub(crate) use crate::__wxbtrv_strace as strace;

/// The handle key is the linear address of the DOS position block.
/// The DOS app reinitializes posblk to spaces before every call, so we never
/// write into posblk — we key the handle table by the address itself.
/// Read the handle ID from posblk[0..4]. On Open, we write an incrementing handle ID
/// into the posblk (matching BTRVDD.DLL behavior). On subsequent calls, we read it back
/// to identify which file the operation targets. This allows multiple files open
/// simultaneously with separate posblks.
pub(super) fn posblk_key(posblk: *const c_void) -> u32 {
    if posblk.is_null() {
        return 0;
    }
    unsafe { core::ptr::read_unaligned(posblk as *const u32) }
}

pub(super) fn keybuf_bytes(key_buffer: *const c_void, max: usize) -> Vec<u8> {
    if key_buffer.is_null() || max == 0 {
        return Vec::new();
    }
    unsafe { slice::from_raw_parts(key_buffer as *const u8, max) }.to_vec()
}

pub(super) fn keybuf_cstr(key_buffer: *const c_void) -> String {
    if key_buffer.is_null() {
        return String::new();
    }
    let bytes = unsafe { slice::from_raw_parts(key_buffer as *const u8, 260) };
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Extract key bytes from a packed record for the given index and write them to the key buffer.
/// Standard Btrieve behavior: after a successful Get, the key buffer is updated with the
/// found record's key field values.
pub(super) fn write_key_buf(key_buf: *mut c_void, meta: &TableMeta, idx_num: u32, packed: &[u8]) {
    if key_buf.is_null() {
        return;
    }
    let Some(idx) = meta.indexes.iter().find(|ix| ix.num == idx_num) else {
        return;
    };
    let field_map: std::collections::HashMap<u32, &crate::state::IntField> =
        meta.fields.iter().map(|f| (f.num, f)).collect();
    let mut out = Vec::with_capacity(idx.key_len as usize);
    for &fnum in &idx.field_nums {
        if let Some(fld) = field_map.get(&fnum) {
            let start = fld.offset as usize;
            let end = start + fld.length as usize;
            if end <= packed.len() {
                out.extend_from_slice(&packed[start..end]);
            } else {
                // Pad with zeros if the packed record is shorter
                let avail = packed.len().saturating_sub(start);
                if avail > 0 {
                    out.extend_from_slice(&packed[start..start + avail]);
                }
                out.resize(out.len() + fld.length as usize - avail, 0);
            }
        }
    }
    unsafe {
        ptr::copy_nonoverlapping(out.as_ptr(), key_buf as *mut u8, out.len());
    }
}

pub(super) fn write_data(data_buf: *mut c_void, data_len: *mut u32, data: &[u8]) {
    if data_buf.is_null() || data_len.is_null() {
        return;
    }
    let cap = unsafe { *data_len } as usize;
    let n = data.len().min(cap);
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), data_buf as *mut u8, n);
        *data_len = n as u32;
    }
}

pub(super) fn alloc_handle(key: u32, meta: TableMeta, b_path: String) {
    let Ok(mut st) = state().lock() else { return };
    st.handles.insert(
        key,
        HandleEntry {
            meta,
            b_path,
            step_last_recnum: None,
            step_dir: 1,
            step_cache: std::collections::VecDeque::new(),
            step_cache_dir: 1,
            get_index_num: None,
            get_last_keys: Vec::new(),
            get_last_desc: Vec::new(),
            get_last_recnum: None,
            get_dir: 1,
            last_recnum: None,
            lock_prefix: None,
            owner_name: None,
        },
    );
}

pub(super) fn clone_table_meta(key: u32) -> Option<TableMeta> {
    let Ok(st) = state().lock() else { return None };
    st.handles.get(&key).map(|h| h.meta.clone())
}

pub(super) fn clone_meta_and_idx(key: u32) -> Option<(TableMeta, Option<u32>)> {
    let Ok(st) = state().lock() else { return None };
    st.handles
        .get(&key)
        .map(|h| (h.meta.clone(), h.get_index_num))
}

pub(super) fn get_lock_prefix(key: u32) -> Result<String, i32> {
    if let Ok(st) = state().lock() {
        if let Some(h) = st.handles.get(&key) {
            if let Some(ref p) = h.lock_prefix {
                return Ok(p.clone());
            }
        }
    }
    let (db, schema, table) = {
        let Ok(st) = state().lock() else {
            return Err(BTR_FILE_NOT_OPEN);
        };
        let Some(h) = st.handles.get(&key) else {
            return Err(BTR_FILE_NOT_OPEN);
        };
        (
            h.meta.db_name.clone(),
            h.meta.schema_name.clone(),
            h.meta.table_name.clone(),
        )
    };
    let prefix = resolve_lock_prefix(&db, &schema, &table)?;
    if let Ok(mut st) = state().lock() {
        if let Some(h) = st.handles.get_mut(&key) {
            h.lock_prefix = Some(prefix.clone());
        }
    }
    Ok(prefix)
}
