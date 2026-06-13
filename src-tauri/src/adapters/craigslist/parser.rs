//! Pure HTML parsing for Craigslist pages. No network: tested entirely
//! against fixtures saved from real pages (fixtures/craigslist_*.html).
//!
//! Craigslist search pages are JS-rendered, but serve a static no-JS
//! fallback (`li.cl-static-search-result`) that carries url/title/price/
//! location. Everything else (description, coords, images, condition,
//! posted date) comes from the listing page.

use crate::models::{AdapterError, Condition};
use scraper::{Html, Selector};

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

/// One row from the search-results page static fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub price: Option<f64>,
    pub location: Option<String>,
}

/// Detail fields only available on the listing page itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListingDetail {
    pub description: Option<String>,
    pub posted_at: Option<String>, // RFC 3339
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub images: Vec<String>,
    pub condition: Option<Condition>,
}

fn parse_price(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    cleaned.parse().ok()
}

/// Posting id is the trailing number in the listing URL: .../7940078320.html
fn source_id_from_url(url: &str) -> Option<String> {
    let stem = url.strip_suffix(".html")?;
    let id = stem.rsplit('/').next()?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_string())
}

pub fn parse_search_page(html: &str) -> Result<Vec<SearchResult>, AdapterError> {
    let doc = Html::parse_document(html);
    let result_sel = sel("li.cl-static-search-result");
    let link_sel = sel("a");
    let title_sel = sel("div.title");
    let price_sel = sel("div.price");
    let location_sel = sel("div.location");

    let mut results = Vec::new();
    for item in doc.select(&result_sel) {
        let Some(link) = item.select(&link_sel).next() else { continue };
        let Some(url) = link.value().attr("href") else { continue };
        let Some(source_id) = source_id_from_url(url) else { continue };
        let Some(title) = item
            .select(&title_sel)
            .next()
            .map(|t| t.text().collect::<String>().trim().to_string())
        else {
            continue;
        };
        let price = item
            .select(&price_sel)
            .next()
            .and_then(|p| parse_price(&p.text().collect::<String>()));
        let location = item
            .select(&location_sel)
            .next()
            .map(|l| l.text().collect::<String>().trim().to_string())
            .filter(|l| !l.is_empty());
        results.push(SearchResult {
            source_id,
            url: url.to_string(),
            title,
            price,
            location,
        });
    }

    // A page with zero results AND none of the static-result markup likely
    // means a layout change or a block page — both should surface loudly
    // rather than read as "no matches".
    if results.is_empty() && !html.contains("cl-static-search-result") {
        return Err(AdapterError::Parse(
            "no cl-static-search-result markup found; page layout changed or request was blocked"
                .into(),
        ));
    }
    Ok(results)
}

