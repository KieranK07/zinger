# Zinger Architecture

```
UI (React + TS + Tailwind, src/)
        ⇅ Tauri commands (src-tauri/src/commands.rs)
Core (Rust, src-tauri/src/)
        ├─ AdapterRegistry → [Craigslist, eBay, Mock*] adapters/   (*dev builds only)
        ├─ Poll layer: isolation, backoff, phash       poll.rs
        ├─ Scheduler (per-search interval + jitter)    scheduler.rs
        ├─ Dedup + normalization pipeline              pipeline.rs
        ├─ SQLite store (rusqlite + refinery)          db.rs, migrations/
        ├─ Settings                                    settings.rs
        ├─ Valuation engine (M3)
        └─ Notifier (M4)
```

## Decisions

**Tauri 2 over Electron.** Small binaries, Rust core where the scheduler/parsers/DB live anyway, OS keychain and notification plugins first-party. No deviation from the recommended stack was needed.

**rusqlite + refinery over sqlx.** Single-user desktop app: one connection behind a mutex is sufficient, and synchronous DB calls keep command handlers simple. sqlx's async pool and compile-time query checking add build complexity (DATABASE_URL at compile time) without buying anything at this scale. refinery gives embedded, versioned, run-on-startup migrations.

**DB access pattern.** One `Mutex<rusqlite::Connection>` in Tauri-managed state. Async adapter I/O always happens *before* the lock is taken (see `run_search`), so slow sources never block the UI thread on the lock. If contention ever matters, switch to a small r2d2 pool — the call sites won't change shape.

**Adapter trait is the load-bearing interface.** `search(spec) -> Result<Vec<RawListing>>` + `health()` + `rate_limit_policy()`. The registry isolates each adapter's failure into per-adapter `AdapterRun` results — one source breaking is data to display, never control flow that aborts a cycle. Rate-limit policy is declared *by* the adapter so politeness limits live next to the source they protect; the M2 scheduler enforces them.

**Dedup: hash, not similarity (for now).** `sha256(normalized_title | price/5-bucket)` truncated to 128 bits. Normalization lowercases and strips punctuation; the $5 price bucket absorbs trivial price edits. This catches the dominant case — the same item cross-posted with cosmetic title differences. Image perceptual hashing (phash) is planned for M2 when real image URLs flow; title+price+location similarity remains the documented fallback for image-less sources.

> **Known-fragile:** the $5 price bucket is a step function. Identical cross-posts whose prices straddle a bucket boundary do not dedup, while a larger gap inside one bucket does. With `round(price/5)` the boundaries sit at $2.50 offsets: $97 vs $98 lands in buckets 19 vs 20 (no dedup), yet $98 vs $102 both land in 20 (dedups). Accepted for M1. The fix is perceptual image hashing (phash) in M2: same photo ⇒ duplicate regardless of price drift; the title/price hash stays as the fallback for image-less listings.

**Fixtures are embedded (`include_str!`).** The mock adapter compiles its fixture JSON into the binary, so dev builds, tests, and packaged apps behave identically with no resource-path handling. Real adapter parser tests (M2) will follow the same pattern with saved HTML pages.

**Settings are plain key-value rows; API keys are not settings.** The `settings` table stores non-secret config (fees, intervals, thresholds). BYOK API keys (M5) go to the OS keychain via the Tauri keyring plugin — never the DB, never logs.

**Score is honest about its absence.** Valuation lands in M3; until then the UI shows "score —" rather than a fake number, and the schema already reserves `valuations.score = NULL` to mean "unpriceable" rather than zero.

**Craigslist: parse the no-JS static fallback, never a headless browser.** CL search pages are JS-rendered, but they ship a server-rendered `li.cl-static-search-result` fallback carrying url/title/price/location. We parse that (one request per search), then fetch detail pages only for listings we haven't seen, capped at 8 per cycle with randomized 5–15s delays. New listings beyond the cap are deliberately dropped that cycle — the next poll picks them up, keeping every cycle's footprint bounded. A page with neither results nor the fallback markup is a loud `Parse` error (layout change or block), never a silent "no matches".

**`SearchContext` keeps adapters DB-free.** Craigslist needs to know which listings are already stored to avoid re-fetching their detail pages forever. Rather than handing adapters a DB connection, the poll layer passes `known_source_ids` in a context struct. Adapters stay pure fetch+parse; cross-cycle knowledge stays in one place.

**Failure bookkeeping lives in the poll layer, not adapters.** `poll.rs` owns `adapter_state`: consecutive failures, exponential backoff (5 min base, ×2 per failure, blocks start at 20 min, cap 24 h), and `disabled` status after 3 consecutive failures. Adapters just return typed errors (`Blocked` vs `Network` vs `Parse`); the policy reaction is centralized and identically applied to every source. NotConfigured adapters (eBay without keys) are skipped before any of this — silence, not errors.

**phash enrichment happens in the poll layer, after search, before ingest.** Adapters return image URLs; `poll.rs` downloads the first image (10 s timeout, 1 MB cap), computes an 8×8 gradient hash (`image_hasher`), and ingest treats hamming distance ≤ 6 as "same photo ⇒ duplicate" — which closes the price-bucket-boundary hole documented in M1 (verified by test: $97 vs $98 cross-post with the same photo dedups). Any enrichment failure degrades to the title/price fallback hash. Tests disable enrichment entirely (`PollOptions { fetch_images: false }`) — no network in tests, ever.

