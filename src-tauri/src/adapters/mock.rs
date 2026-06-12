use super::MarketAdapter;
use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;

/// Fixture-backed adapter. Stands in for real sources during M1 and serves as
/// the fixture adapter for integration tests thereafter. Embedded at compile
/// time so it works identically in dev, tests, and packaged builds.
const FIXTURE_JSON: &str = include_str!("../../fixtures/mock_listings.json");

pub struct MockAdapter;

impl MockAdapter {
    pub fn new() -> Self {
        MockAdapter
    }

    fn load_fixtures() -> Result<Vec<RawListing>, AdapterError> {
        serde_json::from_str(FIXTURE_JSON)
            .map_err(|e| AdapterError::Parse(format!("fixture JSON: {e}")))
    }
}

impl Default for MockAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MarketAdapter for MockAdapter {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn display_name(&self) -> &'static str {
        "Mock (fixtures)"
    }

    async fn search(&self, spec: &SearchSpec) -> Result<Vec<RawListing>, AdapterError> {
        let all = Self::load_fixtures()?;
        let terms: Vec<String> = spec
            .keywords
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();

        // Match any keyword against title/description, like a real search
        // would; empty keywords returns everything.
        let results = all
            .into_iter()
            .filter(|l| {
                if let Some(ceiling) = spec.price_ceiling {
                    if l.price > ceiling {
                        return false;
                    }
                }
                if terms.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {}",
                    l.title.to_lowercase(),
                    l.description.as_deref().unwrap_or("").to_lowercase()
                );
                terms.iter().any(|t| haystack.contains(t))
            })
            .collect();
        Ok(results)
    }

    fn health(&self) -> AdapterStatus {
        AdapterStatus::Ok
    }

    fn rate_limit_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy {
            min_delay_ms: 0,
            max_delay_ms: 0,
            max_requests_per_cycle: 1000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(keywords: &str, ceiling: Option<f64>) -> SearchSpec {
        SearchSpec {
            keywords: keywords.into(),
            category: None,
            location_text: "Seattle, WA".into(),
            lat: Some(47.6062),
            lng: Some(-122.3321),
            radius_km: 40.0,
            price_ceiling: ceiling,
        }
    }

    #[tokio::test]
    async fn fixtures_parse_and_return_all_for_empty_query() {
        let adapter = MockAdapter::new();
        let results = adapter.search(&spec("", None)).await.unwrap();
        assert_eq!(results.len(), 12);
    }

    #[tokio::test]
    async fn keyword_filter_matches_title_and_description() {
        let adapter = MockAdapter::new();
        let results = adapter.search(&spec("dewalt", None)).await.unwrap();
        assert_eq!(results.len(), 2); // the cross-posted drill pair
    }

    #[tokio::test]
    async fn price_ceiling_filters() {
        let adapter = MockAdapter::new();
        let results = adapter.search(&spec("", Some(100.0))).await.unwrap();
        assert!(results.iter().all(|l| l.price <= 100.0));
        assert!(!results.is_empty());
    }
}
