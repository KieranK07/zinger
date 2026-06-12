use crate::models::RawListing;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

/// Dedup hash: normalized title + price bucket. Image perceptual hashing is
/// planned for M2; this catches the common case of cross-posted listings with
/// cosmetic title differences.
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

#[derive(Debug, Default, serde::Serialize)]
pub struct IngestStats {
    pub inserted: usize,
    pub duplicates_skipped: usize,
    pub already_known: usize,
}

/// Insert raw listings for a search, skipping anything already present —
/// either the same source listing (source, source_id) or a cross-post dupe
/// (same dedup_hash anywhere in the DB).
pub fn ingest(
    conn: &Connection,
    search_id: i64,
    raw: Vec<RawListing>,
) -> Result<IngestStats, rusqlite::Error> {
    let mut stats = IngestStats::default();
    let now = chrono::Utc::now().to_rfc3339();

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
                condition, category_guess, dedup_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
            ],
        )?;
        stats.inserted += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
