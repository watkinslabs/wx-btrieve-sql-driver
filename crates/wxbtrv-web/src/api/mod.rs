//! HTTP routes. Top-level: `/api/*` for JSON, `/*` falls through to the
//! embedded React build (with SPA fallback).

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{embed, state::AppState};

mod bimport;
mod browser;
mod connections;
mod diff;
mod fs;
mod project;
mod schema;
mod tables;
mod workflow;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        // project (open wxbtrv.db file)
        .route("/project", get(project::current))
        .route("/project/open", post(project::open_project))
        .route("/project/close", post(project::close_project))
        .route("/project/init", post(project::init_project))
        .route("/project/recent", get(project::list_recent))
        .route("/project/forget", post(project::forget))
        // native file pickers
        .route("/fs/pick-open", post(fs::pick_open))
        .route("/fs/pick-save", post(fs::pick_save))
        .route("/fs/pick-dir", post(fs::pick_dir))
        // connections (global + per-database overrides)
        .route("/connections", get(connections::list))
        .route("/connections/test", post(connections::test))
        .route(
            "/connections/:name",
            get(connections::get_one)
                .post(connections::upsert)
                .delete(connections::delete_one),
        )
        .route("/connections/:name/resolved", get(connections::get_resolved))
        // tables (read + delete)
        .route("/tables", get(tables::list_tables))
        .route(
            "/tables/:name",
            get(tables::show_table).delete(schema::rm_table),
        )
        .route("/tables/:name/prop", post(schema::set_table_prop))
        .route("/tables/:name/fields", post(schema::add_field))
        .route("/tables/:name/fields/:num", axum::routing::delete(schema::rm_field))
        .route("/tables/:name/indexes", post(schema::add_index))
        .route("/tables/:name/indexes/:num", axum::routing::delete(schema::rm_index))
        .route("/tables/:name/rows", get(browser::rows))
        .route("/tables/:name/diff", get(diff::diff))
        // workflow: imports, exports, migration tracking
        .route("/import/int", post(workflow::import_int))
        .route("/import/int/stream", post(workflow::import_int_stream))
        .route("/import/mds", post(workflow::import_mds))
        .route("/import/analyze-b", post(workflow::analyze_b))
        .route("/export/int", post(workflow::export_int))
        .route("/export/mds", post(workflow::export_mds))
        .route("/export/ddl", post(workflow::export_ddl))
        .route("/migration/status", get(workflow::migration_status))
        .route("/migration/mark", post(workflow::mark_migrated))
        .route("/migration/clear", post(workflow::clear_migrated))
        // .B → backend bulk import
        .route("/bimport/info", post(bimport::info))
        .route("/bimport/files", post(bimport::import_files))
        .route("/bimport/dir", post(bimport::import_dir))
        .route("/bimport/files/stream", post(bimport::import_files_stream))
        .route("/bimport/dir/stream", post(bimport::import_dir_stream))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback(embed::serve)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> &'static str {
    "ok"
}
