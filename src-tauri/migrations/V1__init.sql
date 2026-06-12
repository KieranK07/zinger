CREATE TABLE searches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    keywords TEXT NOT NULL,
    category TEXT,
    location_text TEXT NOT NULL DEFAULT '',
    lat REAL,
    lng REAL,
    radius_km REAL NOT NULL DEFAULT 40.0,
    price_ceiling REAL,
    enabled INTEGER NOT NULL DEFAULT 1,
    notify_score_threshold REAL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE listings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    search_id INTEGER NOT NULL REFERENCES searches(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    source_url TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    price REAL NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    location_text TEXT,
    lat REAL,
    lng REAL,
    images TEXT NOT NULL DEFAULT '[]', -- JSON array of URLs
    posted_at TEXT,
    fetched_at TEXT NOT NULL,
    condition TEXT, -- new | like_new | good | fair | parts | NULL=unknown
    category_guess TEXT,
    dedup_hash TEXT NOT NULL,
    UNIQUE (source, source_id)
);

CREATE INDEX idx_listings_dedup_hash ON listings (dedup_hash);
CREATE INDEX idx_listings_fetched_at ON listings (fetched_at);
CREATE INDEX idx_listings_search ON listings (search_id);

CREATE TABLE comps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    listing_id INTEGER NOT NULL REFERENCES listings(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    title TEXT NOT NULL,
    sold_price REAL NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    sold_at TEXT,
    url TEXT,
    fetched_at TEXT NOT NULL
);

CREATE INDEX idx_comps_listing ON comps (listing_id);

CREATE TABLE valuations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    listing_id INTEGER NOT NULL UNIQUE REFERENCES listings(id) ON DELETE CASCADE,
    est_resale REAL,          -- NULL = unpriceable
    iqr_low REAL,
    iqr_high REAL,
    comp_count INTEGER NOT NULL DEFAULT 0,
    est_fees REAL,
    est_shipping REAL,
    est_net_profit REAL,
    score REAL,               -- 0-100, NULL = unpriceable
    score_breakdown TEXT,     -- JSON: per-factor contributions, auditable in UI
    computed_at TEXT NOT NULL
);

CREATE INDEX idx_valuations_score ON valuations (score);

CREATE TABLE user_actions (
    listing_id INTEGER PRIMARY KEY REFERENCES listings(id) ON DELETE CASCADE,
    seen INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    saved INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE adapter_state (
    adapter_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'ok', -- ok | degraded | disabled
    status_detail TEXT,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_success_at TEXT,
    last_error_at TEXT,
    backoff_until TEXT
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
