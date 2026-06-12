# Nexus Architecture

```
UI (React + TS + Tailwind, src/)
        ⇅ Tauri commands (src-tauri/src/commands.rs)
Core (Rust, src-tauri/src/)
        ├─ AdapterRegistry → [MockAdapter]            adapters/
        ├─ Dedup + normalization pipeline             pipeline.rs
        ├─ SQLite store (rusqlite + refinery)         db.rs, migrations/
        ├─ Settings                                   settings.rs
        ├─ Scheduler (M2)
        ├─ Valuation engine (M3)
        └─ Notifier (M4)
```

## Decisions

**Tauri 2 over Electron.** Small binaries, Rust core where the scheduler/parsers/DB live anyway, OS keychain and notification plugins first-party. No deviation from the recommended stack was needed.

**rusqlite + refinery over sqlx.** Single-user desktop app: one connection behind a mutex is sufficient, and synchronous DB calls keep command handlers simple. sqlx's async pool and compile-time query checking add build complexity (DATABASE_URL at compile time) without buying anything at this scale. refinery gives embedded, versioned, run-on-startup migrations.

**DB access pattern.** One `Mutex<rusqlite::Connection>` in Tauri-managed state. Async adapter I/O always happens *before* the lock is taken (see `run_search`), so slow sources never block the UI thread on the lock. If contention ever matters, switch to a small r2d2 pool — the call sites won't change shape.

**Adapter trait is the load-bearing interface.** `search(spec) -> Result<Vec<RawListing>>` + `health()` + `rate_limit_policy()`. The registry isolates each adapter's failure into per-adapter `AdapterRun` results — one source breaking is data to display, never control flow that aborts a cycle. Rate-limit policy is declared *by* the adapter so politeness limits live next to the source they protect; the M2 scheduler enforces them.

**Dedup: hash, not similarity (for now).** `sha256(normalized_title | price/5-bucket)` truncated to 128 bits. Normalization lowercases and strips punctuation; the $5 price bucket absorbs trivial price edits. This catches the dominant case — the same item cross-posted with cosmetic title differences. Image perceptual hashing (phash) is planned for M2 when real image URLs flow; title+price+location similarity remains the documented fallback for image-less sources.

**Fixtures are embedded (`include_str!`).** The mock adapter compiles its fixture JSON into the binary, so dev builds, tests, and packaged apps behave identically with no resource-path handling. Real adapter parser tests (M2) will follow the same pattern with saved HTML pages.

**Settings are plain key-value rows; API keys are not settings.** The `settings` table stores non-secret config (fees, intervals, thresholds). BYOK API keys (M5) go to the OS keychain via the Tauri keyring plugin — never the DB, never logs.

**Score is honest about its absence.** Valuation lands in M3; until then the UI shows "score —" rather than a fake number, and the schema already reserves `valuations.score = NULL` to mean "unpriceable" rather than zero.

## Milestone log

- **M1 (done):** app boots; migrations create `searches/listings/comps/valuations/user_actions/adapter_state/settings`; mock adapter → pipeline → DB → deals feed renders; settings persist; 13 tests (no network).
