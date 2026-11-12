//! Page description component extraction (D-08, services layer).
//!
//! Port of `COMPONENT_MAP` / `component_map`, `extract_all_components`
//! and `get_entity_details` from
//! `apps/api/pi_dash/bgtasks/page_transaction_task.py:21-82`.
//! Fixture: `FX-PAGE-01`
//! (`rust-api/fixtures/tasks_webhooks/fx-page-01-page-transaction.json`).
//!
//! Translation notes:
//!
//! * `BeautifulSoup(description_html, "html.parser").find_all(component)`
//!   becomes a `scraper` fragment select over the same literal tag names.
//!   Both parsers lowercase ASCII tag/attribute names and treat an
//!   unterminated `<tag` (no `>`) as text, so the fixture's
//!   `malformed_html -> empty` vector replays.
//! * The per-tag entity dict keeps RAW attribute values in `attributes`
//!   order (Python insertion order); normalization into
//!   `entity_name` / `entity_type` / `entity_identifier` happens only in
//!   [`get_entity_details`], exactly like the `extract` lambdas. In
//!   particular the `entity_type` tag attribute is read by the extractor
//!   but DISCARDED by both lambdas (`entity_type` is always `None`).
//! * The broad `except Exception -> all-empty` maps to selector-build
//!   failure yielding empty sides; fragment parsing itself is infallible.

use scraper::{Html, Selector};
use serde::Serialize;

/// Tag name for issue/user/page mentions in page HTML.
pub const MENTION_COMPONENT: &str = "mention-component";

/// Tag name for embedded images in page HTML.
pub const IMAGE_COMPONENT: &str = "image-component";

/// Raw attributes collected per `<mention-component>` tag, in
/// `COMPONENT_MAP` order.
pub const MENTION_ATTRIBUTES: &[&str] = &["id", "entity_identifier", "entity_name", "entity_type"];

/// Raw attributes collected per `<image-component>` tag, in
/// `COMPONENT_MAP` order.
pub const IMAGE_ATTRIBUTES: &[&str] = &["id", "src"];

/// Raw attributes of one `<mention-component>` tag (`tag.get(attr)` per
/// attribute; missing attributes are `None`). Field order matches
/// `MENTION_ATTRIBUTES` so serialized JSON keeps the Python key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MentionAttrs {
    pub id: Option<String>,
    pub entity_identifier: Option<String>,
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
}

/// Raw attributes of one `<image-component>` tag. Field order matches
/// `IMAGE_ATTRIBUTES`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageAttrs {
    pub id: Option<String>,
    pub src: Option<String>,
}

/// `extract_all_components` result: component name -> extracted entities.
/// Field order matches `component_map` insertion order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentExtract {
    pub mention_component: Vec<MentionAttrs>,
    pub image_component: Vec<ImageAttrs>,
}

/// Normalized entity triple produced by `get_entity_details`.
/// Field order matches the `extract` lambda dicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityDetails {
    pub entity_name: Option<String>,
    pub entity_type: Option<String>,
    pub entity_identifier: Option<String>,
}

fn select_raw(html: &Html, component: &str, attributes: &[&str]) -> Vec<Vec<Option<String>>> {
    let selector = match Selector::parse(component) {
        Ok(selector) => selector,
        Err(_) => return Vec::new(),
    };
    html.select(&selector)
        .map(|element| {
            attributes
                .iter()
                .map(|attr| element.attr(attr).map(str::to_owned))
                .collect()
        })
        .collect()
}

/// `extract_all_components(description_html)`
/// (`page_transaction_task.py:45-71`): single pass over the HTML value.
/// `None` or empty input yields empty sides, mirroring
/// `if not description_html`.
pub fn extract_all_components(description_html: Option<&str>) -> ComponentExtract {
    match description_html {
        None => ComponentExtract {
            mention_component: Vec::new(),
            image_component: Vec::new(),
        },
        Some("") => ComponentExtract {
            mention_component: Vec::new(),
            image_component: Vec::new(),
        },
        Some(html) => {
            let fragment = Html::parse_fragment(html);
            ComponentExtract {
                mention_component: select_raw(&fragment, MENTION_COMPONENT, MENTION_ATTRIBUTES)
                    .into_iter()
                    .map(|attrs| MentionAttrs {
                        id: attrs[0].clone(),
                        entity_identifier: attrs[1].clone(),
                        entity_name: attrs[2].clone(),
                        entity_type: attrs[3].clone(),
                    })
                    .collect(),
                image_component: select_raw(&fragment, IMAGE_COMPONENT, IMAGE_ATTRIBUTES)
                    .into_iter()
                    .map(|attrs| ImageAttrs {
                        id: attrs[0].clone(),
                        src: attrs[1].clone(),
                    })
                    .collect(),
            }
        }
    }
}

/// `get_entity_details(component, mention)` (`:74-81`): normalizes a RAW
/// attribute dict (as produced by [`extract_all_components`], not the
/// normalized shape) via the component's `extract` lambda. Unknown
/// components yield all-`None` without raising.
pub fn get_entity_details<'a>(
    component: &str,
    attr: impl Fn(&str) -> Option<&'a str>,
) -> EntityDetails {
    match component {
        MENTION_COMPONENT => EntityDetails {
            entity_name: attr("entity_name").map(str::to_owned),
            // The lambda hard-codes None: the tag's entity_type attribute
            // is read by the extractor but discarded here.
            entity_type: None,
            entity_identifier: attr("entity_identifier").map(str::to_owned),
        },
        IMAGE_COMPONENT => EntityDetails {
            entity_name: Some("image".to_owned()),
            entity_type: None,
            entity_identifier: attr("src").map(str::to_owned),
        },
        _ => EntityDetails {
            entity_name: None,
            entity_type: None,
            entity_identifier: None,
        },
    }
}

