pub mod craigslist;
pub mod ebay;
pub mod facebook;
pub mod mock;

use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;
use std::collections::HashSet;

/// Cross-cycle knowledge the poll layer hands to adapters so they can avoid
/// redundant work — e.g. Craigslist skips detail-page fetches for listings
/// we already have. Adapters never touch the DB themselves.
#[derive(Debug, Default)]
pub struct SearchContext {
    /// source_ids already stored for this adapter's source.
    pub known_source_ids: HashSet<String>,
}

/// The contract every marketplace source implements. Sources are expected to
/// be fragile (HTML changes, blocks); errors from one adapter must never
/// affect another, so the poll layer isolates each call.
#[async_trait]
pub trait MarketAdapter: Send + Sync {
    /// Stable identifier, e.g. "ebay", "craigslist", "mock".
    fn id(&self) -> &'static str;

    /// Human-readable name for the UI.
    fn display_name(&self) -> &'static str;

    async fn search(
        &self,
        spec: &SearchSpec,
        ctx: &SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError>;

    fn health(&self) -> AdapterStatus;

    fn rate_limit_policy(&self) -> RateLimitPolicy;
}

pub struct AdapterRegistry {
    adapters: Vec<Box<dyn MarketAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        AdapterRegistry { adapters: Vec::new() }
    }

    pub fn register(&mut self, adapter: Box<dyn MarketAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn adapters(&self) -> &[Box<dyn MarketAdapter>] {
        &self.adapters
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
