use std::path::PathBuf;
use std::process::Command;

fn main() {
    let src = PathBuf::from("src/wxbtrv.asm");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let sys = out_dir.join("wxbtrv.sys");

    // Full path to wxbtrv.dll — embedded in the .sys binary so NTVDM's
    // RegisterModule can find it without relying on PATH.
    // NTVDM uses LoadLibraryExA with LOAD_LIBRARY_SEARCH_SYSTEM32 (0x800),
    // so the DLL must live in System32 and be referenced by bare filename.
    let dll_path = std::env::var("WXBTRV_DLL_PATH").unwrap_or_else(|_| "wxbtrv.dll".to_string());

    println!("cargo:rerun-if-changed=src/wxbtrv.asm");
    println!("cargo:rerun-if-env-changed=WXBTRV_INSTALL_DIR");

    let status = Command::new("nasm")
        .args([
            "-f",
            "bin",
            &format!("-DWXBTRV_DLL_PATH=\"{}\"", dll_path),
            "-o",
            sys.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .status()
        .expect("nasm not found — install nasm");

    assert!(status.success(), "nasm assembly failed");

    println!("cargo:rustc-env=WXBTRV_SYS_PATH={}", sys.display());
}