**keyring v3, not v4.** eBay credentials live in the OS keychain via the `keyring` crate. v4 was rejected because it pulls a full SQLite engine (turso) through `db-keystore` — absurd weight for two secrets; v3 binds the platform-native stores directly.

**Scheduler keeps due-times in memory.** One tokio task ticks per minute; each enabled search is due after its interval (per-search column, else global setting) ±20% jitter. Nothing is persisted: a restart re-polls early at worst, and ingest dedups the overlap. The mock adapter is registered only in debug builds so it can't pollute real searches.

**Facebook Marketplace runs in a sidecar process, never the Tauri webview.** (Decided for M2.5.) Verbatim reasoning:

- The Tauri webview shares process and cookie context with the main app; FB's automation-detection surface and FB session cookies must not touch the primary app process or its webview store.
- The "browser dependency must not load when the adapter is disabled" constraint is unenforceable if automation lives in the always-loaded primary webview; a sidecar is spawned only when FB is enabled and torn down otherwise.
- Session isolation, a dedicated browser profile, and clean teardown are natural in a sidecar and awkward-to-impossible in the shared webview.

**Sidecar runtime: Node + Playwright, persistent Chromium profile.** Playwright's `launchPersistentContext(profileDir)` is the login-once mechanism. Confirmed by test, not assumption: a persistent httpOnly cookie + localStorage written by one process into a profile dir were read back intact by a *separate* process reusing that dir (cookies flushed to the on-disk `Cookies` SQLite store). FB's `c_user`/`xs` auth cookies are persistent (future-dated) cookies, so a logged-in session survives an app restart the same way; session-only cookies would not, which is why the persistent-cookie path is the one that matters. The profile dir lives beside `zinger.db` under the app-data dir (`~/Library/Application Support/com.zinger.app/fb-profile/`), whose parent is already user-only (`drwx------`); no separate secret is needed because the session lives entirely in the profile, but any separately-referenced token would go to the keychain, never plaintext. The sidecar is an ordinary spawned child process: started only when FB is enabled, terminated (SIGTERM→SIGKILL) when disabled or after a cycle, so the browser dependency genuinely does not load while the adapter is off. **Size:** Chromium is ~341 MB — too large to bundle for an opt-in, off-by-default source, so it is downloaded on first FB-enable (Playwright JS adds ~17 MB; the Node runtime is shared/located at launch). The base app stays small; users who never touch FB never pay the cost. Download streams coarse `%` progress (parsed from `playwright install` output) to the UI; a failed/offline download leaves health at "Chromium download required" with a retry action and never half-installs.

**Pinned versions (condition: no silent upgrade can break the parser or trigger a surprise re-download).** `sidecar/package.json` pins `playwright@1.60.0` exactly; that release resolves to the Chromium 1223 build. Bump both together, deliberately, and re-green the parser against a fresh DOM snapshot when doing so.

**FB sidecar protocol & state machine.** The Rust adapter and the Node sidecar speak newline-delimited JSON over stdout (`status`/`install`/`login`/`search`; events `progress`/`installed`/`logged_in`/`page`/`done`/`challenge`/`not_logged_in`/`error`). Parsing stays pure Rust against fixtures (`fixtures/facebook_search.html`), same as every other adapter — the sidecar only ships rendered HTML. FB-specific health policy lives in the adapter, not the shared poll layer: a **challenge/checkpoint** stops the run immediately and, on the second consecutive one, self-disables for 24 h; a **login wall** (session expiry) returns `AdapterError::Auth` so health shows "re-login required" with a one-click re-auth — never a silent empty result. Non-runnable states (off / no ToS / no Chromium / 24 h-disabled) return `Ok(empty)` *before* any spawn, which is what makes the "browser doesn't load when disabled" guarantee hold (verified by unit test + a runtime boot check: no sidecar, Chromium, or profile dir appears while FB is off).

## Milestone log

- **M1 (done):** app boots; migrations create `searches/listings/comps/valuations/user_actions/adapter_state/settings`; mock adapter → pipeline → DB → deals feed renders; settings persist; 13 tests (no network).
- **M2 (done):** live Craigslist adapter (fixture-first parser; real fixtures captured 2026-06-12); eBay Browse adapter (BYOK keychain creds, clean no-op unconfigured, test-connection); poll layer with per-adapter backoff/auto-disable; background scheduler with jitter; phash dedup closing the bucket-boundary hole; 34 offline tests + 1 manual live smoke (passed: 8 listings).
- **M2.5 (done):** Facebook Marketplace via on-demand Node+Playwright sidecar (decision + session-persistence test recorded above). Off by default; one-time ToS-accept gate; lazy Chromium download with progress; manual login into a persistent profile; fixture-first parser anchored on `/marketplace/item/` + currency text; challenge→24h self-disable, expiry→re-login (never silent empty); isolation verified (no sidecar/Chromium while disabled). 40 offline tests. Live search path is experimental — selectors need revalidation against a real logged-in snapshot, and the manual live smoke needs an FB account so it isn't run in this environment.
