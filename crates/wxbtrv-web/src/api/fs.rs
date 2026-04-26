//! Native file-picker endpoints. The server pops a real OS dialog —
//! works because wxbtrv-web is a localhost-only tool: the browser, the
//! server, and the user are all on the same machine.
//!
//! All variants return `{ "path": Option<String> }`. `null` means the
//! user dismissed the picker.

use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct PickedPath {
    pub path: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct PickerOptions {
    /// Window title shown by the OS dialog.
    pub title: Option<String>,
    /// Filter pairs — name + extensions (no leading dot). E.g.
    /// `[{"name":"SQLite","ext":["db","sqlite"]}]`.
    pub filters: Option<Vec<FilterSpec>>,
    /// Suggested filename for save dialogs.
    pub suggest_name: Option<String>,
    /// Starting directory.
    pub start_dir: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct FilterSpec {
    pub name: String,
    pub ext: Vec<String>,
}

pub async fn pick_open(Json(opts): Json<PickerOptions>) -> Json<PickedPath> {
    Json(PickedPath {
        path: tokio::task::spawn_blocking(move || {
            let mut d = build_dialog(&opts);
            if let Some(s) = opts.start_dir.as_deref() {
                d = d.set_directory(s);
            }
            d.pick_file().map(|p| p.to_string_lossy().into_owned())
        })
        .await
        .ok()
        .flatten(),
    })
}

pub async fn pick_save(Json(opts): Json<PickerOptions>) -> Json<PickedPath> {
    Json(PickedPath {
        path: tokio::task::spawn_blocking(move || {
            let mut d = build_dialog(&opts);
            if let Some(s) = opts.suggest_name.as_deref() {
                d = d.set_file_name(s);
            }
            if let Some(s) = opts.start_dir.as_deref() {
                d = d.set_directory(s);
            }
            d.save_file().map(|p| p.to_string_lossy().into_owned())
        })
        .await
        .ok()
        .flatten(),
    })
}

pub async fn pick_dir(Json(opts): Json<PickerOptions>) -> Json<PickedPath> {
    Json(PickedPath {
        path: tokio::task::spawn_blocking(move || {
            let mut d = rfd::FileDialog::new();
            if let Some(t) = opts.title.as_deref() {
                d = d.set_title(t);
            }
            if let Some(s) = opts.start_dir.as_deref() {
                d = d.set_directory(s);
            }
            d.pick_folder().map(|p| p.to_string_lossy().into_owned())
        })
        .await
        .ok()
        .flatten(),
    })
}

fn build_dialog(opts: &PickerOptions) -> rfd::FileDialog {
    let mut d = rfd::FileDialog::new();
    if let Some(t) = opts.title.as_deref() {
        d = d.set_title(t);
    }
    if let Some(filters) = &opts.filters {
        for f in filters {
            let exts: Vec<&str> = f.ext.iter().map(|s| s.as_str()).collect();
            d = d.add_filter(&f.name, &exts);
        }
    }
    d
}
