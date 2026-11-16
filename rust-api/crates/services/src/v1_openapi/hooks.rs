#![forbid(unsafe_code)]

//! D-23 schema hooks: endpoint filter, operation summaries, dual-form params (stage 5).
//!
//! Port of the three functions in `apps/api/pi_dash/utils/openapi/hooks.py:14-94`:
//!
//! * [`preprocess_filter_api_v1_paths`] (`:14-23`) — keep iff the path starts
//!   with `/api/v1/`, the method is not `PUT`, and the path has no `server`
//!   substring. The substring clause is over-broad by construction (`observer`
//!   contains `server`); ported verbatim.
//! * [`generate_operation_summary`] (`:26-56`) — method+path+tag summary with
//!   archive/transfer special cases. NOTE: dead in Python (defined and
//!   re-exported, zero call sites); ported verbatim as a pure function.
//! * [`postprocess_project_id_dual_form`] (`:59-94`) — rewrites `{project_id}`
//!   and `/projects/{pk}/` path params to the dual-form description plus
//!   uuid/slug examples, schema `{type: string}` with `format` removed.
//!
//! Fixture: `rust-api/fixtures/v1_openapi/FX-OPENAPI-02.hooks.json`
//! (`FX-OPENAPI-02`); the tests replay every vector in it.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

/// One drf-spectacular endpoint tuple `(path, path_regex, method, callback)`
/// (`hooks.py:19`). The callback is opaque to the filter, so it stays generic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint<C> {
    pub path: String,
    pub path_regex: String,
    pub method: String,
    pub callback: C,
}

/// Port of `preprocess_filter_api_v1_paths` (`hooks.py:14-23`).
/// Survivors pass through identical, order preserved.
pub fn preprocess_filter_api_v1_paths<C>(endpoints: Vec<Endpoint<C>>) -> Vec<Endpoint<C>> {
    endpoints
        .into_iter()
        .filter(|endpoint| endpoint_kept(&endpoint.path, &endpoint.method))
        .collect()
}

/// The `if` condition of `preprocess_filter_api_v1_paths` (`hooks.py:21`):
/// keep iff the path starts with `/api/v1/` AND the method is not `PUT`
/// (case-insensitive) AND the path contains no `server` (case-insensitive).
pub fn endpoint_kept(path: &str, method: &str) -> bool {
    path.starts_with("/api/v1/")
        && !method.eq_ignore_ascii_case("put")
        && !contains_case_insensitive_ascii(path, "server")
}

/// Case-insensitive substring search over ASCII bytes. Equivalent to Python
/// `needle in haystack.lower()` for these needles: `str.lower()` maps no
/// non-ASCII char onto them (verified: `"ſerver".lower()` has no `"server"`,
/// where Rust `to_lowercase` would wrongly match).
fn contains_case_insensitive_ascii(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    haystack.len() >= needle.len()
        && (0..=haystack.len() - needle.len()).any(|i| {
            haystack[i..i + needle.len()]
                .iter()
                .zip(needle.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
        })
}

/// Port of `generate_operation_summary` (`hooks.py:26-56`).
/// Dead in Python (zero call sites); ported verbatim as a pure function.
pub fn generate_operation_summary(method: &str, path: &str, tag: &str) -> String {
    let resource = path
        .split('/')
        .rfind(|part| !part.is_empty() && !part.starts_with('{'))
        .map(|last| py_title(&last.replace('-', " ")))
        .unwrap_or_else(|| tag.to_string());

    if contains_case_insensitive_ascii(path, "archive") {
        if method == "POST" {
            return format!("Archive {}", tag.trim_end_matches('s'));
        } else if method == "DELETE" {
            return format!("Unarchive {}", tag.trim_end_matches('s'));
        }
    }

    if contains_case_insensitive_ascii(path, "transfer") {
        return format!("Transfer {}", tag.trim_end_matches('s'));
    }

    match method {
        "GET" => format!("Retrieve {resource}"),
        "POST" => format!("Create {resource}"),
        "PATCH" => format!("Update {resource}"),
        "DELETE" => format!("Delete {resource}"),
        _ => format!("{method} {resource}"),
    }
}

/// Port of Python `str.title()`: word-start chars go to titlecase, the rest
/// lower; word boundaries are non-cased chars. Lowercase word-starts take the
/// full uppercase expansion with tail-lower (`"ß".title() == "Ss"`, `"ﬁsh"`
/// → `"Fish"`); the 8 Latin digraphs map to titlecase (`Ǆ`/`ǆ` → `ǅ`);
/// anything else cased is already titlecase and passes through. Limit: Greek
/// vowels with ypogegrammeni (e.g. U+1FB2, multi-char titlecase) need the
/// Unicode SpecialCasing table std does not ship; those stay approximate.
/// Edge pins below were verified against CPython 3.9.
fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for ch in s.chars() {
        if prev_cased {
            out.extend(ch.to_lowercase());
        } else if let Some(title) = digraph_titlecase(ch) {
            out.push(title);
        } else if ch.is_lowercase() {
            let mut upper = ch.to_uppercase();
            if let Some(head) = upper.next() {
                out.push(head);
                for tail in upper {
                    out.extend(tail.to_lowercase());
                }
            }
        } else {
            out.push(ch);
        }
        prev_cased = is_cased(ch);
    }
    out
}

