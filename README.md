# Nexus

A local-first desktop app for second-hand marketplace arbitrage: find underpriced items near you, estimate resale value from comparable sold prices, and rank listings by expected profit.

**Personal tool, not a service.** No accounts, no backend, no telemetry. Your data and API keys never leave your machine.

## Status: M2 (real data)

Working now:
- **Live Craigslist adapter**: polite parsing of public search pages (one request per search, detail pages for new listings only, randomized 5–15s delays, exponential backoff, auto-disable after repeated blocks)
- **eBay adapter (BYOK)**: official Browse API with your own developer keys, stored in the OS keychain. Without keys it shows "not configured" and stays silent. Settings has a "Test connection" button
- **Background scheduler**: per-search polling interval (or a global default) with jitter; the deals feed refreshes live when a cycle completes
- **Smarter dedup**: perceptual image hashing catches cross-posts even when prices differ; title+price hashing remains the fallback for image-less listings
- Saved searches, deals feed, search manager, settings; save/hide actions; SQLite with versioned migrations

Coming next (M2.5): Facebook Marketplace adapter — off by default, explicit ToS warning, your own session. Then M3: valuation from eBay sold comps.

### eBay setup (optional but recommended)
1. Create a (free) developer account at developer.ebay.com and an app with production keys.
2. Settings → eBay API keys: paste the App ID (client ID) and Cert ID (client secret), Save, then Test connection.
3. Keys go to your OS keychain — never the database, never logs.

### Craigslist site
The adapter derives the CL subdomain from each search's location ("Seattle, WA" → `seattle.craigslist.org`). Metros whose site name isn't the city name (e.g. the Bay Area's `sfbay`) can set it explicitly via Settings → "Craigslist site" (restart to apply).

## Honest constraints — read this

**Marketplace access is the riskiest part of this app, by design.**

- **eBay** has official APIs. Nexus uses them with *your own* developer keys. This is the reliable path and powers valuation comps.
- **Craigslist** has no API and has litigated against scrapers. The Craigslist adapter parses public search pages with conservative randomized delays (5–15s), an honest user agent, aggressive caching, and automatic backoff/disable on failures. Using it may still violate Craigslist's ToS. It is your decision and your risk.
- **Facebook Marketplace and OfferUp** prohibit scraping and have no public APIs. Adapters for them are **not implemented** in the MVP — only the interface stub exists. If implemented post-MVP they will be off by default, clearly labeled experimental, use your own authenticated session, and show a ToS warning before enabling.

Every adapter degrades independently: one source breaking or blocking never takes down the others or the app.

**Valuation needs no AI.** Estimates come from median/IQR statistics over comparable sold listings. AI (M5) is an optional, bring-your-own-key refinement layer — model extraction from messy titles, scam red-flags — and the app is fully functional with zero AI configured. If no comps exist, a listing is marked "unpriceable"; Nexus never fabricates a value.

## Development

Prereqs: Rust (1.80+), Node 20+, and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev      # run the app
npm run tauri build    # package for your platform

cd src-tauri
cargo test             # Rust unit + integration tests (no network, fixture-based)
```

The SQLite database lives in your platform's app-data directory (e.g. `~/Library/Application Support/com.nexus.app/nexus.db` on macOS).

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md). Short version: React UI ⇄ Tauri commands ⇄ Rust core (adapter registry → dedup pipeline → SQLite; scheduler and valuation engine land in M2/M3).
