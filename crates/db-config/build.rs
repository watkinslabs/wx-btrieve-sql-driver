fn main() {
    let config_dir =
        std::env::var("WXBTRV_CONFIG_DIR").unwrap_or_else(|_| r"C:\WatkinsX\config".to_string());
    println!("cargo:rustc-env=WXBTRV_CONFIG_DIR={config_dir}");
    println!("cargo:rerun-if-env-changed=WXBTRV_CONFIG_DIR");
    println!("cargo:rerun-if-changed=build.rs");
}
