# Zinger

A local-first desktop app that watches saved second-hand searches across
Craigslist, eBay and (opt-in) Facebook Marketplace, folds the cross-posts
together, and keeps the whole thing in a SQLite file on your own machine.
Tauri 2, Rust core, React UI.

No accounts, no backend, no telemetry. API keys go to the OS keychain.

## Why it exists

Hunting for underpriced second-hand gear means running the same three or four
searches on the same three or four sites, over and over, and mentally
deduplicating the seller who posted the same drill to all of them. That is a
polling loop, which is a thing computers are good at. Every hosted tool that
does it wants an account and keeps your search history; this one is a desktop
binary with a SQLite file next to it.

The interesting part turned out not to be the polling. It was that the three
sources have completely different access stories — eBay has a documented API,
Craigslist has no API and has sued scrapers, Facebook has no API and actively
fights automation — and the app has to keep working when any one of them
breaks, blocks, or is switched off.

## How it works

Every source implements one trait: `search(spec, ctx) -> Result<Vec<RawListing>>`,
plus `health()` and `rate_limit_policy()`. Adapters do nothing but fetch and
parse — they never see the database and never decide policy.

Everything else lives in one layer above them:

- **`poll.rs` owns failure.** It runs each adapter in isolation, so one source
  returning a `Parse` error is a row in the run report, not an aborted cycle.
  Consecutive failures drive exponential backoff (5 min base, doubling, capped
  at 24 h) recorded in an `adapter_state` table; three in a row marks the source
  disabled until the window lapses. An adapter with no credentials is skipped
  before any of this — silence, not an error.
- **Dedup is a perceptual image hash first.** The poll layer downloads each new
  listing's first image (10 s timeout, 1 MB cap), computes an 8x8 gradient hash,
  and ingest treats a hamming distance of 6 or less as the same photo, so a
  cross-post dedups even when the two prices differ. The original scheme —
  `sha256(normalized title | price rounded to $5)` — is a step function with a
  hole at the bucket boundary ($97 and $98 land in different buckets and do not
  dedup, while $98 and $102 land in the same one and do). It survives as the
  fallback for listings with no images.
- **`scheduler.rs` ticks once a minute**, running any search whose interval
  (per-search column, else the global setting) has elapsed, ±20% jitter so the
  traffic isn't a metronome. Due times are in memory only: a restart re-polls
  early at worst, and ingest dedups the overlap.
- **Craigslist parses the no-JS fallback.** The search pages are JS-rendered but
  ship a server-side `li.cl-static-search-result` list carrying url, title,
  price and location. One request per search, then detail pages only for
  listings not already stored, capped at 8 per cycle with randomised 5-15 s
  delays. A page with neither results nor the fallback markup is a loud error,
  never a silent "no matches".
- **Facebook runs in a separate process.** A Node + Playwright sidecar drives a
  persistent Chromium profile and pipes rendered HTML back over newline-
  delimited JSON; parsing stays in Rust against a fixture like every other
  adapter. The sidecar is spawned only while the adapter is enabled and killed
  otherwise, so nothing browser-shaped loads when Facebook is off.
- **SQLite via rusqlite + refinery**, one connection behind a mutex. Adapter
  I/O always completes before the lock is taken, so a slow source never blocks
  the UI on it.

Longer version, including the decisions that were rejected and why, is in
[ARCHITECTURE.md](ARCHITECTURE.md).

## Marketplace access — read this before enabling anything

The three sources are not equivalent, and the difference is legal, not
technical.

- **eBay** has an official API. Zinger uses the Browse API with your own
  developer keys, stored in the OS keychain.
- **Craigslist** has no API and has litigated against scrapers. The adapter
  parses public search pages with randomised delays, an honest user agent that
  identifies the tool rather than impersonating a browser, and automatic
  backoff. Using it may still breach Craigslist's terms.
- **Facebook Marketplace has no public API, and automated access to it breaches
  Facebook's Terms of Service.** The adapter is off by default and behind a
  one-time acceptance gate. It drives your own account, which you log into
  yourself in a visible browser window — Zinger never sees or stores your
  password, and the session lives only in a private browser profile on your
  machine. It waits a randomised 20-45 s between page loads, caps pages per
  cycle at 3, stops immediately on a challenge, and self-disables for 24 hours
  after two consecutive ones. None of that removes the risk that Facebook
  detects the automation and restricts or bans the account you are using.
  Enabling it is your call and your risk.
- **OfferUp** prohibits scraping and has no public API. Not implemented; only
  the adapter interface exists.

## Running it

Needs Rust 1.80+, Node 20+, and the
[Tauri 2 system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run tauri dev      # run it
npm run tauri build    # package for your platform

cd src-tauri && cargo test   # 44 offline tests; 1 live smoke test is #[ignore]d
```

The database goes in the platform app-data directory —
`~/Library/Application Support/com.zinger.app/zinger.db` on macOS.

eBay is optional but is the only source with a supported API: create a free
account at developer.ebay.com, make an app with production keys, then paste the
App ID and Cert ID into Settings and hit Test connection. They go to the
keychain, never the database and never the logs.

The Facebook sidecar has its own `npm install` in `sidecar/`, and only matters
if you enable Facebook. Chromium (~341 MB) is downloaded on first enable rather
than bundled, so the base app stays small for the people who never touch it.

## Status

Working: live Craigslist and eBay adapters, the poll layer with per-adapter
backoff, the background scheduler, phash dedup, saved searches, the deals feed
with save/hide, settings, and versioned SQLite migrations. `cargo test` runs 44
tests and passes; none of them touch the network, because the parsers are tested
against HTML and JSON fixtures captured from the real sites. The one test that
would hit Craigslist for real is marked `#[ignore]`.

Not working yet:

- **There is no valuation.** The whole point — rank listings by expected profit
  against sold comps — is not built. The UI shows `score —` rather than a made-up
  number, and the schema already treats `valuations.score = NULL` as
  "unpriceable" rather than zero. The `comps` and `valuations` tables exist and
  are empty.
- **The Facebook live search path is experimental.** The parser is anchored on
  `/marketplace/item/` links and currency text against a saved fixture, but
  Facebook's markup is obfuscated and changes often, so it can start returning
  fewer results or none without warning. The selectors have not been revalidated against a
  real logged-in snapshot, and the one manual live smoke test needs a Facebook
  account, so it has not been run.
- The Craigslist site slug is derived from the search location text, which is
  right for most metros and wrong for the renamed ones (the Bay Area is
  `sfbay`); there is a settings override for those.
- No notifications, no AI anything. Valuation, when it lands, is median/IQR over
  comparable sold listings — statistics, not a model.

## License

MIT — see [LICENSE](LICENSE).
