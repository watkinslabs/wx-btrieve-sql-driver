use core::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
/// VDD (Virtual Device Driver) interface for NTVDM.
///
/// By exporting VDDInitialize / VDDDispatch / VDDRegisterInit, wxbtrv.dll
/// itself becomes the VDD — loaded directly by wxbtrv.sys via RegisterModule.
///
/// NTVDM calls:
///   VDDInitialize  — once when the VDD is loaded
///   VDDDispatch    — each time the DOS driver fires BOP 58h (INT 7B)
///   VDDRegisterInit — optional, called after init
///
/// VDDDispatch reads the DOS CPU state via ntvdm.exe exported functions,
/// converts DOS segment:offset pointers to 32-bit linear addresses via
/// MGetVdmPointer, then calls our internal BTRCALL.
///
/// NTVDM import strategy:
///   Rust's `raw-dylib` appends ".dll" to names that don't already end in
///   ".dll", turning "ntvdm.exe" into "ntvdm.exe.dll" in the import table.
///   Windows then fails to resolve the import because the running module is
///   "ntvdm.exe", not "ntvdm.exe.dll", and LoadLibrary fails silently.
///
///   Fix: resolve all ntvdm.exe exports at runtime via GetModuleHandleA /
///   GetProcAddress.  Since we ARE running inside ntvdm.exe, the module is
///   always loaded; we just need its handle.
use wxbtrv_core::trace::{
    cstr_from_raw, data_as_text, hex_bytes, key_as_ascii, op_name, set_seq, trace, CALL_SEQ,
};

// ── kernel32 / user32 ─────────────────────────────────────────────────────────
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleA(module_name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, proc_name: *const u8) -> *mut c_void;
}

// user32 MessageBoxA was previously linked for interactive crash dialogs during
// early bring-up; it's no longer used now that VDDInitialize logs to disk.
// Kept here as a comment for future debugging.
// #[link(name = "user32")]
// unsafe extern "system" {
//     fn MessageBoxA(hwnd: *mut c_void, text: *const u8, caption: *const u8, flags: u32) -> i32;
// }

// ── NTVDM function pointers ───────────────────────────────────────────────────
// Resolved once at VDDInitialize time from the ntvdm.exe module.

struct NtvdmFns {
    get_ds: unsafe extern "system" fn() -> u16,
    get_dx: unsafe extern "system" fn() -> u16,
    set_ax: unsafe extern "system" fn(u16),
    m_get_vdm_pointer: unsafe extern "system" fn(u32, u32, u32) -> *mut c_void,
}

static NTVDM: OnceLock<Option<NtvdmFns>> = OnceLock::new();

fn ntvdm() -> Option<&'static NtvdmFns> {
    NTVDM
        .get_or_init(|| unsafe { resolve_ntvdm_fns() })
        .as_ref()
}

unsafe fn resolve_ntvdm_fns() -> Option<NtvdmFns> {
    let h = GetModuleHandleA(b"ntvdm.exe\0".as_ptr());
    if h.is_null() {
        trace("ERROR: GetModuleHandleA(ntvdm.exe) returned NULL — not running inside NTVDM?");
        return None;
    }

    macro_rules! proc {
        ($h:expr, $name:literal, $ty:ty) => {{
            let p = GetProcAddress($h, concat!($name, "\0").as_bytes().as_ptr());
            if p.is_null() {
                trace(concat!("ERROR: GetProcAddress failed for ", $name));
                return None;
            }
            std::mem::transmute::<*mut c_void, $ty>(p)
        }};
    }

    Some(NtvdmFns {
        get_ds: proc!(h, "getDS", unsafe extern "system" fn() -> u16),
        get_dx: proc!(h, "getDX", unsafe extern "system" fn() -> u16),
        set_ax: proc!(h, "setAX", unsafe extern "system" fn(u16)),
        m_get_vdm_pointer: proc!(
            h,
            "MGetVdmPointer",
            unsafe extern "system" fn(u32, u32, u32) -> *mut c_void
        ),
    })
}

/// DOS Btrieve call parameter block (26 bytes) at DS:DX.
/// The DOS driver fills this in before firing BOP 58h.
///
/// All pointer fields are DOS segment:offset pairs (stored as u32 = seg<<16|off).
///
/// Verified layout (DOS Btrieve calling convention):
///   0x00  posblk_ptr    u32   position block seg:off
///   0x04  data_len      u16   data buffer length (in/out)
///   0x06  data_buf_ptr  u32   data buffer seg:off
///   0x0A  acs_ptr       u32   ACS buffer seg:off (usually 0)
///   0x0E  op_code       u16   Btrieve operation code
///   0x10  key_buf_ptr   u32   key buffer seg:off
///   0x14  key_num       u16   key number (LE 16-bit: high byte=flag, low byte=key length or index)
///   0x16  client_id_ptr u32   client ID seg:off
#[repr(C, packed)]
struct BtrCallBlock {
    data_buf_ptr: u32,  // +0x00 — seg:off of data buffer (WAS INCORRECTLY posblk_ptr!)
    data_len: u16,      // +0x04
    posblk_ptr: u32,    // +0x06 — seg:off of position block (WAS INCORRECTLY data_buf_ptr!)
    acs_ptr: u32,       // +0x0A
    op_code: u16,       // +0x0E
    key_buf_ptr: u32,   // +0x10
    key_length: u8,     // +0x14 — key buffer length
    key_number: u8,     // +0x15 — key path number
    client_id_ptr: u32, // +0x16 — seg:off of status/client-ID location
    status: u16,        // +0x1A
}

