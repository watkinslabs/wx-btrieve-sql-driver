//! Btrieve opcode table — complete list of all operations.
//!
//! Source: Btrieve MicroKernel API specification.
//! Each entry: operation code, C constant name, and description.
//!
//! Bias values (added to base opcode):
//!   +50   KEY_BIAS        — Get Key (detect key presence without returning record)
//!   +100  S_WAIT_LOCK     — Single-record wait lock
//!   +200  S_NOWAIT_LOCK   — Single-record no-wait lock
//!   +300  M_WAIT_LOCK     — Multiple-record wait lock
//!   +400  M_NOWAIT_LOCK   — Multiple-record no-wait lock
//!   +500  NOWRITE_WAIT    — No-wait page lock (concurrent transactions)

use crate::constants::{BTR_SUCCESS, BTR_UNSUPPORTED_OP};

pub const STATUS_NOT_IMPLEMENTED: i32 = BTR_UNSUPPORTED_OP;

// ── Bias constants ────────────────────────────────────────────────────────────
pub const KEY_BIAS: u16 = 50;
pub const S_WAIT_LOCK: u16 = 100;
pub const S_NOWAIT_LOCK: u16 = 200;
pub const M_WAIT_LOCK: u16 = 300;
pub const M_NOWAIT_LOCK: u16 = 400;
pub const NOWRITE_WAIT: u16 = 500;

/// Implementation status.
#[derive(Copy, Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum OpStatus {
    /// Fully implemented against the SQL Server backend.
    Implemented,
    /// Partial implementation — no opcodes remain in this state.
    Partial,
    /// Stubbed as a success no-op with no backing work.
    NoOp,
    /// Not yet implemented.
    NotImplemented,
    /// Deliberately unsupported (e.g. deprecated in Btrieve 6.0+).
    Unsupported,
}

/// Full metadata for a Btrieve opcode.
#[derive(Copy, Clone, Debug)]
pub struct OpInfo {
    pub code: u16,
    pub name: &'static str,
    pub constant: &'static str,
    pub desc: &'static str,
    pub status: OpStatus,
}

