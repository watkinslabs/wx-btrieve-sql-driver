fn main() {
    // Build-time configurable install path for `wxbtrv.db`. The production
    // wxbtrv shim overrides this via its own `build.rs`; this crate sets a
    // sensible default so core can be built standalone on any host.
    let config_dir =
        std::env::var("WXBTRV_CONFIG_DIR").unwrap_or_else(|_| r"C:\WatkinsX\config".to_string());
    println!("cargo:rustc-env=WXBTRV_CONFIG_DIR={config_dir}");
    println!("cargo:rerun-if-env-changed=WXBTRV_CONFIG_DIR");

    // Embed build timestamp for deployment verification via trace log.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    println!("cargo:rustc-env=BUILD_DATE={}", format_date(secs));
    println!("cargo:rustc-env=BUILD_TIME={}", format_time(secs));
    println!("cargo:rerun-if-changed=build.rs");
}

fn format_date(secs: u64) -> String {
    let days = secs / 86400;
    let mut y = 1970u64;
    let mut d = days;
    loop {
        let ydays = if is_leap(y) { 366 } else { 365 };
        if d < ydays {
            break;
        }
        d -= ydays;
        y += 1;
    }
    let months = [
        31u64,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1u64;
    for mdays in months {
        if d < mdays {
            break;
        }
        d -= mdays;
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m, d + 1)
}

fn format_time(secs: u64) -> String {
    let h = (secs % 86400) / 3600;
    let mi = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}UTC", h, mi, s)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}