/// VDDInitialize — called once by NTVDM when the VDD is first loaded.
/// Always returns TRUE. NTVDM function pointers are resolved lazily on first
/// VDDDispatch call, not here, so init never fails even in test contexts.
#[unsafe(no_mangle)]
pub extern "system" fn VDDInitialize(
    _h_vdd: *mut c_void,
    _reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    // Raw Win32 diagnostic — write marker file WITHOUT going through Rust std,
    // so we can confirm VDDInitialize is reached even if std panics in NTVDM.
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateFileA(
                path: *const u8,
                access: u32,
                share: u32,
                sec: *mut c_void,
                disp: u32,
                flags: u32,
                tmpl: *mut c_void,
            ) -> *mut c_void;
            fn WriteFile(
                h: *mut c_void,
                buf: *const u8,
                n: u32,
                written: *mut u32,
                ov: *mut c_void,
            ) -> i32;
            fn CloseHandle(h: *mut c_void) -> i32;
        }
        let path = b"C:\\WatkinsX\\logs\\vdd_init.txt\0";
        let h = CreateFileA(
            path.as_ptr(),
            0x40000000,
            0,
            core::ptr::null_mut(),
            2,
            0x80,
            core::ptr::null_mut(),
        );
        if !h.is_null() && h as isize != -1 {
            let msg = b"VDDInitialize called\r\n";
            let mut w = 0u32;
            WriteFile(
                h,
                msg.as_ptr(),
                msg.len() as u32,
                &mut w,
                core::ptr::null_mut(),
            );
            CloseHandle(h);
        }
    }
    trace("VDDInitialize called");
    wxbtrv_core::ops::ensure_init_vdd();
    1 // TRUE — always succeed; NTVDM fn resolution happens lazily in VDDDispatch
}

/// VDDRegisterInit — optional, called after VDDInitialize during RegisterModule.
#[unsafe(no_mangle)]
pub extern "system" fn VDDRegisterInit() {
    trace("VDDRegisterInit called");
}