/// Complete opcode table — every operation in the Btrieve spec.
pub const OPCODE_TABLE: &[OpInfo] = &[
    OpInfo {
        code: 0,
        name: "Open",
        constant: "B_OPEN",
        desc: "Makes a file available for access",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 1,
        name: "Close",
        constant: "B_CLOSE",
        desc: "Releases a file from availability",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 2,
        name: "Insert",
        constant: "B_INSERT",
        desc: "Inserts a new record into a file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 3,
        name: "Update",
        constant: "B_UPDATE",
        desc: "Updates the current record",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 4,
        name: "Delete",
        constant: "B_DELETE",
        desc: "Removes the current record from the file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 5,
        name: "Get Equal",
        constant: "B_GET_EQUAL",
        desc: "Returns the record whose key value matches the specified key value",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 6,
        name: "Get Next",
        constant: "B_GET_NEXT",
        desc: "Returns the record following the current record in the index path",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 7,
        name: "Get Previous",
        constant: "B_GET_PREVIOUS",
        desc: "Returns the record preceding the current record in the index path",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 8,
        name: "Get Greater Than",
        constant: "B_GET_GT",
        desc: "Returns the record whose key value is greater than the specified key value",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 9,
        name: "Get Greater Than or Equal",
        constant: "B_GET_GE",
        desc:
            "Returns the record whose key value is equal to or greater than the specified key value",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 10,
        name: "Get Less Than",
        constant: "B_GET_LT",
        desc: "Returns the record whose key value is less than the specified key value",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 11,
        name: "Get Less Than or Equal",
        constant: "B_GET_LE",
        desc: "Returns the record whose key value is equal to or less than the specified key value",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 12,
        name: "Get First",
        constant: "B_GET_FIRST",
        desc: "Returns the first record in the specified index path",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 13,
        name: "Get Last",
        constant: "B_GET_LAST",
        desc: "Returns the last record in the specified index path",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 14,
        name: "Create",
        constant: "B_CREATE",
        desc: "Creates a file with the specified characteristics",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 15,
        name: "Stat",
        constant: "B_STAT",
        desc: "Returns file and index characteristics, and number of records",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 16,
        name: "Extend",
        constant: "B_EXTEND",
        desc: "Divides a data file over two logical disk drives (deprecated in Btrieve 6.0+)",
        status: OpStatus::Unsupported,
    },
    OpInfo {
        code: 17,
        name: "Set Directory",
        constant: "B_SET_DIR",
        desc: "Sets the current directory to a specified path name",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 18,
        name: "Get Directory",
        constant: "B_GET_DIR",
        desc: "Returns the current directory for a specified logical disk drive",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 19,
        name: "Begin Transaction",
        constant: "B_BEGIN_TRAN",
        desc: "Marks the beginning of an exclusive transaction",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 20,
        name: "End Transaction",
        constant: "B_END_TRAN",
        desc: "Marks the end of a set of logically related operations",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 21,
        name: "Abort Transaction",
        constant: "B_ABORT_TRAN",
        desc: "Removes operations performed during an incomplete transaction",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 22,
        name: "Get Position",
        constant: "B_GET_POSITION",
        desc: "Returns the position of the current record",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 23,
        name: "Get Direct/Record",
        constant: "B_GET_DIRECT",
        desc: "Returns the record at a specified position (or chunk)",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 24,
        name: "Step Next",
        constant: "B_STEP_NEXT",
        desc: "Returns the record from the physical location following the current record",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 25,
        name: "Stop",
        constant: "B_STOP",
        desc: "Terminates the Workstation MicroKernel Engine",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 26,
        name: "Version",
        constant: "B_VERSION",
        desc: "Returns the version number of the MicroKernel Engine",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 27,
        name: "Unlock",
        constant: "B_UNLOCK",
        desc: "Unlocks a record or records",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 28,
        name: "Reset",
        constant: "B_RESET",
        desc: "Releases all resources held by a client",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 29,
        name: "Set Owner",
        constant: "B_SET_OWNER",
        desc: "Assigns an owner name to a file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 30,
        name: "Clear Owner",
        constant: "B_CLEAR_OWNER",
        desc: "Removes an owner name from a file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 31,
        name: "Create Index",
        constant: "B_BUILD_INDEX",
        desc: "Creates an index",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 32,
        name: "Drop Index",
        constant: "B_DROP_INDEX",
        desc: "Removes an index",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 33,
        name: "Step First",
        constant: "B_STEP_FIRST",
        desc: "Returns the record in the first physical location in the file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 34,
        name: "Step Last",
        constant: "B_STEP_LAST",
        desc: "Returns the record in the last physical location in the file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 35,
        name: "Step Previous",
        constant: "B_STEP_PREVIOUS",
        desc: "Returns the record in the physical location preceding the current record",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 36,
        name: "Get Next Extended",
        constant: "B_GET_NEXT_EXTENDED",
        desc: "Returns one or more records following the current record, with optional filter",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 37,
        name: "Get Previous Extended",
        constant: "B_GET_PREV_EXTENDED",
        desc: "Returns one or more records preceding the current record, with optional filter",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 38,
        name: "Step Next Extended",
        constant: "B_STEP_NEXT_EXT",
        desc: "Returns successive records from physical location following current, with filter",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 39,
        name: "Step Previous Extended",
        constant: "B_STEP_PREVIOUS_EXT",
        desc: "Returns successive records from physical location preceding current, with filter",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 40,
        name: "Insert Extended",
        constant: "B_EXT_INSERT",
        desc: "Inserts one or more records into a file",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 42,
        name: "Continuous Operation",
        constant: "B_CONTINUOUS",
        desc: "Allows system backups without closing active MicroKernel Engine files",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 44,
        name: "Get By Percentage",
        constant: "B_SEEK_PERCENT",
        desc: "Returns the record located approximately at a percentage position",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 45,
        name: "Find Percentage",
        constant: "B_GET_PERCENT",
        desc: "Returns a percentage figure based on the current record's position",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 53,
        name: "Update Chunk",
        constant: "B_CHUNK_UPDATE",
        desc: "Updates specified portions (chunks) of the current record",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 65,
        name: "Stat Extended",
        constant: "B_EXTENDED_STAT",
        desc: "Returns file names and paths of an extended file's components",
        status: OpStatus::Implemented,
    },
    // Get Key variants (+50 bias): detect key presence without returning record data
    OpInfo {
        code: 55,
        name: "Get Key Equal",
        constant: "B_GET_EQUAL+50",
        desc: "Detects presence of a key value without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 56,
        name: "Get Key Next",
        constant: "B_GET_NEXT+50",
        desc: "Detects next key without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 57,
        name: "Get Key Previous",
        constant: "B_GET_PREVIOUS+50",
        desc: "Detects previous key without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 58,
        name: "Get Key Greater",
        constant: "B_GET_GT+50",
        desc: "Detects first key > value without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 59,
        name: "Get Key Greater or Equal",
        constant: "B_GET_GE+50",
        desc: "Detects first key >= value without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 60,
        name: "Get Key Less",
        constant: "B_GET_LT+50",
        desc: "Detects first key < value without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 61,
        name: "Get Key Less or Equal",
        constant: "B_GET_LE+50",
        desc: "Detects first key <= value without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 62,
        name: "Get Key First",
        constant: "B_GET_FIRST+50",
        desc: "Detects first key without returning record data",
        status: OpStatus::Implemented,
    },
    OpInfo {
        code: 63,
        name: "Get Key Last",
        constant: "B_GET_LAST+50",
        desc: "Detects last key without returning record data",
        status: OpStatus::Implemented,
    },
    // Concurrent transaction form of Begin (6.x file format)
    OpInfo {
        code: 1019,
        name: "Begin Concurrent Transaction",
        constant: "B_BEGIN_TRAN (concurrent)",
        desc: "Begins a concurrent transaction (6.x file format)",
        status: OpStatus::Implemented,
    },
];