pub fn parse_listing_page(html: &str) -> Result<ListingDetail, AdapterError> {
    let doc = Html::parse_document(html);
    let mut detail = ListingDetail::default();

    if let Some(body) = doc.select(&sel("section#postingbody")).next() {
        let text: String = body.text().collect();
        // The body embeds a print-only QR block; its label is the only text
        // it contributes, so strip it rather than walking child nodes.
        let cleaned = text.replace("QR Code Link to This Post", "");
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            detail.description = Some(trimmed.to_string());
        }
    }

    if let Some(time_el) = doc.select(&sel("time.date.timeago")).next() {
        if let Some(dt) = time_el.value().attr("datetime") {
            // CL uses -0700 style offsets; normalize to RFC 3339.
            detail.posted_at = chrono::DateTime::parse_from_str(dt, "%Y-%m-%dT%H:%M:%S%z")
                .ok()
                .map(|d| d.to_rfc3339());
        }
    }

    if let Some(map_el) = doc.select(&sel("#map")).next() {
        detail.lat = map_el.value().attr("data-latitude").and_then(|v| v.parse().ok());
        detail.lng = map_el.value().attr("data-longitude").and_then(|v| v.parse().ok());
    }

    // Full-size image URLs live in an inline `var imgList = [...]` script.
    if let Some(start) = html.find("imgList = ") {
        let json_start = start + "imgList = ".len();
        if let Some(end) = html[json_start..].find("];") {
            let json = &html[json_start..json_start + end + 1];
            #[derive(serde::Deserialize)]
            struct Img {
                url: String,
            }
            if let Ok(imgs) = serde_json::from_str::<Vec<Img>>(json) {
                detail.images = imgs.into_iter().map(|i| i.url).collect();
            }
        }
    }
    if detail.images.is_empty() {
        if let Some(og) = doc.select(&sel(r#"meta[property="og:image"]"#)).next() {
            if let Some(url) = og.value().attr("content") {
                detail.images.push(url.to_string());
            }
        }
    }

    if let Some(cond) = doc.select(&sel("div.attr.condition span.valu")).next() {
        let text = cond.text().collect::<String>().trim().to_lowercase();
        detail.condition = match text.as_str() {
            "new" => Some(Condition::New),
            "like new" => Some(Condition::LikeNew),
            "excellent" | "good" => Some(Condition::Good),
            "fair" | "salvage" => Some(Condition::Fair),
            "for parts" | "parts" => Some(Condition::Parts),
            _ => None,
        };
    }

    Ok(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_HTML: &str = include_str!("../../../fixtures/craigslist_search.html");
    const LISTING_HTML: &str = include_str!("../../../fixtures/craigslist_listing.html");

    #[test]
    fn search_page_yields_all_static_results() {
        let results = parse_search_page(SEARCH_HTML).unwrap();
        assert_eq!(results.len(), 56);
    }

    #[test]
    fn search_results_have_expected_shape() {
        let results = parse_search_page(SEARCH_HTML).unwrap();
        let first = &results[0];
        assert_eq!(first.source_id, "7940078320");
        assert_eq!(first.title, r#"Like New 3/8" Dewalt Variable Speed Drill"#);
        assert_eq!(first.price, Some(50.0));
        assert_eq!(first.location.as_deref(), Some("Enumclaw"));
        assert!(first.url.starts_with("https://seattle.craigslist.org/"));
        // Every result must carry id, url, title; price/location may be absent.
        for r in &results {
            assert!(!r.source_id.is_empty());
            assert!(r.url.ends_with(".html"));
            assert!(!r.title.is_empty());
        }
    }

    #[test]
    fn unrecognizable_page_is_an_error_not_empty() {
        let err = parse_search_page("<html><body>blocked</body></html>").unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    #[test]
    fn listing_page_parses_all_detail_fields() {
        let d = parse_listing_page(LISTING_HTML).unwrap();
        assert!(d.description.as_deref().unwrap().contains("Like New with handy travel bag"));
        assert!(!d.description.as_deref().unwrap().contains("QR Code"));
        assert_eq!(d.posted_at.as_deref(), Some("2026-06-10T10:38:53-07:00"));
        assert_eq!(d.lat, Some(47.201512));
        assert_eq!(d.lng, Some(-121.988019));
        assert!(!d.images.is_empty());
        assert!(d.images[0].starts_with("https://images.craigslist.org/"));
        assert_eq!(d.condition, Some(Condition::LikeNew));
    }

    #[test]
    fn price_parser_handles_commas_and_garbage() {
        assert_eq!(parse_price("$1,200"), Some(1200.0));
        assert_eq!(parse_price("$50"), Some(50.0));
        assert_eq!(parse_price("free"), None);
    }

    #[test]
    fn source_id_rejects_non_numeric_stems() {
        assert_eq!(source_id_from_url("https://x.org/d/foo/123.html").as_deref(), Some("123"));
        assert_eq!(source_id_from_url("https://x.org/about.html"), None);
        assert_eq!(source_id_from_url("https://x.org/123"), None);
    }
}
