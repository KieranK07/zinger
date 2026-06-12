pub mod mock;

use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;

/// The contract every marketplace source implements. Sources are expected to
/// be fragile (HTML changes, blocks); errors from one adapter must never
/// affect another, so the registry isolates each call.
#[async_trait]
pub trait MarketAdapter: Send + Sync {
    /// Stable identifier, e.g. "ebay", "craigslist", "mock".
    fn id(&self) -> &'static str;

    /// Human-readable name for the UI.
    fn display_name(&self) -> &'static str;

    async fn search(&self, spec: &SearchSpec) -> Result<Vec<RawListing>, AdapterError>;

    fn health(&self) -> AdapterStatus;

    fn rate_limit_policy(&self) -> RateLimitPolicy;
}

pub struct AdapterRegistry {
    adapters: Vec<Box<dyn MarketAdapter>>,
}

/// Result of running one adapter within a poll cycle. Failures are data, not
/// control flow: the cycle reports them and moves on.
#[derive(Debug)]
pub struct AdapterRun {
    pub adapter_id: &'static str,
    pub result: Result<Vec<RawListing>, AdapterError>,
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

    /// Run every adapter against the spec. Each adapter's failure is captured
    /// in its own AdapterRun; one source going down never blocks the rest.
    pub async fn search_all(&self, spec: &SearchSpec) -> Vec<AdapterRun> {
        let mut runs = Vec::with_capacity(self.adapters.len());
        for adapter in &self.adapters {
            let result = adapter.search(spec).await;
            runs.push(AdapterRun { adapter_id: adapter.id(), result });
        }
        runs
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