/// VDDDispatch — called by NTVDM each time the DOS driver fires BOP 58h (INT 7B).
///
/// Convention (matches BTRVDD.DLL reverse-engineered behaviour):
///   DS:DX → BtrCallBlock (28 bytes)
///   AX    ← return code written back to DOS AX register
#[unsafe(no_mangle)]
pub extern "system" fn VDDDispatch() {
    let fns = match ntvdm() {
        Some(f) => f,
        None => {
            trace("VDDDispatch: NTVDM fns not available");
            return;
        }
    };

    // Read DS:DX from the V86 CPU state
    let (ds, dx) = unsafe { ((fns.get_ds)(), (fns.get_dx)()) };
    let seg_off: u32 = ((ds as u32) << 16) | (dx as u32);

    // Map the call block into our 32-bit address space
    let blk_ptr =
        unsafe { (fns.m_get_vdm_pointer)(seg_off, core::mem::size_of::<BtrCallBlock>() as u32, 0) };
    if blk_ptr.is_null() {
        trace("VDDDispatch: MGetVdmPointer returned null");
        unsafe { (fns.set_ax)(0xFFFF) };
        return;
    }

    let blk = unsafe { &*(blk_ptr as *const BtrCallBlock) };
    let op = {
        let v = blk.op_code;
        v
    };
    // BTRVDD passes key_number (index path 0-118) directly to BTRCALL.
    // key_length is the buffer size, NOT combined with key_number.
    let key_num: i16 = blk.key_number as i8 as i16;

    // Map DOS segment:offset pointers to 32-bit linear addresses (mode=0 = V86)
    // CORRECTED: offset 0x00 = data_buf_ptr, offset 0x06 = posblk_ptr (verified from BTRVDD disasm)
    let posblk = unsafe { (fns.m_get_vdm_pointer)(blk.posblk_ptr, 128, 0) };
    let data = unsafe { (fns.m_get_vdm_pointer)(blk.data_buf_ptr, blk.data_len as u32, 0) };
    let keybuf = unsafe { (fns.m_get_vdm_pointer)(blk.key_buf_ptr, 255, 0) };
    let mut dlen: u32 = blk.data_len as u32;
    let acs = blk.acs_ptr as i32 as *mut i8;

    // Assign sequence number and store for use inside ops/sql trace lines
    let seq = CALL_SEQ.fetch_add(1, Ordering::Relaxed);
    set_seq(seq);

    let op_base = op % 100;
    let op_nm = op_name(op);

    // Use separate key fields directly from BtrCallBlock
    let disp_keynum = blk.key_number;
    let disp_keylen = blk.key_length;

    // ── Pre-call: match DEV B tracer format exactly ────────────────────────────
    if op_base == 0 {
        let path = cstr_from_raw(keybuf as *const u8, 255);
        trace(&format!(
            "#{seq} >> Open path={path:?} keynum={disp_keynum} keylen={disp_keylen} dlen_in={dlen}"
        ));
    } else {
        let fname = wxbtrv_core::state::handle_name(posblk as u32);
        let key_str = key_as_ascii(keybuf as *const u8, 64);
        trace(&format!("#{seq} >> {op_nm} handle={fname:?} key={key_str} keynum={disp_keynum} keylen={disp_keylen} dlen_in={dlen}"));
    }
    // Raw BTRCALL hex dump (matches DEV B format)
    let posblk_hex = hex_bytes(posblk as *const u8, 8);
    let keybuf_hex = hex_bytes(keybuf as *const u8, 255);
    trace(&format!("#{seq} BTRCALL op={op:#06x}({op_nm}) posblk=[{posblk_hex}] dlen_in={dlen} key=[{keybuf_hex}] keynum={disp_keynum} keylen={disp_keylen}"));

    // Dispatch into local buffers (data and keybuf are our private copies)
    let rc = wxbtrv_core::ops::btrcall_internal(op, posblk, data, &mut dlen, keybuf, key_num, acs);

    // No copy-back needed — with corrected BtrCallBlock layout (offset 0x00 = data_buf_ptr),
    // data and key buffers are in separate non-overlapping DOS memory regions.

    // Handle ID is written to posblk inside op_open — no need to do it here.

    // Raw BTRCALL result line (matches DEV B format)
    if dlen > 0 {
        let data_hex = hex_bytes(data as *const u8, dlen as usize);
        trace(&format!(
            "#{seq} BTRCALL => rc={rc} dlen_out={dlen} data=[{data_hex}]"
        ));
    } else {
        trace(&format!(
            "#{seq} BTRCALL => rc={rc} dlen_out={dlen} data=<null>"
        ));
    }

    // Human-readable summary (matches DEV B << format)
    if rc == 0 {
        if op_base == 0 {
            let path = cstr_from_raw(keybuf as *const u8, 255);
            trace(&format!("#{seq} << Open OK path={path:?}"));
        } else if dlen > 0 {
            let text = data_as_text(data as *const u8, (dlen as usize).min(80));
            trace(&format!(
                "#{seq} << {op_nm} OK dlen_out={dlen} data_text={text}"
            ));
        } else {
            trace(&format!("#{seq} << {op_nm} OK"));
        }
    } else {
        if op_base == 0 {
            let path = cstr_from_raw(keybuf as *const u8, 255);
            trace(&format!("#{seq} << Open FAILED rc={rc} path={path:?}"));
        } else {
            trace(&format!("#{seq} << {op_nm} rc={rc}"));
        }
    }

    // Write data_len back to BtrCallBlock offset 4 (in/out parameter)
    unsafe {
        let dlen_ptr = (blk_ptr as *mut u8).add(4) as *mut u16;
        *dlen_ptr = dlen as u16;
    }

    // Write status code to the location pointed to by client_id_ptr (BtrCallBlock offset 0x16).
    // BTRVDD.DLL maps client_id_ptr via MGetVdmPointer(4 bytes) and writes the status as u16.
    // Confirmed by disassembly: BTRVDD.DLL does NOT import setAX — status goes here.
    let status_segoff = blk.client_id_ptr;
    if status_segoff != 0 {
        let status_dos = unsafe { (fns.m_get_vdm_pointer)(status_segoff, 4, 0) };
        if !status_dos.is_null() {
            unsafe {
                core::ptr::write_unaligned(status_dos as *mut u16, rc as u16);
            }
        }
    }

    // Final: trace what we actually wrote back to DOS memory
    let posblk_final = hex_bytes(posblk as *const u8, 8);
    let data_final = if !data.is_null() && dlen > 0 {
        hex_bytes(data as *const u8, dlen as usize)
    } else {
        String::from("<null>")
    };
    let key_final = hex_bytes(keybuf as *const u8, 16);
    trace(&format!("#{seq} DOS-WRITEBACK rc={rc} dlen_wb={dlen} posblk=[{posblk_final}] keybuf=[{key_final}] data=[{}]",
        if dlen > 32 { format!("{}..+{}b", hex_bytes(data as *const u8, 32), dlen - 32) } else { data_final }));
}
