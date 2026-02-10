use axum::{
    extract::State,
    response::Html,
    Json,
};

use crate::models::DashboardStats;
use crate::web::state::WebSharedState;

/// Handler for the dashboard HTML page
pub async fn get_dashboard_html() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}

/// Handler for /api/stats - returns dashboard statistics
pub async fn get_stats(State(state): State<WebSharedState>) -> Json<DashboardStats> {
    let stats = state.stats.read().await;
    Json(stats.clone())
}