/// Strip all biases from an operation code to get the base opcode (0-99).
pub fn base_opcode(code: u16) -> u16 {
    // Biases: +50 (Get Key), +100/200/300/400 (record lock), +500 (page lock)
    // All biases are multiples of 50/100, so mod 50 with care, or just mod 100.
    code % 100
}

/// Detect the Get Key (+50) bias — if set, the op should not return record data.
pub fn has_key_bias(code: u16) -> bool {
    let without_locks = code % 100;
    (50..100).contains(&without_locks)
}

/// Detect lock bias.
pub fn lock_bias(code: u16) -> Option<u16> {
    match code / 100 {
        1 => Some(S_WAIT_LOCK),
        2 => Some(S_NOWAIT_LOCK),
        3 => Some(M_WAIT_LOCK),
        4 => Some(M_NOWAIT_LOCK),
        5 => Some(NOWRITE_WAIT),
        _ => None,
    }
}

/// Look up opcode metadata.  Tries the exact code first, then the base (mod 100).
pub fn opcode_info(code: u16) -> Option<&'static OpInfo> {
    OPCODE_TABLE
        .iter()
        .find(|o| o.code == code)
        .or_else(|| OPCODE_TABLE.iter().find(|o| o.code == base_opcode(code)))
}

/// Human-readable name for an opcode.
pub fn opcode_name(code: u16) -> String {
    opcode_info(code)
        .map(|o| o.name.to_string())
        .unwrap_or_else(|| format!("Unknown({})", code))
}

/// Check if the opcode is in implemented/partial state.
pub fn is_implemented(code: u16) -> bool {
    opcode_info(code)
        .map(|o| matches!(o.status, OpStatus::Implemented | OpStatus::Partial))
        .unwrap_or(false)
}

/// Handle stub opcodes: return SUCCESS for NoOp, UNSUPPORTED for the rest.
pub fn handle_stub(code: u16) -> i32 {
    match opcode_info(code) {
        Some(info) => match info.status {
            OpStatus::NoOp => BTR_SUCCESS,
            _ => STATUS_NOT_IMPLEMENTED,
        },
        None => STATUS_NOT_IMPLEMENTED,
    }
}
