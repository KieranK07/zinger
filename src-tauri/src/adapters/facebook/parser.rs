//! Pure parsing of a Facebook Marketplace search-results DOM. No network:
//! tested against fixtures/facebook_search.html (authored from observed
//! structure — see that file's header).
//!
//! FB randomizes class names every build, so we anchor on the only two stable
//! signals: the `/marketplace/item/<id>/` href that marks a listing card, and
//! visible price text starting with a currency symbol. Title and location are
//! positional heuristics within the card. This is the most fragile parser in
//! the project by far; treat live output as experimental until the selectors
//! are revalidated against a real logged-in snapshot.

use crate::models::{AdapterError, RawListing};
use scraper::{Html, Selector};

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

/// "/marketplace/item/1122334455/?ref=search" -> "1122334455"
fn item_id_from_href(href: &str) -> Option<String> {
    let after = href.split("/marketplace/item/").nth(1)?;
    let id: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    (!id.is_empty()).then_some(id)
}

/// First text that looks like a currency amount -> numeric value.
/// "Free", "$1,200", "$65". Returns None for non-priced text so the caller
/// can still surface the listing as price 0 / unpriceable.
fn parse_price(texts: &[String]) -> Option<f64> {
    for t in texts {
        let trimmed = t.trim();
        if trimmed.starts_with('$') || trimmed.starts_with('£') || trimmed.starts_with('€') {
            let digits: String = trimmed
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = digits.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

/// Parse a Marketplace results page into normalized listings. An empty result
/// on a page that has no item anchors at all is reported as a parse error —
/// the caller distinguishes "logged in but layout changed" from a genuinely
/// empty search via the sidecar's not_logged_in/challenge signals before this
/// is ever called.
pub fn parse_search_page(html: &str) -> Result<Vec<RawListing>, AdapterError> {
    let doc = Html::parse_document(html);
    let link_sel = sel(r#"a[href*="/marketplace/item/"]"#);
    let span_sel = sel("span");
    let img_sel = sel("img");

    let mut listings = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for card in doc.select(&link_sel) {
        let Some(href) = card.value().attr("href") else { continue };
        let Some(id) = item_id_from_href(href) else { continue };
        // The same item can appear behind multiple anchors; keep the first.
        if !seen.insert(id.clone()) {
            continue;
        }

        let texts: Vec<String> = card
            .select(&span_sel)
            .map(|s| s.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();

        let price = parse_price(&texts).unwrap_or(0.0);

        // Non-price spans, in document order: [title, location, ...]. Title is
        // the first; location is the next that looks like a place ("City, ST").
        let non_price: Vec<&String> = texts
            .iter()
            .filter(|t| parse_price(std::slice::from_ref(*t)).is_none() && t.as_str() != "Free")
            .collect();
        let Some(title) = non_price.first().map(|s| s.to_string()) else { continue };
        let location = non_price
            .iter()
            .skip(1)
            .find(|t| t.contains(", "))
            .map(|s| s.to_string());

        let image = card
            .select(&img_sel)
            .next()
            .and_then(|i| i.value().attr("src"))
            .map(String::from);

        listings.push(RawListing {
            source: "facebook".into(),
            source_id: id.clone(),
            source_url: format!("https://www.facebook.com/marketplace/item/{id}/"),
            title,
            description: None,
            price,
            currency: "USD".into(),
            location_text: location,
            lat: None,
            lng: None,
            images: image.into_iter().collect(),
            posted_at: None,
            condition: None,
            phash: None,
        });
    }

    if listings.is_empty() {
        return Err(AdapterError::Parse(
            "no /marketplace/item/ cards found; layout changed or page was not results".into(),
        ));
    }
    Ok(listings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../fixtures/facebook_search.html");

    #[test]
    fn parses_all_item_cards_ignoring_noise() {
        let listings = parse_search_page(FIXTURE).unwrap();
        // 4 item cards; the Marketplace/Selling/category/see-more links are noise.
        assert_eq!(listings.len(), 4);
    }

    #[test]
    fn maps_fields_from_first_card() {
        let listings = parse_search_page(FIXTURE).unwrap();
        let first = &listings[0];
        assert_eq!(first.source, "facebook");
        assert_eq!(first.source_id, "1122334455");
        assert_eq!(first.source_url, "https://www.facebook.com/marketplace/item/1122334455/");
        assert_eq!(first.price, 120.0);
        assert_eq!(first.title, "Vintage Pioneer SX-780 Receiver");
        assert_eq!(first.location_text.as_deref(), Some("Seattle, WA"));
        assert_eq!(first.images.len(), 1);
        assert!(first.images[0].contains("receiver"));
    }

    #[test]
    fn parses_thousands_separator_price() {
        let listings = parse_search_page(FIXTURE).unwrap();
        let chair = listings.iter().find(|l| l.source_id == "3344556677").unwrap();
        assert_eq!(chair.price, 1200.0);
    }

    #[test]
    fn free_item_is_priced_zero_with_title_preserved() {
        let listings = parse_search_page(FIXTURE).unwrap();
        let sofa = listings.iter().find(|l| l.source_id == "4455667788").unwrap();
        assert_eq!(sofa.price, 0.0);
        assert_eq!(sofa.title, "Used sectional sofa - must pick up");
        assert_eq!(sofa.location_text.as_deref(), Some("Tacoma, WA"));
    }

    #[test]
    fn item_id_extraction() {
        assert_eq!(item_id_from_href("/marketplace/item/999/?ref=x").as_deref(), Some("999"));
        assert_eq!(item_id_from_href("/marketplace/item/123/").as_deref(), Some("123"));
        assert_eq!(item_id_from_href("/marketplace/category/x/"), None);
    }

    #[test]
    fn empty_or_unrecognizable_page_is_error() {
        assert!(matches!(
            parse_search_page("<html><body>blocked</body></html>"),
            Err(AdapterError::Parse(_))
        ));
    }
}
