// ── Internal driver error codes ───────────────────────────────────────────────
pub const ERR_NOT_LOADED: i32 = 0x4651; // 18001 — driver not loaded
pub const ERR_CONTEXT_SETUP: i32 = 0x4652; // 18002 — connection setup failed
pub const ERR_CONTEXT_FAILURE: i32 = 0x07dc; //  2012 — ODBC execution error
pub const ERR_INVALID_OPTION: i32 = 0x008b; //   139 — bad MdsSetOption value

// ── Btrieve status codes (SPEC.md §7) ────────────────────────────────────────
pub const BTR_SUCCESS: i32 = 0;
pub const BTR_IO_ERROR: i32 = 2;
pub const BTR_FILE_NOT_OPEN: i32 = 3;
pub const BTR_KEY_NOT_FOUND: i32 = 4;
pub const BTR_DUPLICATE_KEY: i32 = 5;
pub const BTR_INVALID_KEY_NUM: i32 = 6;
pub const BTR_DIFF_KEY_NUM: i32 = 7; // key_num changed between Get and GetNext
pub const BTR_INVALID_POS: i32 = 8; // Get Key used before Delete
pub const BTR_EOF: i32 = 9;
pub const BTR_FILE_NOT_FOUND: i32 = 12;
pub const BTR_UNSUPPORTED_OP: i32 = 20;
pub const BTR_DATA_TOO_SHORT: i32 = 22; // data buffer too small for record
pub const BTR_CREATE_ERROR: i32 = 25;
pub const BTR_NOT_ALLOWED: i32 = 29; // read-only / permission
pub const BTR_TXN_ERROR: i32 = 36;
pub const BTR_TXN_ACTIVE: i32 = 37; // another transaction already active
pub const BTR_RECORD_LOCKED: i32 = 43;
pub const BTR_ACCESS_DENIED: i32 = 46;
pub const BTR_NO_TXN: i32 = 39; // no active transaction (for End/Abort)
