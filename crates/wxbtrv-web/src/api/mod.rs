//! HTTP routes. Top-level: `/api/*` for JSON, `/*` falls through to the
//! embedded React build (with SPA fallback).

use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

use crate::{embed, state::AppState};

mod connection;
mod tables;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/config", get(connection::get_config))
        .route("/config/backend", axum::routing::post(connection::set_backend))
        .route("/test-connection", axum::routing::post(connection::test_connection))
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