/// [`get_entity_details`] over a [`MentionAttrs`] row.
pub fn mention_details(mention: &MentionAttrs) -> EntityDetails {
    get_entity_details(MENTION_COMPONENT, |key| match key {
        "id" => mention.id.as_deref(),
        "entity_identifier" => mention.entity_identifier.as_deref(),
        "entity_name" => mention.entity_name.as_deref(),
        "entity_type" => mention.entity_type.as_deref(),
        _ => None,
    })
}

/// [`get_entity_details`] over an [`ImageAttrs`] row.
pub fn image_details(image: &ImageAttrs) -> EntityDetails {
    get_entity_details(IMAGE_COMPONENT, |key| match key {
        "id" => image.id.as_deref(),
        "src" => image.src.as_deref(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // FX-PAGE-01 `extract_vectors_executed.sample`.
    const SAMPLE_HTML: &str = concat!(
        r#"<p>See <mention-component id="m1" entity_identifier="iid-1" "#,
        r#"entity_name="ISSUE" entity_type="x">ref</mention-component> "#,
        r#"and <image-component id="i9" src="https://cdn/x.png"></image-component></p>"#,
    );

    #[test]
    fn component_map_attributes_match_fixture() {
        assert_eq!(
            MENTION_ATTRIBUTES,
            &["id", "entity_identifier", "entity_name", "entity_type"]
        );
        assert_eq!(IMAGE_ATTRIBUTES, &["id", "src"]);
    }

    #[test]
    fn sample_html_matches_fixture_vectors() {
        let extracted = extract_all_components(Some(SAMPLE_HTML));
        assert_eq!(
            extracted.mention_component,
            vec![MentionAttrs {
                id: Some("m1".to_owned()),
                entity_identifier: Some("iid-1".to_owned()),
                entity_name: Some("ISSUE".to_owned()),
                // Raw tag attrs: entity_type is present here; only
                // get_entity_details discards it.
                entity_type: Some("x".to_owned()),
            }]
        );
        assert_eq!(
            extracted.image_component,
            vec![ImageAttrs {
                id: Some("i9".to_owned()),
                src: Some("https://cdn/x.png".to_owned()),
            }]
        );
    }

    #[test]
    fn empty_inputs_yield_empty_sides() {
        for html in [None, Some(""), Some("<p>plain</p>")] {
            let extracted = extract_all_components(html);
            assert!(extracted.mention_component.is_empty());
            assert!(extracted.image_component.is_empty());
        }
    }

    #[test]
    fn malformed_html_yields_empty_sides() {
        // An unterminated tag (no `>`) is text to both html.parser and
        // html5ever, so no component is found.
        let extracted = extract_all_components(Some("<mention-component id=\"m1\""));
        assert!(extracted.mention_component.is_empty());
        assert!(extracted.image_component.is_empty());
    }

    #[test]
    fn mention_without_id_keeps_null_id() {
        // FX-PAGE-01 `no_id_mention` vector.
        let extracted = extract_all_components(Some(
            r#"<mention-component entity_identifier="iid-9" entity_name="ISSUE">x</mention-component>"#,
        ));
        assert_eq!(
            extracted.mention_component,
            vec![MentionAttrs {
                id: None,
                entity_identifier: Some("iid-9".to_owned()),
                entity_name: Some("ISSUE".to_owned()),
                entity_type: None,
            }]
        );
    }

    #[test]
    fn get_entity_details_matches_fixture_vectors() {
        let mention = MentionAttrs {
            id: Some("m1".to_owned()),
            entity_identifier: Some("iid-1".to_owned()),
            entity_name: Some("ISSUE".to_owned()),
            entity_type: Some("x".to_owned()),
        };
        assert_eq!(
            mention_details(&mention),
            EntityDetails {
                entity_name: Some("ISSUE".to_owned()),
                entity_type: None,
                entity_identifier: Some("iid-1".to_owned()),
            }
        );

        let image = ImageAttrs {
            id: Some("i9".to_owned()),
            src: Some("https://cdn/x.png".to_owned()),
        };
        assert_eq!(
            image_details(&image),
            EntityDetails {
                entity_name: Some("image".to_owned()),
                entity_type: None,
                entity_identifier: Some("https://cdn/x.png".to_owned()),
            }
        );

        assert_eq!(
            get_entity_details("unknown-component", |_| Some("v")),
            EntityDetails {
                entity_name: None,
                entity_type: None,
                entity_identifier: None,
            }
        );
    }

    #[test]
    fn serialized_key_order_matches_python_dicts() {
        let details = EntityDetails {
            entity_name: Some("ISSUE".to_owned()),
            entity_type: None,
            entity_identifier: Some("iid-1".to_owned()),
        };
        let json = serde_json::to_string(&details).expect("serializes");
        assert_eq!(
            json,
            r#"{"entity_name":"ISSUE","entity_type":null,"entity_identifier":"iid-1"}"#
        );
    }
}
