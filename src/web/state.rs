use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::DashboardStats;

/// Shared state for the web server (contains only thread-safe data)
#[derive(Clone)]
pub struct WebSharedState {
    pub stats: Arc<RwLock<DashboardStats>>,
}

impl WebSharedState {
    pub fn new(stats: DashboardStats) -> Self {
        Self {
            stats: Arc::new(RwLock::new(stats)),
        }
    }

    #[allow(dead_code)]
    pub async fn update_stats(&self, stats: DashboardStats) {
        let mut state = self.stats.write().await;
        *state = stats;
    }
}
