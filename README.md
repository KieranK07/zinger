# Nexus

A local-first desktop app for second-hand marketplace arbitrage: find underpriced items near you, estimate resale value from comparable sold prices, and rank listings by expected profit.

**Personal tool, not a service.** No accounts, no backend, no telemetry. Your data and API keys never leave your machine.

## Status: M1 (skeleton)

Working now:
- Tauri 2 desktop app with SQLite storage and migrations
- Saved searches (keyword, location, radius, price ceiling)
- Pluggable marketplace adapter interface with a fixture-backed mock adapter
- Cross-post deduplication (normalized title + price bucket hashing)
- Deals feed, search manager, and settings views; save/hide listing actions
- One example search pre-seeded

Coming next (M2): live Craigslist and eBay adapters, background polling with backoff.

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
