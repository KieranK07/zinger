use rusqlite::Connection;
use std::collections::HashMap;

// Plain key-value settings. API keys do NOT go here — they go to the OS
// keychain when BYOK lands in M5.

pub fn get_all(conn: &Connection) -> Result<HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn set_and_get_roundtrip() {
        let conn = db::open_in_memory().unwrap();
        set(&conn, "platform_fee_pct", "10.5").unwrap();
        let all = get_all(&conn).unwrap();
        assert_eq!(all.get("platform_fee_pct").map(String::as_str), Some("10.5"));
    }
}
