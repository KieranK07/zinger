//! Full poll cycle against the fixture adapter: search -> ingest -> dedup.

use nexus_lib::adapters::{mock::MockAdapter, AdapterRegistry};
use nexus_lib::db;
use nexus_lib::models::SearchSpec;
use nexus_lib::pipeline;

fn registry() -> AdapterRegistry {
    let mut r = AdapterRegistry::new();
    r.register(Box::new(MockAdapter::new()));
    r
}

fn match_all_spec() -> SearchSpec {
    SearchSpec {
        keywords: String::new(),
        category: None,
        location_text: "Seattle, WA".into(),
        lat: Some(47.6062),
        lng: Some(-122.3321),
        radius_km: 40.0,
        price_ceiling: None,
    }
}

#[tokio::test]
async fn poll_cycle_ingests_and_dedupes() {
    let conn = db::open_in_memory().unwrap();
    let registry = registry();
    let spec = match_all_spec();

    let runs = registry.search_all(&spec).await;
    assert_eq!(runs.len(), 1);
    let raw = runs.into_iter().next().unwrap().result.unwrap();
    assert_eq!(raw.len(), 12);

    let stats = pipeline::ingest(&conn, 1, raw).unwrap();
    // Fixtures contain one cross-posted pair (the DeWalt drill) that must
    // collapse via dedup_hash.
    assert_eq!(stats.inserted, 11);
    assert_eq!(stats.duplicates_skipped, 1);
    assert_eq!(stats.already_known, 0);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM listings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 11);
}

#[tokio::test]
async fn second_cycle_is_a_no_op() {
    let conn = db::open_in_memory().unwrap();
    let registry = registry();
    let spec = match_all_spec();

    let raw = registry.search_all(&spec).await.remove(0).result.unwrap();
    pipeline::ingest(&conn, 1, raw).unwrap();

    let raw = registry.search_all(&spec).await.remove(0).result.unwrap();
    let stats = pipeline::ingest(&conn, 1, raw).unwrap();
    assert_eq!(stats.inserted, 0);
    assert_eq!(stats.already_known, 11);
    // The cross-post is still caught by hash, never by source_id.
    assert_eq!(stats.duplicates_skipped, 1);
}
