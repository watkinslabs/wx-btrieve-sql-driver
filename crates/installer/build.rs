use std::path::Path;

fn main() {
    // Build-time install paths.
    let install_dir =
        std::env::var("WXBTRV_INSTALL_DIR").unwrap_or_else(|_| r"C:\WatkinsX\bin".to_string());
    let config_dir =
        std::env::var("WXBTRV_CONFIG_DIR").unwrap_or_else(|_| r"C:\WatkinsX\config".to_string());
    println!("cargo:rustc-env=WXBTRV_INSTALL_DIR={install_dir}");
    println!("cargo:rustc-env=WXBTRV_CONFIG_DIR={config_dir}");
    println!("cargo:rerun-if-env-changed=WXBTRV_INSTALL_DIR");
    println!("cargo:rerun-if-env-changed=WXBTRV_CONFIG_DIR");

    // Embed payload binaries into the installer.
    // Set these env vars to the built binary paths before running cargo build.
    // The Makefile `installer` target does this automatically.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let payloads = [
        ("WXBTRV_DLL", "wxbtrv.dll"),
        ("WXBTRV_SYS", "wxbtrv.sys"),
        ("INT_TOOL_EXE", "db_config.exe"),
        ("BTR_IMPORT_EXE", "btr-import.exe"),
    ];

    for (env_var, filename) in &payloads {
        let dst = format!("{out_dir}/{filename}");
        let src = std::env::var(env_var).unwrap_or_default();
        if !src.is_empty() && Path::new(&src).exists() {
            std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copy {src}: {e}"));
            println!("cargo:rerun-if-changed={src}");
        } else {
            // No binary provided — write empty placeholder so include_bytes! compiles.
            // The installer will warn at runtime that this binary is missing.
            std::fs::write(&dst, b"").ok();
            println!("cargo:warning=Payload not set for {env_var} — installer.exe will not bundle {filename}");
        }
        println!("cargo:rerun-if-env-changed={env_var}");
    }

    println!("cargo:rerun-if-changed=build.rs");
}
