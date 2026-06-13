//! Browse API item_summary/search response -> RawListing. Pure JSON mapping,
//! tested against fixtures/ebay_item_summaries.json (authored to the
//! documented Browse schema — real captures require user API keys).

use crate::models::{AdapterError, Condition, RawListing};
use serde::Deserialize;

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default, rename = "itemSummaries")]
    item_summaries: Vec<ItemSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemSummary {
    item_id: String,
    title: String,
    price: Option<Price>,
    item_web_url: String,
    image: Option<Image>,
    #[serde(default)]
    additional_images: Vec<Image>,
    condition_id: Option<String>,
    item_location: Option<ItemLocation>,
    item_creation_date: Option<String>,
    #[serde(default)]
    categories: Vec<Category>,
}

#[derive(Deserialize)]
struct Price {
    value: String,
    currency: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Image {
    image_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemLocation {
    city: Option<String>,
    state_or_province: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Category {
    category_name: Option<String>,
}

/// eBay condition IDs, per their condition-ID table: 1000s = new-ish,
/// 2000s/2500 = refurbished, 3000-5000 = used grades, 7000 = parts.
fn condition_from_id(id: &str) -> Option<Condition> {
    match id.parse::<u32>().ok()? {
        1000..=1999 => Some(Condition::New),
        2000..=2999 => Some(Condition::LikeNew),
        3000..=3999 => Some(Condition::Good),
        4000..=6999 => Some(Condition::Fair),
        7000..=7999 => Some(Condition::Parts),
        _ => None,
    }
}

pub fn parse_item_summaries(json: &str) -> Result<Vec<RawListing>, AdapterError> {
    let resp: SearchResponse =
        serde_json::from_str(json).map_err(|e| AdapterError::Parse(format!("browse JSON: {e}")))?;

    let listings = resp
        .item_summaries
        .into_iter()
        .filter_map(|item| {
            // Listings without a parseable price can't be ranked or
            // compared; skip rather than store price = 0.
            let price = item.price.as_ref()?;
            let amount: f64 = price.value.parse().ok()?;
            let mut images: Vec<String> =
                item.image.into_iter().map(|i| i.image_url).collect();
            images.extend(item.additional_images.into_iter().map(|i| i.image_url));
            let location_text = item.item_location.and_then(|l| match (l.city, l.state_or_province) {
                (Some(c), Some(s)) => Some(format!("{c}, {s}")),
                (Some(c), None) => Some(c),
                (None, Some(s)) => Some(s),
                (None, None) => None,
            });
            Some(RawListing {
                source: "ebay".into(),
                source_id: item.item_id,
                source_url: item.item_web_url,
                title: item.title,
                description: item
                    .categories
                    .first()
                    .and_then(|c| c.category_name.clone()),
                price: amount,
                currency: price.currency.clone(),
                location_text,
                lat: None,
                lng: None,
                images,
                posted_at: item.item_creation_date,
                condition: item.condition_id.as_deref().and_then(condition_from_id),
                phash: None,
            })
        })
        .collect();
    Ok(listings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../fixtures/ebay_item_summaries.json");

    #[test]
    fn parses_all_summaries() {
        let listings = parse_item_summaries(FIXTURE).unwrap();
        assert_eq!(listings.len(), 3);
    }

    #[test]
    fn maps_fields_correctly() {
        let listings = parse_item_summaries(FIXTURE).unwrap();
        let first = &listings[0];
        assert_eq!(first.source, "ebay");
        assert_eq!(first.source_id, "v1|110554038305|0");
        assert_eq!(first.price, 89.99);
        assert_eq!(first.currency, "USD");
        assert_eq!(first.source_url, "https://www.ebay.com/itm/110554038305");
        assert_eq!(first.location_text.as_deref(), Some("Kent, WA"));
        assert_eq!(first.condition, Some(Condition::Good)); // 3000 = Used
        assert_eq!(first.images.len(), 2); // primary + 1 additional
        assert_eq!(first.posted_at.as_deref(), Some("2026-06-09T18:24:11.000Z"));
    }

    #[test]
    fn condition_id_mapping() {
        let listings = parse_item_summaries(FIXTURE).unwrap();
        assert_eq!(listings[1].condition, Some(Condition::New)); // 1000
        assert_eq!(listings[2].condition, Some(Condition::Parts)); // 7000
    }

    #[test]
    fn item_without_image_or_location_still_parses() {
        let listings = parse_item_summaries(FIXTURE).unwrap();
        let parts = &listings[2];
        assert!(parts.images.is_empty());
        assert_eq!(parts.location_text, None); // only postal code in fixture
        assert_eq!(parts.posted_at, None);
    }

    #[test]
    fn empty_response_is_ok_empty() {
        assert_eq!(parse_item_summaries("{}").unwrap().len(), 0);
    }

    #[test]
    fn malformed_json_is_parse_error() {
        assert!(matches!(
            parse_item_summaries("not json"),
            Err(AdapterError::Parse(_))
        ));
    }
}
