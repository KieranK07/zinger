use serde::{Deserialize, Serialize};

/// A saved search as stored in the DB and edited in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    pub keywords: String,
    pub category: Option<String>,
    pub location_text: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: f64,
    pub price_ceiling: Option<f64>,
    pub enabled: bool,
    pub notify_score_threshold: Option<f64>,
    /// None = use the global poll_interval_minutes setting.
    pub poll_interval_minutes: Option<i64>,
}

/// What an adapter needs to execute a search. Decoupled from SavedSearch so
/// adapters never see DB ids or notification config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSpec {
    pub keywords: String,
    pub category: Option<String>,
    pub location_text: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: f64,
    pub price_ceiling: Option<f64>,
}

impl From<&SavedSearch> for SearchSpec {
    fn from(s: &SavedSearch) -> Self {
        SearchSpec {
            keywords: s.keywords.clone(),
            category: s.category.clone(),
            location_text: s.location_text.clone(),
            lat: s.lat,
            lng: s.lng,
            radius_km: s.radius_km,
            price_ceiling: s.price_ceiling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    New,
    LikeNew,
    Good,
    Fair,
    Parts,
}

impl Condition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Condition::New => "new",
            Condition::LikeNew => "like_new",
            Condition::Good => "good",
            Condition::Fair => "fair",
            Condition::Parts => "parts",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "new" => Some(Condition::New),
            "like_new" => Some(Condition::LikeNew),
            "good" => Some(Condition::Good),
            "fair" => Some(Condition::Fair),
            "parts" => Some(Condition::Parts),
            _ => None,
        }
    }
}

/// What an adapter returns: source-shaped, unnormalized. The pipeline turns
/// these into stored listings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawListing {
    pub source: String,
    pub source_id: String,
    pub source_url: String,
    pub title: String,
    pub description: Option<String>,
    pub price: f64,
    pub currency: String,
    pub location_text: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub images: Vec<String>,
    pub posted_at: Option<String>,
    pub condition: Option<Condition>,
    /// Perceptual hash of the first image, filled in by the poll layer
    /// (adapters return None — they don't download images).
    #[serde(default)]
    pub phash: Option<String>,
}

/// A normalized, deduplicated listing as stored and shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: i64,
    pub search_id: i64,
    pub source: String,
    pub source_id: String,
    pub source_url: String,
    pub title: String,
    pub description: Option<String>,
    pub price: f64,
    pub currency: String,
    pub location_text: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub images: Vec<String>,
    pub posted_at: Option<String>,
    pub fetched_at: String,
    pub condition: Option<Condition>,
    pub category_guess: Option<String>,
    pub dedup_hash: String,
    // Joined user_actions state; defaults when no row exists.
    pub seen: bool,
    pub hidden: bool,
    pub saved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AdapterStatus {
    Ok,
    /// Adapter needs user-supplied configuration (e.g. eBay API keys) and is
    /// silently inert until it gets it. Not an error state.
    NotConfigured { reason: String },
    Degraded { reason: String },
    Disabled { reason: String },
}

/// Per-adapter politeness contract. The scheduler (M2) enforces it; adapters
/// declare it so limits live next to the source they protect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_requests_per_cycle: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("blocked or rate limited: {0}")]
    Blocked(String),
    #[error("adapter disabled: {0}")]
    Disabled(String),
}
