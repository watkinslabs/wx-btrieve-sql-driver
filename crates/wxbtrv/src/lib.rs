//! wxbtrv — Windows cdylib shim around `wxbtrv-core`.
//!
//! This crate is the actual production DLL. It contains only:
//!   * C ABI wrappers (`BTRCALL`, `BTRCALLID`, `_BTRCALL`, `_BTRCALLID`) that
//!     forward into `wxbtrv_core::ops::btrcall_internal`.
//!   * The `obtrDriver` / `obtrTrace` / etc data exports.
//!   * `DllMain`, VDD glue (`vdd.rs`), and the non-op `WB*` / `DBU*` / `Mds*`
//!     exports (`exports_misc.rs`).
//!
//! All platform-agnostic op logic lives in `wxbtrv-core`.

extern crate wxbtrv_core;

#[cfg(target_os = "windows")]
mod vdd;

mod exports_misc;
mod ops_exports;

// Re-export everything from the shim's submodules. The `#[unsafe(no_mangle)]`
// items inside them are what end up in the cdylib's export table.
#[allow(unused_imports)]
pub use exports_misc::*;
#[allow(unused_imports)]
pub use ops_exports::*;

#[cfg(target_os = "windows")]
use core::ffi::c_void;
#[cfg(target_os = "windows")]
use std::fs;

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    _hinst_dll: *mut c_void,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if fdw_reason == DLL_PROCESS_ATTACH {
        let msg = "rust-mer loaded\n";
        let paths = [
            r"C:\PVSW\bin\rust_mer_loaded.txt",
            r"C:\Windows\Temp\rust_mer_loaded.txt",
            "rust_mer_loaded.txt",
        ];
        for p in paths {
            if fs::write(p, msg).is_ok() {
                break;
            }
        }
        wxbtrv_core::trace::trace("DllMain process attach");
    }
    1
}
