//! wxbtrv-core op dispatch — platform-agnostic Btrieve call dispatcher.
//!
//! The actual handlers live in per-op sibling modules. The Windows-facing
//! `BTRCALL` / `BTRCALLID` / `_BTRCALL` / `_BTRCALLID` C ABI wrappers live in
//! the `wxbtrv` shim crate and delegate to `btrcall_internal` here.

pub mod opcodes;

mod close;
mod create;
mod delete;
mod get;
mod get_ext;
mod get_key;
mod helpers;
mod index;
mod insert;
mod open;
mod owner;
mod position;
mod session;
pub mod sql_helpers;
mod stat;
mod step;
mod step_ext;
mod txn;
mod update;

use crate::constants;
use crate::state::{set_err, INIT_COUNT, IS_INITIALIZED};
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use helpers::strace;

// Note: `obtrDriver` / `obtrTrace` / `obtrAvailableSQLServers` /
// `obtrAvailableSQLServerName` — the DLL-exported data symbols used by
// MdsSetOption etc. — live in the Windows `wxbtrv` shim, not here. They are
// not referenced by any dispatch path in wxbtrv-core.

/// Global handle counter — each Open gets a unique ID, matching BTRVDD.DLL behavior.
pub(crate) static HANDLE_CTR: AtomicU32 = AtomicU32::new(1);

// ── Init ──────────────────────────────────────────────────────────────────────

pub fn do_init() -> bool {
    strace!("init: build={}", env!("CARGO_PKG_VERSION"));
    strace!(
        "init: compiled={} {}",
        option_env!("BUILD_DATE").unwrap_or("unknown"),
        option_env!("BUILD_TIME").unwrap_or("unknown")
    );
    let n = crate::sqlite_meta::preload_from_sqlite().unwrap_or(0);
    strace!("init: sqlite_tables={}", n);
    n > 0
}

fn ensure_init() {
    if IS_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    do_init();
    INIT_COUNT.fetch_add(1, Ordering::AcqRel);
    IS_INITIALIZED.store(true, Ordering::Release);
}

/// Called from VDDInitialize — ensures the SQLite config and tables are loaded.
pub fn ensure_init_vdd() {
    ensure_init();
}

