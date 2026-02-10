use axum::{
    routing::get,
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::web::handlers::{get_dashboard_html, get_stats};
use crate::web::state::WebSharedState;

/// Start the web server for the dashboard
pub async fn start_web_server(shared_state: WebSharedState) {
    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build our application with routes
    let app = Router::new()
        .route("/", get(get_dashboard_html))
        .route("/api/stats", get(get_stats))
        .layer(cors)
        .with_state(shared_state);

    // Run the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Failed to bind to address");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
