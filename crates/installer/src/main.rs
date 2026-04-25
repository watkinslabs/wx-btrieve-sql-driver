/// wxbtrv stack installer — single-file bundle.
///
/// Embeds wxbtrv.dll, wxbtrv.sys, db_config.exe, btr-import.exe at build time.
/// Copy installer.exe to the target machine and run it — no other files needed.
///
/// What it does:
///   1. Creates the install and config directories
///   2. Extracts bundled binaries to the install directory
///   3. Adds the install directory to the system PATH (registry)
///   4. Patches config.nt (removes BTRDRVR.SYS, inserts our DEVICE= line)
///   5. Kills ntvdm.exe so the new driver is picked up on next launch
///
/// Database setup is handled separately via db_config.
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const INSTALL_DIR: &str = env!("WXBTRV_INSTALL_DIR");
const CONFIG_DIR: &str = env!("WXBTRV_CONFIG_DIR");
const CONFIG_NT: &str = r"C:\Windows\System32\config.nt";

const PAYLOAD_DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wxbtrv.dll"));
const PAYLOAD_SYS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/wxbtrv.sys"));
const PAYLOAD_INT_TOOL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/db_config.exe"));
const PAYLOAD_BTR_IMPORT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/btr-import.exe"));

const PAYLOADS: &[(&str, &[u8])] = &[
    ("wxbtrv.dll", PAYLOAD_DLL),
    ("wxbtrv.sys", PAYLOAD_SYS),
    ("db_config.exe", PAYLOAD_INT_TOOL),
    ("btr-import.exe", PAYLOAD_BTR_IMPORT),
];

#[derive(Parser)]
#[command(name = "installer", about = "wxbtrv stack installer")]
struct Args {
    /// Install directory for binaries
    #[arg(long, default_value = INSTALL_DIR)]
    install_dir: PathBuf,
    /// Config directory (created but not populated — use int-tool for that)
    #[arg(long, default_value = CONFIG_DIR)]
    config_dir: PathBuf,
    /// Skip updating system PATH
    #[arg(long)]
    no_path: bool,
    /// Skip killing ntvdm.exe
    #[arg(long)]
    no_ntvdm: bool,
}

fn main() {
    let args = Args::parse();

    println!("=== wxbtrv installer ===");
    println!("  install dir : {}", args.install_dir.display());
    println!("  config dir  : {}", args.config_dir.display());
    println!();

    step_create_dirs(&args.install_dir, &args.config_dir);
    step_extract_binaries(&args.install_dir);
    if !args.no_path {
        step_update_path(&args.install_dir);
    }
    step_patch_config_nt(&args.install_dir);
    if !args.no_ntvdm {
        step_kill_ntvdm();
    }

    println!();
    println!("=== Installation complete ===");
    println!("  Run db_config.exe to initialise the database and import schemas.");
}

// ── Steps ─────────────────────────────────────────────────────────────────────

fn step_create_dirs(install_dir: &Path, config_dir: &Path) {
    print!("[1] Creating directories... ");
    fs::create_dir_all(install_dir)
        .unwrap_or_else(|e| die(&format!("create {}: {e}", install_dir.display())));
    fs::create_dir_all(config_dir)
        .unwrap_or_else(|e| die(&format!("create {}: {e}", config_dir.display())));
    println!("OK");
}

fn step_extract_binaries(install_dir: &Path) {
    println!("[2] Extracting binaries to {}...", install_dir.display());
    for (name, bytes) in PAYLOADS {
        if bytes.is_empty() {
            eprintln!("  WARNING: {name} was not bundled at build time — skipping");
            continue;
        }
        let dst = install_dir.join(name);
        fs::write(&dst, bytes).unwrap_or_else(|e| die(&format!("write {name}: {e}")));
        println!("  {name}  ({} bytes)", bytes.len());
    }
}

fn step_update_path(install_dir: &Path) {
    print!("[3] Updating system PATH... ");
    let dir_str = install_dir.to_string_lossy();
    let current = reg_get_system_path();
    if current
        .to_ascii_lowercase()
        .contains(&dir_str.to_ascii_lowercase())
    {
        println!("already present");
        return;
    }
    let new_path = format!("{};{}", dir_str, current);
    let status = Command::new("reg")
        .args([
            "add",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            "/v",
            "Path",
            "/t",
            "REG_EXPAND_SZ",
            "/d",
            &new_path,
            "/f",
        ])
        .status();
    match status {
        Ok(s) if s.success() => println!("OK"),
        Ok(s) => eprintln!("WARNING: reg exited {s}"),
        Err(e) => eprintln!("WARNING: could not run reg.exe: {e}"),
    }
}

fn step_patch_config_nt(install_dir: &Path) {
    print!("[4] Patching {}... ", CONFIG_NT);
    let sys_path = install_dir.join("wxbtrv.sys");
    let device_line = format!("DEVICE={}", sys_path.display());

    let content = match fs::read_to_string(CONFIG_NT) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("WARNING: could not read config.nt: {e}");
            return;
        }
    };

    // Comment out any existing Btrieve DEVICE= lines.
    // Remove any previous wxbtrv.sys line (idempotent re-run).
    // Then append our line.
    let mut new_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let u = line.to_ascii_uppercase();
        let is_btrv_device = u.contains("DEVICE=") && u.contains("BTRV");
        let is_our_line = u.contains("WXBTRV.SYS");
        if is_our_line {
            // Drop previous wxbtrv.sys entry — we'll re-add fresh below
        } else if is_btrv_device {
            new_lines.push(format!("REM [replaced by WatkinsX] {}", line));
        } else {
            new_lines.push(line.to_string());
        }
    }
    new_lines.push(device_line.clone());

    let mut new_content = new_lines.join("\r\n");
    new_content.push_str("\r\n");

    match fs::write(CONFIG_NT, &new_content) {
        Ok(_) => {
            println!("OK");
            for line in new_lines.iter().filter(|l| {
                let u = l.to_ascii_uppercase();
                u.contains("DEVICE=") || u.contains("REM [REPLACED")
            }) {
                println!("    {}", line);
            }
        }
        Err(e) => eprintln!("WARNING: could not write config.nt: {e}"),
    }
}

fn step_kill_ntvdm() {
    print!("[5] Stopping ntvdm.exe... ");
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "ntvdm.exe"])
        .status();
    println!("OK");
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn reg_get_system_path() -> String {
    let output = match Command::new("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            "/v",
            "Path",
        ])
        .output()
    {
        Ok(o) => o,
        Err(_) => return String::new(),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("PATH") {
            for marker in ["REG_EXPAND_SZ", "REG_SZ"] {
                if let Some(pos) = upper.find(marker) {
                    return trimmed[pos + marker.len()..].trim().to_string();
                }
            }
        }
    }
    String::new()
}

fn die(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    std::process::exit(1);
}
