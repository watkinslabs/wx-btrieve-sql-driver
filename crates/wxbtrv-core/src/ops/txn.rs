//! Transaction ops — 19 BeginTxn, 20 EndTxn, 21 AbortTxn, 1019 BeginConcurrentTxn.
//!
//! Mapped straight onto SQL Server transactions on the shared ODBC connection
//! (crate::sql). Exclusive (19) uses SERIALIZABLE isolation; concurrent (1019)
//! uses READ COMMITTED. End/Abort commit/rollback respectively. Abort also
//! invalidates currency on every open handle — any recnum captured in the
//! rolled-back txn may be a ghost.

use super::helpers::strace;
use crate::constants::BTR_SUCCESS;
use crate::sql::{abort_txn, begin_txn, commit_txn};
use crate::state::state;

/// Clear every handle's cached currency — called by op_abort_txn because
/// rows observed inside the rolled-back txn may no longer exist.
fn invalidate_all_currency() {
    if let Ok(mut st) = state().lock() {
        for h in st.handles.values_mut() {
            h.step_last_recnum = None;
            h.step_cache.clear();
            h.get_last_keys.clear();
            h.get_last_desc.clear();
            h.get_last_recnum = None;
            h.last_recnum = None;
        }
    }
}

/// **Op 19 — Begin Transaction** (`B_BEGIN_TRAN`, exclusive)
///
/// Opens a SERIALIZABLE SQL Server transaction on the shared connection.
/// All subsequent Insert/Update/Delete on any open handle become atomic
/// until End Transaction (20) or Abort Transaction (21).
///
/// Returns: 0 success, 37 txn already active.
pub(super) fn op_begin_txn() -> i32 {
    match begin_txn("SERIALIZABLE") {
        Ok(()) => {
            strace!("op_begin_txn: BEGIN TRANSACTION SERIALIZABLE");
            BTR_SUCCESS
        }
        Err(e) => {
            strace!("op_begin_txn: FAIL rc={}", e);
            e
        }
    }
}

/// **Op 1019 — Begin Concurrent Transaction** (`B_BEGIN_TRAN` concurrent)
///
/// Same as op 19 but with READ COMMITTED isolation — page-level concurrent
/// semantics, multiple writers allowed.
///
/// Returns: 0 success, 37 txn already active.
pub(super) fn op_begin_concurrent_txn() -> i32 {
    match begin_txn("READ COMMITTED") {
        Ok(()) => {
            strace!("op_begin_concurrent_txn: BEGIN TRANSACTION READ COMMITTED");
            BTR_SUCCESS
        }
        Err(e) => {
            strace!("op_begin_concurrent_txn: FAIL rc={}", e);
            e
        }
    }
}

/// **Op 20 — End Transaction** (`B_END_TRAN`)
///
/// Commits the active transaction. All writes since Begin become permanent.
///
/// Returns: 0 success, 39 no active transaction.
pub(super) fn op_end_txn() -> i32 {
    match commit_txn() {
        Ok(()) => {
            strace!("op_end_txn: COMMIT");
            BTR_SUCCESS
        }
        Err(e) => {
            strace!("op_end_txn: FAIL rc={}", e);
            e
        }
    }
}

/// **Op 21 — Abort Transaction** (`B_ABORT_TRAN`)
///
/// Rolls back the active transaction and invalidates currency on every
/// open handle — any row observed during the txn may no longer exist.
///
/// Returns: 0 success, 39 no active transaction.
pub(super) fn op_abort_txn() -> i32 {
    match abort_txn() {
        Ok(()) => {
            invalidate_all_currency();
            strace!("op_abort_txn: ROLLBACK; currency invalidated on all handles");
            BTR_SUCCESS
        }
        Err(e) => {
            strace!("op_abort_txn: FAIL rc={}", e);
            e
        }
    }
}