/// Platform-agnostic BTRCALL dispatcher. All the per-op routing lives here.
///
/// The Windows `wxbtrv` shim exports `BTRCALL` / `BTRCALLID` / `_BTRCALL` /
/// `_BTRCALLID` as `extern "system"` C ABI functions that are thin wrappers
/// around this `pub fn`.
/// # Safety
/// All non-null pointer arguments must point at valid Btrieve-call-shaped
/// buffers. The C ABI shims (`BTRCALL`, `_BTRCALL`, `BTRCALLID`,
/// `_BTRCALLID`) own that contract on behalf of the legacy DOS app.
pub unsafe fn btrcall_internal(
    operation: u16,
    position_block: *mut c_void,
    data_buffer: *mut c_void,
    data_len: *mut u32,
    key_buffer: *mut c_void,
    key_num: i16,
    _acs: *mut i8,
) -> i32 {
    ensure_init();
    crate::trace::CALL_SEQ.fetch_add(1, Ordering::Relaxed);
    crate::trace::set_seq(crate::trace::CALL_SEQ.load(Ordering::Relaxed));

    // Special-case ops that live outside the 1..5 lock-bias range so the
    // generic strip below doesn't misfold them. 1019 is the concurrent form
    // of Begin Transaction; it must dispatch to its own handler directly.
    if operation == 1019 {
        let rc = txn::op_begin_concurrent_txn();
        set_err(rc);
        return rc;
    }
    // Op 16 (Extend) is deprecated in Btrieve 6.0+. Dispatch it explicitly
    // and return "unsupported" with a clear log, rather than falling through
    // to the generic stub handler which would also return 20 but silently.
    if operation == 16 {
        strace!("BTRCALL op=16 Extend — deprecated in Btrieve 6.0+ spec, returning UNSUPPORTED");
        set_err(constants::BTR_UNSUPPORTED_OP);
        return constants::BTR_UNSUPPORTED_OP;
    }

    // Strip lock-bias: ops +100, +200, +300, +400 are locking variants of base ops.
    let base_op = if (100..500).contains(&operation) {
        operation % 100
    } else {
        operation
    };

    let dlen_in = if data_len.is_null() {
        0
    } else {
        unsafe { *data_len }
    };
    if operation == 0 || base_op == 17 {
        let path = unsafe { crate::trace::cstr_from_raw(key_buffer as *const u8, 80) };
        strace!(
            "BTRCALL op={} ({}) path={:?} posblk={:?} dlen_in={} kn={}",
            operation,
            opcodes::opcode_name(operation),
            path,
            position_block,
            dlen_in,
            key_num
        );
    } else {
        strace!(
            "BTRCALL op={} ({}) posblk={:?} dlen_in={} kn={}",
            operation,
            opcodes::opcode_name(operation),
            position_block,
            dlen_in,
            key_num
        );
    }

    let rc = match base_op {
        // File / session ops
        0 => open::op_open(position_block, key_buffer),
        1 => close::op_close(position_block),
        15 => stat::op_stat(position_block, data_buffer, data_len),
        25 => session::op_stop(),
        26 => session::op_version(data_buffer, data_len),
        // Key-ordered navigation
        5 => get::op_get_equal(position_block, data_buffer, data_len, key_buffer, key_num),
        6 => get::op_get_next(position_block, data_buffer, data_len, key_buffer),
        7 => get::op_get_prev(position_block, data_buffer, data_len, key_buffer),
        8 => get::op_get_greater(position_block, data_buffer, data_len, key_buffer, key_num),
        9 => {
            get::op_get_greater_or_equal(position_block, data_buffer, data_len, key_buffer, key_num)
        }
        10 => get::op_get_less(position_block, data_buffer, data_len, key_buffer, key_num),
        11 => get::op_get_less_or_equal(position_block, data_buffer, data_len, key_buffer, key_num),
        12 => get::op_get_first(position_block, data_buffer, data_len, key_buffer, key_num),
        13 => get::op_get_last(position_block, data_buffer, data_len, key_buffer, key_num),
        // Physical-order navigation
        24 => step::op_step_next(position_block, data_buffer, data_len),
        33 => step::op_step_first(position_block, data_buffer, data_len),
        34 => step::op_step_last(position_block, data_buffer, data_len),
        35 => step::op_step_prev(position_block, data_buffer, data_len),
        // Write ops
        2 => insert::op_insert(position_block, data_buffer as *const c_void, data_len),
        3 => update::op_update(position_block, data_buffer as *const c_void, data_len),
        4 => delete::op_delete(position_block),
        // Transactions
        19 => txn::op_begin_txn(),
        20 => txn::op_end_txn(),
        21 => txn::op_abort_txn(),
        // Position / direct access
        22 => position::op_get_position(position_block, data_buffer, data_len),
        23 => position::op_get_direct(position_block, data_buffer, data_len, key_buffer),
        // Locking / misc
        27 => session::op_unlock(position_block),
        28 => session::op_reset(position_block),
        // Session directory
        17 => session::op_set_dir(key_buffer as *const c_void),
        18 => session::op_get_dir(data_buffer, data_len, key_buffer as *const c_void),
        // Percentage positioning
        // Note: these are swapped relative to the internal function names.
        // Btrieve op 44 (Get By Percent): input pct -> returns record at that pct.
        // Btrieve op 45 (Find Percent):   input record/key -> returns percentile.
        // Internally, op_find_percent() does the "input pct -> record" work and
        // op_get_percent() computes the percentile of the current record.
        44 => position::op_find_percent(position_block, data_buffer, data_len),
        45 => position::op_get_percent(position_block, data_buffer, data_len),
        // Create / Owner / Index
        14 => create::op_create(data_buffer, data_len, key_buffer, key_num),
        29 => owner::op_set_owner(position_block, data_buffer, data_len, key_num),
        30 => owner::op_clear_owner(position_block),
        31 => index::op_create_index(position_block, data_buffer, data_len),
        32 => index::op_drop_index(position_block, key_num),
        // Extended Get / Step / Insert / Update / Stat
        36 => get_ext::op_get_next_extended(position_block, data_buffer, data_len),
        37 => get_ext::op_get_prev_extended(position_block, data_buffer, data_len),
        38 => step_ext::op_step_next_extended(position_block, data_buffer, data_len),
        39 => step_ext::op_step_prev_extended(position_block, data_buffer, data_len),
        40 => insert::op_insert_extended(position_block, data_buffer, data_len),
        42 => session::op_continuous_operation(data_buffer, data_len, key_num),
        53 => update::op_update_chunk(position_block, data_buffer as *const c_void, data_len),
        65 => stat::op_stat_extended(position_block, data_buffer, data_len),
        // Get Key variants (+50 bias).
        55..=63 => get_key::op_get_key(
            operation,
            position_block,
            data_buffer,
            data_len,
            key_buffer,
            key_num,
        ),
        // Everything else: look up in the opcode table for the right stub behavior.
        _ => {
            let info = opcodes::opcode_info(operation);
            let name = info.map(|i| i.name).unwrap_or("Unknown");
            let status = info
                .map(|i| format!("{:?}", i.status))
                .unwrap_or_else(|| "Unknown".to_string());
            strace!("BTRCALL: op={} ({}) status={}", operation, name, status);
            opcodes::handle_stub(operation)
        }
    };
    set_err(rc);
    rc
}
