//! build.rs — compile the React UI into ui/dist/ before cargo embeds it
//! into the binary via rust-embed.
//!
//! Strategy: graceful — if `npm` is missing, the UI's `package.json` is
//! absent, or `npm install/build` fails, log a warning and continue.
//! The resulting binary still serves the API; static-asset routes will
//! 404 until a future build re-runs with npm available.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ui_dir = manifest_dir.join("ui");
    let pkg_json = ui_dir.join("package.json");
    let dist_dir = ui_dir.join("dist");

    // Re-run when these change.
    println!("cargo:rerun-if-changed={}", ui_dir.join("package.json").display());
    println!("cargo:rerun-if-changed={}", ui_dir.join("vite.config.ts").display());
    println!("cargo:rerun-if-changed={}", ui_dir.join("src").display());
    println!("cargo:rerun-if-changed={}", ui_dir.join("index.html").display());
    println!("cargo:rerun-if-env-changed=WXBTRV_WEB_SKIP_UI");

    if std::env::var("WXBTRV_WEB_SKIP_UI").ok().as_deref() == Some("1") {
        println!("cargo:warning=WXBTRV_WEB_SKIP_UI=1 — skipping React UI build");
        ensure_dist_placeholder(&dist_dir);
        return;
    }

    if !pkg_json.exists() {
        println!(
            "cargo:warning=ui/package.json not found — skipping React UI build (server will 404 on /)"
        );
        ensure_dist_placeholder(&dist_dir);
        return;
    }

    if which("npm").is_none() {
        println!(
            "cargo:warning=npm not found on PATH — skipping React UI build (server will 404 on /)"
        );
        ensure_dist_placeholder(&dist_dir);
        return;
    }

    println!("cargo:warning=Running npm install + npm run build in {}", ui_dir.display());

    if !run_in("npm", &["install", "--no-audit", "--no-fund", "--no-progress"], &ui_dir) {
        println!("cargo:warning=npm install failed — skipping React UI build");
        ensure_dist_placeholder(&dist_dir);
        return;
    }
    if !run_in("npm", &["run", "build"], &ui_dir) {
        println!("cargo:warning=npm run build failed — skipping React UI build");
        ensure_dist_placeholder(&dist_dir);
        return;
    }

    if !dist_dir.exists() {
        println!(
            "cargo:warning=ui/dist not produced after npm run build — embedding placeholder"
        );
        ensure_dist_placeholder(&dist_dir);
    }
}

fn run_in(cmd: &str, args: &[&str], cwd: &PathBuf) -> bool {
    Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// rust-embed errors at compile time if the embedded folder doesn't
/// exist. Make sure ui/dist/ has at least an index.html so the binary
/// always builds.
fn ensure_dist_placeholder(dist_dir: &PathBuf) {
    let _ = std::fs::create_dir_all(dist_dir);
    let placeholder = dist_dir.join("index.html");
    if !placeholder.exists() {
        let _ = std::fs::write(
            placeholder,
            r#"<!doctype html>
<html><head><meta charset="utf-8"><title>wxbtrv-web</title></head>
<body style="font-family: system-ui; max-width: 40rem; margin: 4rem auto; padding: 0 1rem">
  <h1>wxbtrv-web</h1>
  <p>The React UI was not built into this binary. The API still works at
    <code>/api/*</code>. To rebuild with the UI included:</p>
  <pre>cd crates/wxbtrv-web/ui &amp;&amp; npm install &amp;&amp; npm run build
cargo build -p wxbtrv-web --release</pre>
</body></html>"#,
        );
    }
}
