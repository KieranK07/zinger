# Zinger

A local-first desktop app for second-hand marketplace arbitrage: find underpriced items near you, estimate resale value from comparable sold prices, and rank listings by expected profit.

**Personal tool, not a service.** No accounts, no backend, no telemetry. Your data and API keys never leave your machine.

## Status: M2.5 (Facebook Marketplace, experimental)

Working now:
- **Live Craigslist adapter**: polite parsing of public search pages (one request per search, detail pages for new listings only, randomized 5–15s delays, exponential backoff, auto-disable after repeated blocks)
- **eBay adapter (BYOK)**: official Browse API with your own developer keys, stored in the OS keychain. Without keys it shows "not configured" and stays silent. Settings has a "Test connection" button
- **Facebook Marketplace (experimental, off by default)**: an opt-in adapter that drives *your own* logged-in session via a private, isolated browser sidecar. See the ToS warning below before enabling
- **Background scheduler**: per-search polling interval (or a global default) with jitter; the deals feed refreshes live when a cycle completes
- **Smarter dedup**: perceptual image hashing catches cross-posts even when prices differ; title+price hashing remains the fallback for image-less listings
- Saved searches, deals feed, search manager, settings; save/hide actions; SQLite with versioned migrations

Coming next (M3): valuation from eBay sold comps; FB and Craigslist listings score against those comps like every other source.

### eBay setup (optional but recommended)
1. Create a (free) developer account at developer.ebay.com and an app with production keys.
2. Settings → eBay API keys: paste the App ID (client ID) and Cert ID (client secret), Save, then Test connection.
3. Keys go to your OS keychain — never the database, never logs.

### Craigslist site
The adapter derives the CL subdomain from each search's location ("Seattle, WA" → `seattle.craigslist.org`). Metros whose site name isn't the city name (e.g. the Bay Area's `sfbay`) can set it explicitly via Settings → "Craigslist site" (restart to apply).

## Honest constraints — read this

**Marketplace access is the riskiest part of this app, by design.**

- **eBay** has official APIs. Zinger uses them with *your own* developer keys. This is the reliable path and powers valuation comps.
- **Craigslist** has no API and has litigated against scrapers. The Craigslist adapter parses public search pages with conservative randomized delays (5–15s), an honest user agent, aggressive caching, and automatic backoff/disable on failures. Using it may still violate Craigslist's ToS. It is your decision and your risk.
- **Facebook Marketplace** has no public API, and automated access **violates Facebook's Terms of Service.** The adapter is **off by default** and gated behind a one-time warning you must explicitly accept. It uses **your own account**, which you log into yourself in a visible browser window; Zinger never sees or stores your credentials, and the session lives only in a private browser profile on your machine. To reduce (not eliminate) detection risk it minimizes automation signals, waits a randomized 20–45s between page loads, caps pages per cycle (default 3), and **auto-disables for 24 hours after two consecutive challenges/blocks** — a challenge stops it immediately rather than retrying. **None of this removes the risk that Facebook detects automation and restricts or bans your account. Enabling it is your decision and your risk.** Enabling triggers a one-time ~341 MB private-browser download. The live search path is experimental: Facebook's obfuscated, frequently-changing markup means the parser can break without warning and silently return fewer or no results until updated.
- **OfferUp** prohibits scraping and has no public API. Not implemented — only the adapter interface exists.

The Facebook browser sidecar is a **separate process spawned only when the adapter is enabled** and torn down when disabled; it shares nothing with the main app, and while Facebook is off it does not load at all.

Every adapter degrades independently: one source breaking or blocking never takes down the others or the app.

**Valuation needs no AI.** Estimates come from median/IQR statistics over comparable sold listings. AI (M5) is an optional, bring-your-own-key refinement layer — model extraction from messy titles, scam red-flags — and the app is fully functional with zero AI configured. If no comps exist, a listing is marked "unpriceable"; Zinger never fabricates a value.

## Development

Prereqs: Rust (1.80+), Node 20+, and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev      # run the app
npm run tauri build    # package for your platform

cd src-tauri
cargo test             # Rust unit + integration tests (no network, fixture-based)
```

The Facebook adapter shells out to a Node sidecar in `sidecar/`. It only matters if you enable FB; install its deps with `cd sidecar && npm install`. Chromium itself is fetched lazily on first FB-enable, not at dev-setup time.

The SQLite database lives in your platform's app-data directory (e.g. `~/Library/Application Support/com.zinger.app/zinger.db` on macOS); the Facebook browser profile, when used, sits beside it in `fb-profile/`.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md). Short version: React UI ⇄ Tauri commands ⇄ Rust core (adapter registry → dedup pipeline → SQLite; scheduler and valuation engine land in M2/M3).
