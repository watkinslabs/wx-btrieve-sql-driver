mod debug_trace {
    use std::cell::Cell;
    use std::fs::{self, File, OpenOptions};
    use std::io::{BufWriter, Write};
    use std::slice;
    use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
    use std::sync::{Mutex, OnceLock};

    static TRACE_WRITER: OnceLock<Mutex<BufWriter<File>>> = OnceLock::new();
    static TRACE_STATE: AtomicU8 = AtomicU8::new(0); // 0=uninit,1=ready,2=disabled
    static TRACE_LEVEL: AtomicU8 = AtomicU8::new(LEVEL_UNRESOLVED);
    pub static CALL_SEQ: AtomicU32 = AtomicU32::new(0);

    // Trace levels — higher numbers = more verbose. Compared against
    // WXBTRV_TRACE_LEVEL at runtime; if unset, debug builds default to DEBUG
    // and release builds default to OFF (back-compat with the previous
    // #[cfg(debug_assertions)] gate).
    pub const LEVEL_OFF: u8 = 0;
    pub const LEVEL_ERROR: u8 = 1;
    pub const LEVEL_INFO: u8 = 2;
    pub const LEVEL_DEBUG: u8 = 3;
    const LEVEL_UNRESOLVED: u8 = 0xFF;

    fn resolve_level() -> u8 {
        // Env var takes precedence — operators can turn tracing on in a
        // release build without rebuilding.
        if let Ok(v) = std::env::var("WXBTRV_TRACE_LEVEL") {
            return match v.trim().to_ascii_lowercase().as_str() {
                "off" | "0" | "none" | "" => LEVEL_OFF,
                "error" | "err" | "1" => LEVEL_ERROR,
                "info" | "2" => LEVEL_INFO,
                "debug" | "dbg" | "3" | "trace" => LEVEL_DEBUG,
                _ => LEVEL_OFF,
            };
        }
        // No env var → compile-time default: debug builds log at DEBUG,
        // release builds stay silent.
        if cfg!(debug_assertions) {
            LEVEL_DEBUG
        } else {
            LEVEL_OFF
        }
    }

    /// Current trace level, resolved once on first call.
    pub fn trace_level() -> u8 {
        let cached = TRACE_LEVEL.load(Ordering::Relaxed);
        if cached != LEVEL_UNRESOLVED {
            return cached;
        }
        let lvl = resolve_level();
        TRACE_LEVEL.store(lvl, Ordering::Relaxed);
        lvl
    }

    /// Whether a message at `msg_level` would be emitted right now.
    pub fn trace_enabled(msg_level: u8) -> bool {
        trace_level() >= msg_level
    }

    thread_local! {
        static CURRENT_SEQ: Cell<u32> = Cell::new(0);
    }

    pub fn set_seq(s: u32) {
        CURRENT_SEQ.with(|c| c.set(s));
    }
    pub fn get_seq() -> u32 {
        CURRENT_SEQ.with(|c| c.get())
    }

    #[cfg(target_os = "windows")]
    fn open_file() -> Option<File> {
        let log_dir = r"C:\WatkinsX\logs";
        let _ = fs::create_dir_all(log_dir);
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let stamped = format!(r"{}\wxbtrv_{}.log", log_dir, ts);
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&stamped) {
            return Some(f);
        }
        for p in [
            r"C:\WatkinsX\bin\wxbtrv_trace.log",
            r"C:\Windows\Temp\wxbtrv_trace.log",
            "wxbtrv_trace.log",
        ] {
            if let Ok(f) = OpenOptions::new().create(true).append(true).open(p) {
                return Some(f);
            }
        }
        None
    }

    /// Non-Windows trace sink: honor `WXBTRV_TRACE_LOG` env var, otherwise
    /// write to `/tmp/wxbtrv_<unix_ts>.log`, with `/tmp/wxbtrv_trace.log` as
    /// a last-ditch fallback.
    #[cfg(not(target_os = "windows"))]
    fn open_file() -> Option<File> {
        use std::time::{SystemTime, UNIX_EPOCH};
        if let Ok(path) = std::env::var("WXBTRV_TRACE_LOG") {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
                return Some(f);
            }
        }
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let stamped = format!("/tmp/wxbtrv_{}.log", ts);
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&stamped) {
            return Some(f);
        }
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/wxbtrv_trace.log")
        {
            return Some(f);
        }
        None
    }

    pub fn trace(msg: &str) {
        // Honor the runtime trace level. LEVEL_OFF short-circuits before we
        // touch any I/O — this is also the default in release builds.
        if trace_level() == LEVEL_OFF {
            return;
        }
        if TRACE_STATE.load(Ordering::Acquire) == 2 {
            return;
        }
        let lock = TRACE_WRITER.get_or_init(|| {
            if let Some(f) = open_file() {
                TRACE_STATE.store(1, Ordering::Release);
                Mutex::new(BufWriter::new(f))
            } else {
                TRACE_STATE.store(2, Ordering::Release);
                #[cfg(target_os = "windows")]
                let null_path = "NUL";
                #[cfg(not(target_os = "windows"))]
                let null_path = "/dev/null";
                let fallback = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(null_path)
                    .expect("open null device");
                Mutex::new(BufWriter::new(fallback))
            }
        });
        if TRACE_STATE.load(Ordering::Acquire) != 1 {
            return;
        }
        if let Ok(mut w) = lock.lock() {
            let _ = writeln!(w, "{} {msg}", iso_ts_ms());
            let _ = w.flush();
        }
    }

    /// ISO 8601 UTC timestamp with millisecond precision, e.g. `2026-04-12T14:05:07.123Z`.
    /// Used to prefix every trace line so synthesis can interleave with the
    /// MCP server's marks.log sidecar by wall-clock time.
    fn iso_ts_ms() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = d.as_secs();
        let ms = d.subsec_millis();
        let days = (secs / 86_400) as i64;
        let tod = secs % 86_400;
        let hh = tod / 3600;
        let mm = (tod % 3600) / 60;
        let ss = tod % 60;
        // Civil-from-days (Howard Hinnant, public-domain algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d_ = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            y, m, d_, hh, mm, ss, ms
        )
    }

    pub fn op_name(op: u16) -> &'static str {
        match op % 100 {
            0 => "Open",
            1 => "Close",
            2 => "Insert",
            3 => "Update",
            4 => "Delete",
            5 => "GetEqual",
            6 => "GetNext",
            7 => "GetPrev",
            8 => "GetGT",
            9 => "GetGE",
            10 => "GetLT",
            11 => "GetLE",
            12 => "GetFirst",
            13 => "GetLast",
            14 => "Create",
            15 => "Stat",
            16 => "Extend",
            17 => "SetDir",
            18 => "GetDir",
            19 => "BeginTxn",
            20 => "EndTxn",
            21 => "AbortTxn",
            22 => "GetPos",
            23 => "GetDirect",
            24 => "StepNext",
            25 => "Stop",
            26 => "Version",
            27 => "Unlock",
            28 => "Reset",
            31 => "CreateIdx",
            32 => "DropIdx",
            33 => "StepFirst",
            34 => "StepLast",
            35 => "StepPrev",
            44 => "GetPct",
            45 => "FindPct",
            55 => "Op55",
            _ => "Unknown",
        }
    }

    /// Read a null-terminated path or string from a raw pointer (up to max bytes).
    pub fn cstr_from_raw(ptr: *const u8, max: usize) -> String {
        if ptr.is_null() || max == 0 {
            return "<null>".into();
        }
        let bytes = unsafe { slice::from_raw_parts(ptr, max) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(max);
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    /// Decode a key buffer as printable ASCII (non-printable → '.'), stops at null or max_len.
    pub fn key_as_ascii(ptr: *const u8, max_len: usize) -> String {
        if ptr.is_null() || max_len == 0 {
            return "<null>".into();
        }
        let bytes = unsafe { slice::from_raw_parts(ptr, max_len) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(max_len);
        if end == 0 {
            return "<empty>".into();
        }
        let s: String = bytes[..end]
            .iter()
            .map(|&b| {
                if b >= 0x20 && b < 0x7f {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        format!("\"{}\"", s)
    }

    /// Show up to max_len bytes as printable ASCII (non-printable → '.').
    pub fn data_as_text(ptr: *const u8, max_len: usize) -> String {
        if ptr.is_null() || max_len == 0 {
            return "<null>".into();
        }
        let bytes = unsafe { slice::from_raw_parts(ptr, max_len) };
        let s: String = bytes
            .iter()
            .map(|&b| {
                if b >= 0x20 && b < 0x7f {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        format!("\"{}\"", s)
    }

    /// Hex dump of raw bytes, space-separated.
    pub fn hex_bytes(ptr: *const u8, len: usize) -> String {
        if ptr.is_null() || len == 0 {
            return String::new();
        }
        unsafe { slice::from_raw_parts(ptr, len) }
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub use debug_trace::{
    cstr_from_raw, data_as_text, get_seq, hex_bytes, key_as_ascii, op_name, set_seq, trace,
    trace_enabled, trace_level, CALL_SEQ, LEVEL_DEBUG, LEVEL_ERROR, LEVEL_INFO, LEVEL_OFF,
};
