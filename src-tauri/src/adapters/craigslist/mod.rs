pub mod parser;

use super::{MarketAdapter, SearchContext};
use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;
use rand::RngExt;
use std::time::Duration;

/// Detail-page fetches per poll cycle. Unknown listings beyond this cap are
/// simply not returned this cycle and get picked up by the next one — keeps
/// every cycle bounded to ~cap × max_delay seconds of polite traffic.
const DETAIL_FETCH_CAP: usize = 8;
const MIN_DELAY_MS: u64 = 5_000;
const MAX_DELAY_MS: u64 = 15_000;

/// Honest user agent: identifies the tool, doesn't pretend to be a browser.
const USER_AGENT: &str = "Zinger/0.1 (personal marketplace search tool; conservative rate limits)";

pub struct CraigslistAdapter {
    client: reqwest::Client,
    /// Optional explicit CL site slug (e.g. "sfbay") from settings; otherwise
    /// derived from the search's location text. Read once at startup.
    site_override: Option<String>,
}

impl CraigslistAdapter {
    pub fn new(site_override: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .gzip(true)
            .build()
            .expect("reqwest client");
        CraigslistAdapter {
            client,
            site_override: site_override.filter(|s| !s.trim().is_empty()),
        }
    }

    /// "Seattle, WA" -> "seattle". Works for single-word metros; multi-word
    /// cities collapse ("New York" -> "newyork", which is correct), but
    /// renamed sites like SF's "sfbay" need the settings override.
    fn site_for(&self, spec: &SearchSpec) -> String {
        if let Some(site) = &self.site_override {
            return site.to_lowercase();
        }
        spec.location_text
            .split(',')
            .next()
            .unwrap_or("")
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    async fn fetch(&self, url: &str) -> Result<String, AdapterError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;
        let status = resp.status();
        if status == 403 || status == 429 {
            return Err(AdapterError::Blocked(format!("HTTP {status} from {url}")));
        }
        if !status.is_success() {
            return Err(AdapterError::Network(format!("HTTP {status} from {url}")));
        }
        resp.text().await.map_err(|e| AdapterError::Network(e.to_string()))
    }

    async fn polite_delay(&self) {
        let ms = rand::rng().random_range(MIN_DELAY_MS..=MAX_DELAY_MS);
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

#[async_trait]
impl MarketAdapter for CraigslistAdapter {
    fn id(&self) -> &'static str {
        "craigslist"
    }

    fn display_name(&self) -> &'static str {
        "Craigslist"
    }

    async fn search(
        &self,
        spec: &SearchSpec,
        ctx: &SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError> {
        let site = self.site_for(spec);
        if site.is_empty() {
            return Err(AdapterError::Disabled(
                "no Craigslist site: set a search location or the craigslist_site setting".into(),
            ));
        }

        let search_url = format!(
            "https://{site}.craigslist.org/search/sss?query={}&sort=date",
            urlencode(&spec.keywords)
        );
        let html = self.fetch(&search_url).await?;
        let results = parser::parse_search_page(&html)?;

        // Only spend detail fetches on listings we don't already have.
        let new_results: Vec<_> = results
            .into_iter()
            .filter(|r| !ctx.known_source_ids.contains(&r.source_id))
            .filter(|r| match (spec.price_ceiling, r.price) {
                (Some(ceiling), Some(price)) => price <= ceiling,
                _ => true,
            })
            .take(DETAIL_FETCH_CAP)
            .collect();

        let mut listings = Vec::with_capacity(new_results.len());
        for result in new_results {
            self.polite_delay().await;
            // A single failed detail page shouldn't void the whole cycle —
            // fall back to search-page data. But a block must propagate so
            // the poll layer can back off.
            let detail = match self.fetch(&result.url).await {
                Ok(page) => parser::parse_listing_page(&page).unwrap_or_default(),
                Err(AdapterError::Blocked(msg)) => return Err(AdapterError::Blocked(msg)),
                Err(_) => parser::ListingDetail::default(),
            };
            listings.push(raw_listing_from(&result, detail, spec));
        }
        Ok(listings)
    }

    fn health(&self) -> AdapterStatus {
        AdapterStatus::Ok
    }

    fn rate_limit_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy {
            min_delay_ms: MIN_DELAY_MS,
            max_delay_ms: MAX_DELAY_MS,
            max_requests_per_cycle: (DETAIL_FETCH_CAP + 1) as u32,
        }
    }
}

fn raw_listing_from(
    result: &parser::SearchResult,
    detail: parser::ListingDetail,
    spec: &SearchSpec,
) -> RawListing {
    RawListing {
        source: "craigslist".into(),
        source_id: result.source_id.clone(),
        source_url: result.url.clone(),
        title: result.title.clone(),
        description: detail.description,
        price: result.price.unwrap_or(0.0),
        currency: "USD".into(),
        location_text: result.location.clone().or_else(|| {
            (!spec.location_text.is_empty()).then(|| spec.location_text.clone())
        }),
        lat: detail.lat,
        lng: detail.lng,
        images: detail.images,
        posted_at: detail.posted_at,
        condition: detail.condition,
        phash: None,
    }
}

fn urlencode(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c.to_string(),
            ' ' => "+".to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_derivation() {
        let a = CraigslistAdapter::new(None);
        let spec = |loc: &str| SearchSpec {
            keywords: "x".into(),
            category: None,
            location_text: loc.into(),
            lat: None,
            lng: None,
            radius_km: 40.0,
            price_ceiling: None,
        };
        assert_eq!(a.site_for(&spec("Seattle, WA")), "seattle");
        assert_eq!(a.site_for(&spec("New York, NY")), "newyork");
        assert_eq!(a.site_for(&spec("")), "");
        let b = CraigslistAdapter::new(Some("sfbay".into()));
        assert_eq!(b.site_for(&spec("San Francisco, CA")), "sfbay");
    }

    #[test]
    fn urlencode_basics() {
        assert_eq!(urlencode("dewalt drill"), "dewalt+drill");
        assert_eq!(urlencode("3/8\" socket"), "3%2F8%22+socket");
    }

    /// Live smoke test — hits real Craigslist once. Run manually:
    /// cargo test --release -- --ignored craigslist_live
    #[tokio::test]
    #[ignore]
    async fn craigslist_live_smoke() {
        let adapter = CraigslistAdapter::new(Some("seattle".into()));
        let spec = SearchSpec {
            keywords: "dewalt drill".into(),
            category: None,
            location_text: "Seattle, WA".into(),
            lat: None,
            lng: None,
            radius_km: 40.0,
            price_ceiling: None,
        };
        let ctx = SearchContext::default();
        let listings = adapter.search(&spec, &ctx).await.unwrap();
        assert!(!listings.is_empty());
        println!("live smoke fetched {} listings", listings.len());
    }
}
