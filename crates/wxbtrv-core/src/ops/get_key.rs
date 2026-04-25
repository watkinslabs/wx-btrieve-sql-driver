//! Get Key ops (+50 bias) — 55–63.
//! Detects key presence without returning record data; calls the base Get op
//! then zeroes data_len so the app knows no record data was transferred.

use super::get::{
    op_get_equal, op_get_first, op_get_greater, op_get_greater_or_equal, op_get_last, op_get_less,
    op_get_less_or_equal, op_get_next, op_get_prev,
};
use super::helpers::strace;
use crate::constants::BTR_UNSUPPORTED_OP;
use core::ffi::c_void;

/// **Ops 55–63 — Get Key** (+50 bias; `B_GET_KEY_EQUAL` etc.)
///
/// Adding 50 to any logical-retrieval Get opcode instructs the engine
/// to return *only* the matched key value in the key buffer, without
/// reading the data page. Faster than the full Get because it skips
/// the data-page read and the passive-concurrency bookkeeping.
///
/// Mapping: 5→55 Equal, 6→56 Next, 7→57 Previous, 8→58 GT, 9→59 GE,
/// 10→60 LT, 11→61 LE, 12→62 First, 13→63 Last.
///
/// **Parameters**: same as the underlying Get. Data buffer length is
/// ignored on entry; no record is returned.
///
/// **Prerequisites**: same as the underlying Get op.
///
/// **Returns**: same status codes as the underlying Get.
///
/// **Notes**: establishes logical currency at the matched key, but
/// **not** the physical currency required for Update or Delete — those
/// return status 8 until a non-Get-Key op refreshes physical currency.
/// Get Position also returns 8. We implement by dispatching to the base
/// op and then zeroing `*data_len` so the caller sees no record image.
pub(super) fn op_get_key(
    operation: u16,
    posblk: *mut c_void,
    data_buf: *mut c_void,
    data_len: *mut u32,
    key_buf: *mut c_void,
    key_num: i16,
) -> i32 {
    let base = operation.saturating_sub(50);
    strace!("op_get_key: op={} base={}", operation, base);

    // Save original data_len so we can restore it (app's buffer size)
    let original_dlen = if !data_len.is_null() {
        unsafe { *data_len }
    } else {
        0
    };

    let rc = match base {
        5 => op_get_equal(posblk, data_buf, data_len, key_buf, key_num),
        6 => op_get_next(posblk, data_buf, data_len, key_buf),
        7 => op_get_prev(posblk, data_buf, data_len, key_buf),
        8 => op_get_greater(posblk, data_buf, data_len, key_buf, key_num),
        9 => op_get_greater_or_equal(posblk, data_buf, data_len, key_buf, key_num),
        10 => op_get_less(posblk, data_buf, data_len, key_buf, key_num),
        11 => op_get_less_or_equal(posblk, data_buf, data_len, key_buf, key_num),
        12 => op_get_first(posblk, data_buf, data_len, key_buf, key_num),
        13 => op_get_last(posblk, data_buf, data_len, key_buf, key_num),
        _ => {
            strace!("op_get_key: invalid base op {}", base);
            return BTR_UNSUPPORTED_OP;
        }
    };

    if !data_len.is_null() {
        unsafe {
            *data_len = 0;
        }
    }
    let _ = original_dlen;

    strace!("op_get_key: rc={} (dlen zeroed)", rc);
    rc
}
