pub mod parser;

use super::{MarketAdapter, SearchContext};
use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TOKEN_URL: &str = "https://api.ebay.com/identity/v1/oauth2/token";
const BROWSE_URL: &str = "https://api.ebay.com/buy/browse/v1/item_summary/search";
const OAUTH_SCOPE: &str = "https://api.ebay.com/oauth/api_scope";

pub const KEYRING_SERVICE: &str = "zinger";
pub const KEYRING_CLIENT_ID: &str = "ebay_client_id";
pub const KEYRING_CLIENT_SECRET: &str = "ebay_client_secret";

/// Read eBay credentials from the OS keychain. None = not configured, which
/// is a fully supported state (adapter no-ops).
pub fn stored_credentials() -> Option<(String, String)> {
    let id = keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_ID)
        .ok()?
        .get_password()
        .ok()?;
    let secret = keyring::Entry::new(KEYRING_SERVICE, KEYRING_CLIENT_SECRET)
        .ok()?
        .get_password()
        .ok()?;
    (!id.is_empty() && !secret.is_empty()).then_some((id, secret))
}

pub fn store_credentials(client_id: &str, client_secret: &str) -> Result<(), String> {
    let write = |key: &str, value: &str| -> Result<(), String> {
        keyring::Entry::new(KEYRING_SERVICE, key)
            .map_err(|e| e.to_string())?
            .set_password(value)
            .map_err(|e| e.to_string())
    };
    write(KEYRING_CLIENT_ID, client_id)?;
    write(KEYRING_CLIENT_SECRET, client_secret)
}

struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub struct EbayAdapter {
    client: reqwest::Client,
    token: Mutex<Option<CachedToken>>,
}

impl EbayAdapter {
    pub fn new() -> Self {
        EbayAdapter {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("reqwest client"),
            token: Mutex::new(None),
        }
    }

    fn cached_token(&self) -> Option<String> {
        let guard = self.token.lock().ok()?;
        guard
            .as_ref()
            .filter(|t| t.expires_at > Instant::now())
            .map(|t| t.token.clone())
    }

    /// Client-credentials grant. Also serves as "Test connection".
    pub async fn fetch_token(&self) -> Result<String, AdapterError> {
        if let Some(token) = self.cached_token() {
            return Ok(token);
        }
        let Some((client_id, client_secret)) = stored_credentials() else {
            return Err(AdapterError::Disabled("eBay API keys not configured".into()));
        };

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: u64,
        }

        let resp = self
            .client
            .post(TOKEN_URL)
            .basic_auth(&client_id, Some(&client_secret))
            .form(&[
                ("grant_type", "client_credentials"),
                ("scope", OAUTH_SCOPE),
            ])
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            // Body may contain an error description but never echo secrets.
            return Err(AdapterError::Network(format!(
                "eBay token endpoint returned HTTP {status} — check your App ID / Cert ID"
            )));
        }
        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AdapterError::Parse(format!("token response: {e}")))?;

        if let Ok(mut guard) = self.token.lock() {
            *guard = Some(CachedToken {
                token: token.access_token.clone(),
                // Refresh a minute early to avoid using a token at the edge.
                expires_at: Instant::now()
                    + Duration::from_secs(token.expires_in.saturating_sub(60)),
            });
        }
        Ok(token.access_token)
    }
}

impl Default for EbayAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MarketAdapter for EbayAdapter {
    fn id(&self) -> &'static str {
        "ebay"
    }

    fn display_name(&self) -> &'static str {
        "eBay"
    }

    async fn search(
        &self,
        spec: &SearchSpec,
        _ctx: &SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError> {
        // Unconfigured = silent no-op, per contract. The poll layer also
        // skips us based on health(), so this is a second line of defense.
        if stored_credentials().is_none() {
            return Ok(Vec::new());
        }
        let token = self.fetch_token().await?;

        let mut query: Vec<(String, String)> =
            vec![("q".into(), spec.keywords.clone()), ("limit".into(), "50".into())];
        if let Some(ceiling) = spec.price_ceiling {
            query.push((
                "filter".into(),
                format!("price:[..{ceiling}],priceCurrency:USD"),
            ));
        }

        let resp = self
            .client
            .get(BROWSE_URL)
            .bearer_auth(&token)
            .header("X-EBAY-C-MARKETPLACE-ID", "EBAY_US")
            .query(&query)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = resp.status();
        if status == 429 {
            return Err(AdapterError::Blocked("eBay rate limit (HTTP 429)".into()));
        }
        if !status.is_success() {
            return Err(AdapterError::Network(format!("eBay Browse HTTP {status}")));
        }
        let body = resp.text().await.map_err(|e| AdapterError::Network(e.to_string()))?;
        parser::parse_item_summaries(&body)
    }

    fn health(&self) -> AdapterStatus {
        if stored_credentials().is_none() {
            AdapterStatus::NotConfigured {
                reason: "add eBay API keys in Settings".into(),
            }
        } else {
            AdapterStatus::Ok
        }
    }

    fn rate_limit_policy(&self) -> RateLimitPolicy {
        // Official API with per-key quotas; one search call per cycle.
        RateLimitPolicy {
            min_delay_ms: 0,
            max_delay_ms: 0,
            max_requests_per_cycle: 2,
        }
    }
}
