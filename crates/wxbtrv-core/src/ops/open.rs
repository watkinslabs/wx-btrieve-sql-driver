//! Op 0 — Open.

use super::helpers::{alloc_handle, keybuf_cstr, strace};
use super::HANDLE_CTR;
use crate::constants::*;
use crate::table_lookup::table_name_from_path;
use core::ffi::c_void;

/// **Op 0 — Open** (`B_OPEN`)
///
/// Makes a file available for access. Initializes the 128-byte position block
/// that identifies the file in subsequent calls. Must precede any other
/// file-scoped op (except Create, Stat, Set/Get Dir, Version, Reset, Stop,
/// Begin/End/Abort Txn).
///
/// **Parameters**:
/// - posblk: 128-byte buffer; we write a unique handle id at offset 0.
/// - data buffer: optional owner name (null-terminated); ignored here.
/// - key buffer: null/blank-terminated pathname (≤80 bytes).
/// - key number: open mode (0 Normal, -1 Accelerated, -2 RO, -3 Verify,
///   -4 Exclusive) optionally +SEFS/MEFS bias; we ignore mode.
///
/// **Prerequisites**: file must exist; position block zero-initialized.
///
/// **Returns**: 0 on success. Key statuses:
/// - 12: file not found (BTR_FILE_NOT_FOUND) — no INT metadata and SQL
///   discovery failed.
/// - 11/46/88: invalid name / access denied / incompatible mode (spec).
///
/// **Notes**: Open does not establish currency; a subsequent Step Next
/// returns the first physical record. Table metadata is loaded from
/// `wxbtrv.db` or auto-discovered from SQL Server on first open.
pub(super) fn op_open(posblk: *mut c_void, key_buf: *mut c_void) -> i32 {
    let path = keybuf_cstr(key_buf as *const c_void);
    let table_name = table_name_from_path(&path);
    strace!("op_open path={} table={}", path, table_name);

    // Look up table metadata: first from wxbtrv.db, then auto-discover from SQL Server.
    let meta = if let Some(m) = crate::table_lookup::get_table_meta_for_path(&table_name, &path) {
        m
    } else {
        strace!(
            "op_open: no INT metadata for '{}', discovering from SQL Server...",
            table_name
        );
        match crate::sql::discover_table_meta(&table_name, &path) {
            Ok(m) => {
                if let Ok(mut st) = crate::state::state().lock() {
                    let key = table_name.to_ascii_uppercase();
                    st.tables.insert(key, m.clone());
                }
                m
            }
            Err(_) => {
                strace!(
                    "op_open FAILED: no metadata for '{}' (INT or SQL Server)",
                    table_name
                );
                return BTR_FILE_NOT_FOUND;
            }
        }
    };

    strace!(
        "op_open recnum_col={} ignore_null={} trim={} oem_to_ansi={}",
        meta.recnum_col,
        meta.ignore_null_values,
        meta.trim_string_fields,
        meta.translate_oem_to_ansi
    );

    // Assign a unique handle ID and write it to posblk[0..4].
    let handle_id = HANDLE_CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if !posblk.is_null() {
        unsafe {
            core::ptr::write_unaligned(posblk as *mut u32, handle_id);
        }
    }

    let reclen = meta.record_length;
    alloc_handle(handle_id, meta, path.clone());
    strace!("op_open handle_id={} reclen={}", handle_id, reclen);
    BTR_SUCCESS
}
