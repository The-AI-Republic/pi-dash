//! `response_headers::mime_encode` parity with CPython's `email.header`.
//!
//! Every vector in `data/headers_corpus.json` was recorded from
//! `email.header.Header(s, "utf-8",
//! maxlinelen=sys.maxsize).encode()` (CPython 3.12.3); the corpus pins
//! the Q-vs-B `SHORTEST` choice, the Q safe set, and long single-word
//! inputs with no folding.

use pidash_api::response_headers::mime_encode;

#[test]
fn mime_encoding_matches_cpython_corpus() {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("data/headers_corpus.json")).expect("corpus parses");
    let cases = doc["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "corpus must not be empty");
    for case in cases {
        let input = case["in"].as_str().expect("in str");
        let expected = case["expected"].as_str().expect("expected str");
        let tag = case["tag"].as_str().unwrap_or("?");
        assert_eq!(mime_encode(input), expected, "corpus tag {tag}");
    }
}
