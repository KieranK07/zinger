//! Full poll cycle through the real poll layer: adapter -> ingest -> dedup
//! -> adapter_state bookkeeping (backoff, isolation). Strictly offline:
//! image/phash enrichment is disabled via PollOptions (NO_NET) and the
//! only adapters used are fixture-backed or synthetic.

use nexus_lib::adapters::{mock::MockAdapter, AdapterRegistry};
use nexus_lib::db;
use nexus_lib::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use nexus_lib::poll::{self, PollOptions};
use std::sync::Mutex;

fn setup() -> (Mutex<rusqlite::Connection>, AdapterRegistry) {
    let conn = db::open_in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(MockAdapter::new()));
    (Mutex::new(conn), registry)
}

// The seeded example search (id 1) is "dewalt drill" — matches the two
// cross-posted drill fixtures.
const SEEDED_SEARCH_ID: i64 = 1;

/// Tests must never touch the network: no image fetching.
const NO_NET: PollOptions = PollOptions { fetch_images: false };

#[tokio::test]
async fn poll_cycle_ingests_and_dedupes() {
    let (db, registry) = setup();

    let report = poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    assert_eq!(report.adapters.len(), 1);
    assert!(report.adapters[0].error.is_none());
    assert_eq!(report.adapters[0].fetched, 2); // the cross-posted drill pair
    assert_eq!(report.inserted, 1); // ...collapses to one listing
    assert_eq!(report.duplicates_skipped, 1);

    let conn = db.lock().unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM listings", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
    let status: String = conn
        .query_row("SELECT status FROM adapter_state WHERE adapter_id='mock'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(status, "ok");
}

#[tokio::test]
async fn second_cycle_is_a_no_op() {
    let (db, registry) = setup();

    poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    let report = poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    assert_eq!(report.inserted, 0);
    assert_eq!(report.already_known, 1);
    assert_eq!(report.duplicates_skipped, 1); // cross-post still caught by hash
}

#[tokio::test]
async fn missing_search_is_an_error_not_a_panic() {
    let (db, registry) = setup();
    let err = poll::run_poll_cycle_with(&db, &registry, 9999, NO_NET).await.unwrap_err();
    assert!(err.contains("not found"));
}

// ---------- failure isolation & backoff ----------

struct FailingAdapter;

#[async_trait::async_trait]
impl nexus_lib::adapters::MarketAdapter for FailingAdapter {
    fn id(&self) -> &'static str {
        "failing"
    }
    fn display_name(&self) -> &'static str {
        "Always Fails"
    }
    async fn search(
        &self,
        _spec: &SearchSpec,
        _ctx: &nexus_lib::adapters::SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError> {
        Err(AdapterError::Blocked("simulated block".into()))
    }
    fn health(&self) -> AdapterStatus {
        AdapterStatus::Ok
    }
    fn rate_limit_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy { min_delay_ms: 0, max_delay_ms: 0, max_requests_per_cycle: 1 }
    }
}

struct NotConfiguredAdapter;

#[async_trait::async_trait]
impl nexus_lib::adapters::MarketAdapter for NotConfiguredAdapter {
    fn id(&self) -> &'static str {
        "unconfigured"
    }
    fn display_name(&self) -> &'static str {
        "Needs Keys"
    }
    async fn search(
        &self,
        _spec: &SearchSpec,
        _ctx: &nexus_lib::adapters::SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError> {
        panic!("must never be called when not configured");
    }
    fn health(&self) -> AdapterStatus {
        AdapterStatus::NotConfigured { reason: "no keys".into() }
    }
    fn rate_limit_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy { min_delay_ms: 0, max_delay_ms: 0, max_requests_per_cycle: 1 }
    }
}

#[tokio::test]
async fn one_failing_adapter_never_blocks_others() {
    let conn = db::open_in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(FailingAdapter));
    registry.register(Box::new(NotConfiguredAdapter));
    registry.register(Box::new(MockAdapter::new()));
    let db = Mutex::new(conn);

    let report = poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    assert_eq!(report.adapters.len(), 3);

    let failing = &report.adapters[0];
    assert!(failing.error.as_deref().unwrap().contains("simulated block"));
    let unconfigured = &report.adapters[1];
    assert!(unconfigured.skipped.as_deref().unwrap().contains("not configured"));
    let mock = &report.adapters[2];
    assert!(mock.error.is_none());
    assert_eq!(report.inserted, 1); // mock results landed despite the others
}

#[tokio::test]
async fn blocked_adapter_backs_off_and_is_skipped_next_cycle() {
    let conn = db::open_in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(FailingAdapter));
    let db = Mutex::new(conn);

    let first = poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    assert!(first.adapters[0].error.is_some());
    {
        let conn = db.lock().unwrap();
        let (status, failures): (String, i64) = conn
            .query_row(
                "SELECT status, consecutive_failures FROM adapter_state WHERE adapter_id='failing'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "degraded");
        assert_eq!(failures, 1);
    }

    // Second cycle: the adapter is inside its backoff window, so it's
    // skipped — no new error, failure count unchanged.
    let second = poll::run_poll_cycle_with(&db, &registry, SEEDED_SEARCH_ID, NO_NET).await.unwrap();
    assert!(second.adapters[0].skipped.as_deref().unwrap().contains("backing off"));
    assert!(second.adapters[0].error.is_none());
    {
        let conn = db.lock().unwrap();
        let failures: i64 = conn
            .query_row(
                "SELECT consecutive_failures FROM adapter_state WHERE adapter_id='failing'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(failures, 1);
    }
}
