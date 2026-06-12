use rusqlite::Connection;
use std::path::Path;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("./migrations");
}

pub fn open(db_path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    embedded::migrations::runner().run(&mut conn)?;
    seed_defaults(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection, Box<dyn std::error::Error>> {
    let mut conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    embedded::migrations::runner().run(&mut conn)?;
    seed_defaults(&conn)?;
    Ok(conn)
}

/// Sensible defaults instead of an onboarding wizard: one example search and
/// the settings the valuation engine (M3) will read.
fn seed_defaults(conn: &Connection) -> Result<(), rusqlite::Error> {
    let search_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM searches", [], |row| row.get(0))?;
    if search_count == 0 {
        conn.execute(
            "INSERT INTO searches (name, keywords, location_text, lat, lng, radius_km, price_ceiling)
             VALUES ('Example: cordless drills', 'dewalt drill', 'Seattle, WA', 47.6062, -122.3321, 40.0, 150.0)",
            [],
        )?;
    }

    let defaults = [
        ("platform_fee_pct", "13.0"),
        ("shipping_flat_default", "12.0"),
        ("poll_interval_minutes", "30"),
        ("notify_score_threshold_default", "70"),
        ("quiet_hours_start", "22:00"),
        ("quiet_hours_end", "08:00"),
    ];
    for (key, value) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_all_tables() {
        let conn = open_in_memory().unwrap();
        let expected = [
            "searches",
            "listings",
            "comps",
            "valuations",
            "user_actions",
            "adapter_state",
            "settings",
        ];
        for table in expected {
            let found: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(found, "missing table: {table}");
        }
    }

    #[test]
    fn defaults_are_seeded() {
        let conn = open_in_memory().unwrap();
        let searches: i64 = conn
            .query_row("SELECT COUNT(*) FROM searches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(searches, 1);
        let fee: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='platform_fee_pct'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fee, "13.0");
    }

    #[test]
    fn migrations_are_idempotent_on_reopen() {
        let dir = std::env::temp_dir().join(format!("nexus-test-{}", std::process::id()));
        let path = dir.join("test.db");
        let _ = std::fs::remove_file(&path);
        {
            let conn = open(&path).unwrap();
            drop(conn);
        }
        // Second open must not re-run V1 or duplicate seeds.
        let conn = open(&path).unwrap();
        let searches: i64 = conn
            .query_row("SELECT COUNT(*) FROM searches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(searches, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
