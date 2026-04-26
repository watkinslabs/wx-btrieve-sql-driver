//! HTTP routes. Top-level: `/api/*` for JSON, `/*` falls through to the
//! embedded React build (with SPA fallback).

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{embed, state::AppState};

mod connections;
mod fs;
mod project;
mod tables;

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
        // tables
        .route("/tables", get(tables::list_tables))
        .route("/tables/:name", get(tables::show_table))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback(embed::serve)
        .layer(TraceLayer::new_for_http())
}

async fn health() -> &'static str {
    "ok"
}
