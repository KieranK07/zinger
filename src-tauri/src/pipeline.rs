use crate::models::RawListing;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

/// Max hamming distance between two 64-bit phashes to call them the same
/// image. 6/64 bits tolerates re-encoding and thumbnail scaling while
/// keeping distinct products apart.
pub const PHASH_DISTANCE_THRESHOLD: u32 = 6;

/// Fallback dedup hash: normalized title + price bucket. Known-fragile at
/// bucket boundaries (see ARCHITECTURE.md); phash is the primary signal
/// whenever images exist.
pub fn dedup_hash(title: &str, price: f64) -> String {
    let normalized: String = title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    // $5 buckets so trivial price edits ($65 vs $65.00) still collide.
    let price_bucket = (price / 5.0).round() as i64;
    let mut hasher = Sha256::new();
    hasher.update(tokens.join(" "));
    hasher.update(b"|");
    hasher.update(price_bucket.to_le_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..16])
}

/// Hamming distance between two base64-encoded image_hasher hashes.
/// None if either fails to decode (treat as "not comparable").
fn phash_distance(a: &str, b: &str) -> Option<u32> {
    let a = image_hasher::ImageHash::<Box<[u8]>>::from_base64(a).ok()?;
    let b = image_hasher::ImageHash::<Box<[u8]>>::from_base64(b).ok()?;
    Some(a.dist(&b))
}

#[derive(Debug, Default, serde::Serialize)]
pub struct IngestStats {
    pub inserted: usize,
    pub duplicates_skipped: usize,
    pub already_known: usize,
}