/// Titlecase for the Latin digraphs whose title differs from uppercase
/// (`Ǆ`→`ǅ`, `ǆ`→`ǅ`, …; verified against CPython 3.9).
fn digraph_titlecase(ch: char) -> Option<char> {
    match ch {
        '\u{1C4}' | '\u{1C6}' => Some('\u{1C5}'),
        '\u{1C7}' | '\u{1C9}' => Some('\u{1C8}'),
        '\u{1CA}' | '\u{1CC}' => Some('\u{1CB}'),
        '\u{1F1}' | '\u{1F3}' => Some('\u{1F2}'),
        _ => None,
    }
}

/// One-char `str.iscased()`: Unicode Lu/Ll plus the 31 Lt code points.
fn is_cased(ch: char) -> bool {
    ch.is_uppercase()
        || ch.is_lowercase()
        || matches!(
            ch,
            '\u{1C5}' | '\u{1C8}' | '\u{1CB}' | '\u{1F2}' | '\u{1F88}'..='\u{1F8F}' | '\u{1F98}'..='\u{1F9F}' | '\u{1FA8}'..='\u{1FAF}'
        )
}

/// Dual-form parameter description (`hooks.py:70-73`), verbatim.
pub const DUAL_FORM_DESCRIPTION: &str = "Either the project's UUID or its workspace-scoped identifier (e.g. ``ENG``). Identifier matching is case-insensitive.";

/// Dual-form uuid example value (`hooks.py:91`), verbatim.
pub const DUAL_FORM_UUID_VALUE: &str = "00000000-0000-0000-0000-000000000000";

/// Dual-form uuid example summary (`hooks.py:91`), verbatim.
pub const DUAL_FORM_UUID_SUMMARY: &str = "UUID form";

/// Dual-form slug example value (`hooks.py:92`), verbatim.
pub const DUAL_FORM_SLUG_VALUE: &str = "ENG";

/// Dual-form slug example summary (`hooks.py:92`), verbatim.
pub const DUAL_FORM_SLUG_SUMMARY: &str = "Workspace-scoped identifier";

