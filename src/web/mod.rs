pub mod routes;
pub mod sse;
pub mod state;

use axum::Router;
use axum::routing::get;
use tower_http::services::ServeDir;

use state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(routes::dashboard))
        .route("/sse", get(sse::sse_handler))
        .nest_service("/static", ServeDir::new("templates/static"))
        .with_state(state)
}
