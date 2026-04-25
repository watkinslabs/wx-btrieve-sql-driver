//! wxbtrv-core — platform-agnostic Btrieve op logic.
//!
//! This crate contains all the portable dispatch, SQL, INT-metadata, and record
//! codec code. It is consumed by the Windows `wxbtrv` cdylib (which adds the C
//! ABI exports and VDD glue) and is intended for reuse by test harnesses and a
//! future Linux driver port.

pub mod constants;
pub mod opcode_map;
pub mod ops;
pub mod record;
pub mod sql;
pub mod sqlite_meta;
pub mod state;
pub mod table_lookup;
pub mod trace;
pub mod util;