/// Port of `postprocess_project_id_dual_form` (`hooks.py:59-94`).
/// Mutates `doc` in place (Python returns the same object) and drops the
/// `generator`/`request`/`public` arguments, which the implementation ignores.
/// Non-object `paths`/operations/params are skipped (Python would raise;
/// no fixture covers them).
pub fn postprocess_project_id_dual_form(doc: &mut serde_json::Value) {
    let Some(paths) = doc
        .get_mut("paths")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for (path, path_item) in paths.iter_mut() {
        if !path.contains("{project_id}") && !path.contains("/projects/{pk}/") {
            continue;
        }
        let Some(operations) = path_item.as_object_mut() else {
            continue;
        };
        for operation in operations.values_mut() {
            let Some(operation) = operation.as_object_mut() else {
                continue;
            };
            let Some(params) = operation
                .get_mut("parameters")
                .and_then(serde_json::Value::as_array_mut)
            else {
                continue;
            };
            for param in params.iter_mut() {
                let Some(param) = param.as_object_mut() else {
                    continue;
                };
                if param.get("in").and_then(serde_json::Value::as_str) != Some("path") {
                    continue;
                }
                let name = param.get("name").and_then(serde_json::Value::as_str);
                let is_project_id = name == Some("project_id");
                let is_pk = name == Some("pk") && path.contains("/projects/{pk}/");
                if !is_project_id && !is_pk {
                    continue;
                }
                param.insert(
                    "description".to_string(),
                    serde_json::Value::String(DUAL_FORM_DESCRIPTION.to_string()),
                );
                if !matches!(param.get("schema"), Some(schema) if schema.is_object()) {
                    param.insert("schema".to_string(), serde_json::json!({}));
                }
                if let Some(schema) = param
                    .get_mut("schema")
                    .and_then(serde_json::Value::as_object_mut)
                {
                    schema.remove("format");
                    schema.insert(
                        "type".to_string(),
                        serde_json::Value::String("string".to_string()),
                    );
                }
                param.insert(
                    "examples".to_string(),
                    serde_json::json!({
                        "uuid": {"value": DUAL_FORM_UUID_VALUE, "summary": DUAL_FORM_UUID_SUMMARY},
                        "slug": {"value": DUAL_FORM_SLUG_VALUE, "summary": DUAL_FORM_SLUG_SUMMARY},
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURE: &str = include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-02.hooks.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn preprocess_vectors() {
        let vectors = fixture()["preprocess_filter_api_v1_paths"]["vectors"].clone();
        let vectors = vectors.as_array().unwrap();
        assert_eq!(vectors.len(), 19);
        for vector in vectors {
            let path = vector["path"].as_str().unwrap();
            let method = vector["method"].as_str().unwrap();
            assert_eq!(
                endpoint_kept(path, method),
                vector["kept"].as_bool().unwrap(),
                "{}",
                vector["reason"].as_str().unwrap(),
            );
        }
    }

    #[test]
    fn preprocess_batch_preserves_order_and_identity() {
        let vectors = fixture()["preprocess_filter_api_v1_paths"]["vectors"].clone();
        let vectors = vectors.as_array().unwrap().clone();
        let endpoints: Vec<Endpoint<&str>> = vectors
            .iter()
            .map(|vector| Endpoint {
                path: vector["path"].as_str().unwrap().to_string(),
                path_regex: format!("^{}$", vector["path"].as_str().unwrap()),
                method: vector["method"].as_str().unwrap().to_string(),
                callback: "cb",
            })
            .collect();
        let survivors = preprocess_filter_api_v1_paths(endpoints.clone());
        let kept: Vec<Endpoint<&str>> = endpoints
            .into_iter()
            .zip(vectors.iter())
            .filter(|(_, vector)| vector["kept"].as_bool().unwrap())
            .map(|(endpoint, _)| endpoint)
            .collect();
        assert_eq!(survivors, kept);
        assert!(!survivors.is_empty());
    }

    #[test]
    fn summary_vectors() {
        let vectors = fixture()["generate_operation_summary"]["vectors"].clone();
        let vectors = vectors.as_array().unwrap();
        assert_eq!(vectors.len(), 18);
        for vector in vectors {
            assert_eq!(
                generate_operation_summary(
                    vector["method"].as_str().unwrap(),
                    vector["path"].as_str().unwrap(),
                    vector["tag"].as_str().unwrap(),
                ),
                vector["summary"].as_str().unwrap(),
                "{}",
                vector["reason"].as_str().unwrap(),
            );
        }
    }

    #[test]
    fn summary_python_title_edges() {
        // Verified against CPython 3.9 `str.title()`.
        assert_eq!(py_title("ß"), "Ss");
        assert_eq!(py_title("ſ"), "S");
        assert_eq!(py_title("ﬁsh"), "Fish");
        assert_eq!(py_title("ςpot"), "Σpot");
        assert_eq!(py_title("ǅa"), "ǅa");
        assert_eq!(py_title("Ǆa"), "ǅa");
        assert_eq!(py_title("ǆ"), "ǅ");
        assert_eq!(py_title("ΣAT"), "Σat");
        assert_eq!(py_title("aǅ"), "Aǆ");
        assert_eq!(py_title("they're"), "They'Re");
        assert_eq!(py_title("7EIGHTS bar"), "7Eights Bar");
        assert_eq!(py_title("ARCHIVE"), "Archive");
    }

    #[test]
    fn postprocess_before_after() {
        let fx = fixture()["postprocess_project_id_dual_form"].clone();
        let mut before = fx["before"].clone();
        postprocess_project_id_dual_form(&mut before);
        assert_eq!(before, fx["after"].clone());
    }

    #[test]
    fn postprocess_description_and_examples_verbatim() {
        let fx = fixture()["postprocess_project_id_dual_form"].clone();
        assert_eq!(
            DUAL_FORM_DESCRIPTION,
            fx["description_verbatim"].as_str().unwrap()
        );
        assert_eq!(
            DUAL_FORM_UUID_VALUE,
            fx["examples_verbatim"]["uuid"]["value"].as_str().unwrap()
        );
        assert_eq!(
            DUAL_FORM_UUID_SUMMARY,
            fx["examples_verbatim"]["uuid"]["summary"].as_str().unwrap()
        );
        assert_eq!(
            DUAL_FORM_SLUG_VALUE,
            fx["examples_verbatim"]["slug"]["value"].as_str().unwrap()
        );
        assert_eq!(
            DUAL_FORM_SLUG_SUMMARY,
            fx["examples_verbatim"]["slug"]["summary"].as_str().unwrap()
        );
    }
}