/// Insert raw listings for a search, skipping anything already present:
/// 1. same (source, source_id)            -> already_known
/// 2. phash within threshold of any row   -> duplicate (cross-post, same photo)
/// 3. same title/price-bucket hash        -> duplicate (fallback, no images)
pub fn ingest(
    conn: &Connection,
    search_id: i64,
    raw: Vec<RawListing>,
) -> Result<IngestStats, rusqlite::Error> {
    let mut stats = IngestStats::default();
    let now = chrono::Utc::now().to_rfc3339();

    // Load stored phashes once per batch; personal-scale DB makes a linear
    // scan in Rust cheaper than any SQLite gymnastics.
    let mut stored_phashes: Vec<String> = conn
        .prepare("SELECT phash FROM listings WHERE phash IS NOT NULL")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    for listing in raw {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM listings WHERE source = ?1 AND source_id = ?2)",
            params![listing.source, listing.source_id],
            |row| row.get(0),
        )?;
        if exists {
            stats.already_known += 1;
            continue;
        }

        if let Some(new_phash) = &listing.phash {
            let is_dupe = stored_phashes.iter().any(|stored| {
                phash_distance(new_phash, stored)
                    .is_some_and(|d| d <= PHASH_DISTANCE_THRESHOLD)
            });
            if is_dupe {
                stats.duplicates_skipped += 1;
                continue;
            }
        }

        let hash = dedup_hash(&listing.title, listing.price);
        let dupe: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM listings WHERE dedup_hash = ?1)",
            params![hash],
            |row| row.get(0),
        )?;
        if dupe {
            stats.duplicates_skipped += 1;
            continue;
        }

        conn.execute(
            "INSERT INTO listings (search_id, source, source_id, source_url, title, description,
                price, currency, location_text, lat, lng, images, posted_at, fetched_at,
                condition, category_guess, dedup_hash, phash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                search_id,
                listing.source,
                listing.source_id,
                listing.source_url,
                listing.title.trim(),
                listing.description,
                listing.price,
                listing.currency,
                listing.location_text,
                listing.lat,
                listing.lng,
                serde_json::to_string(&listing.images).unwrap_or_else(|_| "[]".into()),
                listing.posted_at,
                now,
                listing.condition.map(|c| c.as_str()),
                Option::<String>::None, // category_guess: M3, with valuation
                hash,
                listing.phash,
            ],
        )?;
        if let Some(p) = listing.phash {
            stored_phashes.push(p);
        }
        stats.inserted += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::RawListing;
    use image_hasher::HasherConfig;

    #[test]
    fn hash_is_stable() {
        assert_eq!(dedup_hash("DeWalt Drill", 65.0), dedup_hash("DeWalt Drill", 65.0));
    }

    #[test]
    fn hash_ignores_case_punctuation_and_small_price_drift() {
        let a = dedup_hash("DeWalt DCD791 20V Max Cordless Drill - barely used", 65.0);
        let b = dedup_hash("Dewalt dcd791 20v max cordless drill — BARELY USED!", 66.0);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_differs_for_different_items() {
        let a = dedup_hash("Nintendo Switch OLED", 180.0);
        let b = dedup_hash("Nintendo Switch Lite", 180.0);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_differs_across_price_buckets() {
        let a = dedup_hash("Herman Miller Aeron", 250.0);
        let b = dedup_hash("Herman Miller Aeron", 400.0);
        assert_ne!(a, b);
    }

    fn raw(source_id: &str, title: &str, price: f64, phash: Option<String>) -> RawListing {
        RawListing {
            source: "test".into(),
            source_id: source_id.into(),
            source_url: format!("https://example.com/{source_id}"),
            title: title.into(),
            description: None,
            price,
            currency: "USD".into(),
            location_text: None,
            lat: None,
            lng: None,
            images: vec![],
            posted_at: None,
            condition: None,
            phash,
        }
    }

    /// Generate a phash from a synthetic image: a horizontal gradient with a
    /// dark square whose position varies by `seed` — different seeds give
    /// genuinely different image structure, like different products.
    fn synthetic_phash(seed: u32) -> String {
        let mut img = image::GrayImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Luma([(x * 4) as u8]);
            let sq = 8 + (seed * 13) % 36;
            if x >= sq && x < sq + 16 && y >= sq && y < sq + 16 {
                *p = image::Luma([0]);
            }
        }
        let hasher = HasherConfig::new().hash_size(8, 8).to_hasher();
        hasher.hash_image(&image::DynamicImage::ImageLuma8(img)).to_base64()
    }

    #[test]
    fn phash_dedups_across_price_bucket_boundary() {
        // The known-fragile case: $97 vs $98 land in different $5 buckets,
        // so the fallback hash misses the dupe — the shared photo must catch it.
        let conn = db::open_in_memory().unwrap();
        let photo = synthetic_phash(1);
        assert_ne!(dedup_hash("Dyson V8 vacuum", 97.0), dedup_hash("Dyson V8 vacuum", 98.0));

        let stats = ingest(&conn, 1, vec![
            raw("a1", "Dyson V8 vacuum", 97.0, Some(photo.clone())),
            raw("a2", "Dyson V8 vacuum", 98.0, Some(photo)),
        ])
        .unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.duplicates_skipped, 1);
    }

    #[test]
    fn distinct_images_do_not_collide() {
        let conn = db::open_in_memory().unwrap();
        let stats = ingest(&conn, 1, vec![
            raw("b1", "Mystery box lot one", 50.0, Some(synthetic_phash(1))),
            raw("b2", "Mystery box lot two", 250.0, Some(synthetic_phash(2))),
        ])
        .unwrap();
        assert_eq!(stats.inserted, 2);
        assert_eq!(stats.duplicates_skipped, 0);
    }

    #[test]
    fn no_phash_falls_back_to_title_price_hash() {
        let conn = db::open_in_memory().unwrap();
        let stats = ingest(&conn, 1, vec![
            raw("c1", "KitchenAid Mixer Red", 140.0, None),
            raw("c2", "kitchenaid mixer red!", 141.0, None),
        ])
        .unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.duplicates_skipped, 1);
    }

    #[test]
    fn phash_listing_still_deduped_by_fallback_when_titles_match() {
        // Same title/price but *different* photos (e.g. seller re-shot the
        // item): fallback hash still treats as dupe. Photographic difference
        // must not resurrect an otherwise identical listing.
        let conn = db::open_in_memory().unwrap();
        let stats = ingest(&conn, 1, vec![
            raw("d1", "Trek FX 3 bike", 320.0, Some(synthetic_phash(1))),
            raw("d2", "Trek FX 3 bike", 320.0, Some(synthetic_phash(2))),
        ])
        .unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.duplicates_skipped, 1);
    }
}
