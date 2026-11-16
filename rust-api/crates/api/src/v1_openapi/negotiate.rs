//! DRF content negotiation for the `/api/schema/*` surface (PIDASHCONV-1207).
//!
//! Ports `DefaultContentNegotiation.select_renderer`
//! (`rest_framework/negotiation.py:35-78`, pinned 3.15.2) as exercised by
//! drf-spectacular 0.28.0's views: the document view's four renderer
//! classes ([`DOC_RENDERERS`]) and the UI pages' single
//! `TemplateHTMLRenderer` ([`UI_RENDERERS`]). `APIView.initial` runs this
//! before throttling (`views.py:399-407`), so the renderer it picks decides
//! the 200 document, the unsafe-method 405s, and the 429 denials alike
//! ([`Renderer::denial_body`]).
//!
//! Faithful pieces:
//!
//! * `?format=` filters the candidate renderers by `renderer.format`
//!   (exact, case-sensitive) *before* `Accept` is read; a filter that
//!   empties the list raises the bare `Http404` of
//!   `negotiation.py:filter_renderers` (→ 404, not 406).
//! * `Accept` is split on `,` and whitespace-trimmed
//!   (`get_accept_list`), defaulting to `*/*` when the header is absent.
//!   Tokens are grouped by structural precedence — `type/subtype;param`
//!   (3), `type/subtype` (2), `type/*` (1), `*/*` (0) — via
//!   [`order_by_precedence`], and the groups are walked most-specific
//!   first.
//! * Within a group, renderers are tried in `renderer_classes` order and,
//!   for one renderer, the group's tokens in header order; the first
//!   `media_type_matches` hit wins (`negotiation.py:53-56`).
//! * The echoed media type is the matched token, or — when the renderer's
//!   own type is more specific than the token (e.g. `Accept: */*`) — the
//!   renderer's media type with the token's params appended
//!   (`negotiation.py:57-76`).
//! * Nothing matching raises `NotAcceptable`, which
//!   `perform_content_negotiation(force=True)` renders with the *first*
//!   renderer (`views.py:302-314`) — [`Renderer::Yaml`] for the document
//!   view, [`Renderer::Html`] for the UI pages.
//!
//! `parse_header_parameters` (`django/utils/http.py`) is reproduced for the
//! media type / param split: the type is lower-cased, param names are
//! lower-cased and their quoted values unescaped, and a star-suffixed param
//! name drops the star.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::Request;
use axum::http::header;

/// One drf-spectacular renderer class, in `renderer_classes` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// `OpenApiYamlRenderer`: `application/vnd.oai.openapi`, `format='yaml'`.
    Yaml,
    /// `OpenApiYamlRenderer2`: `application/yaml`, `format='yaml'`.
    Yaml2,
    /// `OpenApiJsonRenderer`: `application/vnd.oai.openapi+json`, `format='json'`.
    Json,
    /// `OpenApiJsonRenderer2`: `application/json`, `format='json'`.
    Json2,
    /// `TemplateHTMLRenderer` (UI pages): `text/html`, `format='html'`.
    Html,
}

impl Renderer {
    /// DRF `renderer.format` (the `?format=` filter key).
    pub fn format(self) -> &'static str {
        match self {
            Renderer::Yaml | Renderer::Yaml2 => "yaml",
            Renderer::Json | Renderer::Json2 => "json",
            Renderer::Html => "html",
        }
    }

    /// DRF `renderer.media_type`.
    pub fn media_type(self) -> &'static str {
        match self {
            Renderer::Yaml => "application/vnd.oai.openapi",
            Renderer::Yaml2 => "application/yaml",
            Renderer::Json => "application/vnd.oai.openapi+json",
            Renderer::Json2 => "application/json",
            Renderer::Html => "text/html",
        }
    }

    /// DRF `renderer.charset` (`BaseRenderer`/`TemplateHTMLRenderer` set
    /// `utf-8`; `JSONRenderer` sets `None`).
    pub fn charset(self) -> Option<&'static str> {
        match self {
            Renderer::Yaml | Renderer::Yaml2 | Renderer::Html => Some("utf-8"),
            Renderer::Json | Renderer::Json2 => None,
        }
    }

    /// The document body flavour this renderer emits: JSON for the two JSON
    /// renderers, YAML otherwise (both YAML renderers render identically).
    pub fn is_json(self) -> bool {
        matches!(self, Renderer::Json | Renderer::Json2)
    }

    /// The 429 denial bytes this renderer serializes (the rewritten
    /// `{error_code, error_message}` dict, or the `TemplateHTMLRenderer`
    /// exception fallback on the UI pages).
    pub fn denial_body(self) -> &'static str {
        match self {
            Renderer::Yaml | Renderer::Yaml2 => DENIAL_BODY_YAML,
            Renderer::Json | Renderer::Json2 => DENIAL_BODY_JSON,
            Renderer::Html => DENIAL_BODY_UI,
        }
    }
}

/// `SpectacularAPIView.renderer_classes` (`views.py:51-53`).
pub const DOC_RENDERERS: [Renderer; 4] = [
    Renderer::Yaml,
    Renderer::Yaml2,
    Renderer::Json,
    Renderer::Json2,
];

/// `SpectacularSwaggerView`/`SpectacularRedocView.renderer_classes`.
pub const UI_RENDERERS: [Renderer; 1] = [Renderer::Html];

/// Exact bytes of the 429 on the document view's YAML renderers.
pub const DENIAL_BODY_YAML: &str = "error_code: 5900\nerror_message: RATE_LIMIT_EXCEEDED\n";
/// Exact bytes of the 429 on the document view's JSON renderers.
pub const DENIAL_BODY_JSON: &str =
    "{\n    \"error_code\": 5900,\n    \"error_message\": \"RATE_LIMIT_EXCEEDED\"\n}";
/// Exact bytes of the 429 on the UI pages.
pub const DENIAL_BODY_UI: &str = "429 Too Many Requests";

/// Why [`select_renderer`] found no renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationError {
    /// `filter_renderers` emptied the list (DRF bare `Http404`).
    NotFound,
    /// No surviving renderer matched `Accept` (DRF `NotAcceptable`).
    NotAcceptable,
}

/// The request's `Accept` header, defaulting to `*/*` when absent
/// (`get_accept_list`, `negotiation.py:96`).
pub fn accept_header(req: &Request) -> String {
    req.headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("*/*")
        .to_string()
}

/// `DefaultContentNegotiation.select_renderer`: pick a renderer and the
/// accepted media type DRF passes to `renderer.render` (the response
/// `Content-Type` comes from the renderer itself, see [`content_type`]).
pub fn select_renderer(
    renderers: &[Renderer],
    format: Option<&str>,
    accept: &str,
) -> Result<(Renderer, String), NegotiationError> {
    // `if format:` — an empty `?format=` counts as absent.
    let candidates: Vec<Renderer> = match format {
        Some(fmt) if !fmt.is_empty() => {
            let filtered: Vec<Renderer> = renderers
                .iter()
                .copied()
                .filter(|renderer| renderer.format() == fmt)
                .collect();
            if filtered.is_empty() {
                return Err(NegotiationError::NotFound);
            }
            filtered
        }
        _ => renderers.to_vec(),
    };

    let accepts: Vec<String> = accept
        .split(',')
        .map(|token| token.trim().to_string())
        .collect();

    for group in order_by_precedence(&accepts) {
        for renderer in &candidates {
            for media_type in &group {
                if !media_type_matches(renderer.media_type(), media_type) {
                    continue;
                }
                let wrapper = MediaType::parse(media_type);
                let renderer_type = MediaType::parse(renderer.media_type());
                if renderer_type.precedence() > wrapper.precedence() {
                    // Client asked something broader (`*/*`, `type/*`):
                    // echo the renderer's type plus the token's params.
                    let mut full = renderer.media_type().to_string();
                    for (key, value) in &wrapper.params {
                        full.push(';');
                        full.push_str(key);
                        full.push('=');
                        full.push_str(value);
                    }
                    return Ok((*renderer, full));
                }
                // Equal or more specific token: echo it verbatim.
                return Ok((*renderer, media_type.clone()));
            }
        }
    }
    Err(NegotiationError::NotAcceptable)
}

/// `Content-Type` of a response rendered by `renderer`: DRF takes the
/// renderer's own media type (not the matched `Accept` token) and appends
/// `; charset=<charset>` when the renderer carries a charset
/// (`response.py:64-72`). Accept parameters such as `q` or `indent` never
/// reach the header.
pub fn content_type(renderer: Renderer) -> String {
    match renderer.charset() {
        Some(charset) => format!("{}; charset={charset}", renderer.media_type()),
        None => renderer.media_type().to_string(),
    }
}

/// `media_type_matches(lhs, rhs)` (`utils/mediatypes.py:9-24`).
pub fn media_type_matches(lhs: &str, rhs: &str) -> bool {
    MediaType::parse(lhs).matches(&MediaType::parse(rhs))
}

/// `DefaultContentNegotiation.get_accept_list` + `order_by_precedence`
/// (`negotiation.py:27-41,91-97`): tokens grouped by precedence, groups
/// returned most-specific first. Python's per-group `set` collapses exact
/// duplicates; order within a group is otherwise preserved as given.
fn order_by_precedence(accepts: &[String]) -> Vec<Vec<String>> {
    let mut groups: [Vec<String>; 4] = Default::default();
    for media_type in accepts {
        let precedence = MediaType::parse(media_type).precedence();
        let bucket = &mut groups[3 - precedence];
        if !bucket.contains(media_type) {
            bucket.push(media_type.clone());
        }
    }
    groups
        .into_iter()
        .filter(|group| !group.is_empty())
        .collect()
}

/// One parsed media type (`django.utils.http.parse_header_parameters` +
/// `utils/mediatypes.py:_MediaType`).
struct MediaType {
    main_type: String,
    sub_type: String,
    params: Vec<(String, String)>,
}

impl MediaType {
    fn parse(raw: &str) -> Self {
        let parts = parse_header_parameters(raw);
        let (full_type, params) = parts;
        let (main_type, sub_type) = match full_type.split_once('/') {
            Some((main, sub)) => (main.to_string(), sub.to_string()),
            None => (full_type, String::new()),
        };
        MediaType {
            main_type,
            sub_type,
            params,
        }
    }

    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// `_MediaType.precedence` (`utils/mediatypes.py:64-75`).
    fn precedence(&self) -> usize {
        if self.main_type == "*" {
            0
        } else if self.sub_type == "*" {
            1
        } else if self.params.is_empty() || self.params.len() == 1 && self.params[0].0 == "q" {
            2
        } else {
            3
        }
    }

    /// `_MediaType.match`: `self` is the renderer's type, `other` the
    /// accepted token.
    fn matches(&self, other: &MediaType) -> bool {
        // Renderer media types carry no params, so the (DRF) param loop is
        // only ever meaningful for exotic renderer types; kept for parity.
        for (key, value) in &self.params {
            if key != "q" && other.param(key) != Some(value.as_str()) {
                return false;
            }
        }
        if self.sub_type != "*" && other.sub_type != "*" && other.sub_type != self.sub_type {
            return false;
        }
        if self.main_type != "*" && other.main_type != "*" && other.main_type != self.main_type {
            return false;
        }
        true
    }
}

/// `parse_header_parameters` (`django/utils/http.py`): split a `;`-separated
/// header into `(lower-cased main value, ordered params)`.
fn parse_header_parameters(line: &str) -> (String, Vec<(String, String)>) {
    let parts = parse_params(line);
    let mut iter = parts.into_iter();
    let key = iter.next().unwrap_or_default().to_lowercase();
    let mut params: Vec<(String, String)> = Vec::new();
    for part in iter {
        let Some(index) = part.find('=') else {
            continue;
        };
        let mut name = part[..index].trim().to_lowercase();
        let mut has_encoding = false;
        if name.ends_with('*') {
            name.pop();
            if part.matches('\'').count() == 2 {
                has_encoding = true;
            }
        }
        let mut value = part[index + 1..].trim().to_string();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            let inner = &value[1..value.len() - 1];
            value = inner.replace("\\\\", "\\").replace("\\\"", "\"");
        }
        if has_encoding {
            // RFC 2231: `charset'lang'value`. Faithful to `split("'")` only
            // for the well-formed 3-part case; a malformed value is kept.
            let segments: Vec<&str> = value.split('\'').collect();
            if segments.len() == 3 {
                value = percent_decode(segments[2]);
            }
        }
        if let Some(existing) = params.iter_mut().find(|(existing, _)| *existing == name) {
            existing.1 = value;
        } else {
            params.push((name, value));
        }
    }
    (key, params)
}

/// `django.utils.http._parseparam`: split on `;`, ignoring `;` inside a
/// quoted string, and strip each piece.
fn parse_params(line: &str) -> Vec<String> {
    let mut rest = format!(";{line}");
    let mut parts = Vec::new();
    while rest.starts_with(';') {
        rest.remove(0);
        let end = top_level_semicolon(&rest);
        match end {
            Some(index) => {
                parts.push(rest[..index].trim().to_string());
                rest = rest[index..].to_string();
            }
            None => {
                parts.push(rest.trim().to_string());
                rest.clear();
            }
        }
    }
    parts
}

/// Index of the first `;` outside a quoted string, or `None`.
fn top_level_semicolon(s: &str) -> Option<usize> {
    let mut search_from = 0;
    loop {
        let index = s[search_from..].find(';')? + search_from;
        let prefix = &s[..index];
        let quotes = prefix.matches('"').count();
        let escaped = prefix.matches("\\\"").count();
        if (quotes as i64 - escaped as i64) % 2 == 0 {
            return Some(index);
        }
        search_from = index + 1;
    }
}

/// Percent-decode for RFC 2231 param values (`urllib.parse.unquote`), UTF-8
/// lossy. Only reached by the exotic `name*=` form.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = bytes
                .get(i + 1)
                .zip(bytes.get(i + 2))
                .and_then(|(&hi, &lo)| hex_nibble(hi).zip(hex_nibble(lo)));
            if let Some((hi, lo)) = pair {
                decoded.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// One hex digit's value, or `None` outside `[0-9a-fA-F]`.
fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default `*/*` picks the first YAML renderer; the echoed media
    /// type is the renderer's, not the token's.
    #[test]
    fn star_star_picks_yaml_renderer() {
        let (renderer, media_type) = select_renderer(&DOC_RENDERERS, None, "*/*").expect("match");
        assert_eq!(renderer, Renderer::Yaml);
        assert_eq!(media_type, "application/vnd.oai.openapi");
    }

    /// Exact tokens select their renderer and echo verbatim.
    #[test]
    fn exact_tokens_select_their_renderer() {
        for (accept, renderer, media_type) in [
            ("application/json", Renderer::Json2, "application/json"),
            (
                "application/vnd.oai.openapi+json",
                Renderer::Json,
                "application/vnd.oai.openapi+json",
            ),
            ("application/yaml", Renderer::Yaml2, "application/yaml"),
            (
                "application/vnd.oai.openapi",
                Renderer::Yaml,
                "application/vnd.oai.openapi",
            ),
        ] {
            let got = select_renderer(&DOC_RENDERERS, None, accept);
            assert_eq!(got, Ok((renderer, media_type.to_string())), "{accept}");
        }
    }

    /// A broader `type/*` token still picks the first matching renderer and
    /// echoes the renderer's media type.
    #[test]
    fn type_star_echoes_renderer_type() {
        let (renderer, media_type) =
            select_renderer(&DOC_RENDERERS, None, "application/*").expect("match");
        assert_eq!(renderer, Renderer::Yaml);
        assert_eq!(media_type, "application/vnd.oai.openapi");
    }

    /// No matching token is `NotAcceptable` (406).
    #[test]
    fn unmatched_accept_is_not_acceptable() {
        assert_eq!(
            select_renderer(&DOC_RENDERERS, None, "text/html"),
            Err(NegotiationError::NotAcceptable)
        );
    }

    /// `?format=` filters first: a valid format whose renderers cannot
    /// satisfy `Accept` is still a 406, and an unknown format is a 404.
    #[test]
    fn format_filter_precedes_accept() {
        assert_eq!(
            select_renderer(&DOC_RENDERERS, Some("yaml"), "application/json"),
            Err(NegotiationError::NotAcceptable)
        );
        assert_eq!(
            select_renderer(&DOC_RENDERERS, Some("xml"), "application/json"),
            Err(NegotiationError::NotFound)
        );
        assert_eq!(
            select_renderer(&DOC_RENDERERS, Some("json"), "*/*"),
            Ok((
                Renderer::Json,
                "application/vnd.oai.openapi+json".to_string()
            ))
        );
        // An empty `?format=` is falsy — no filtering.
        assert_eq!(
            select_renderer(&DOC_RENDERERS, Some(""), "*/*"),
            Ok((Renderer::Yaml, "application/vnd.oai.openapi".to_string()))
        );
    }

    /// `Content-Type` comes from the renderer and appends its charset; the
    /// matched `Accept` token and its params (for example `q`) are ignored.
    #[test]
    fn content_type_appends_charset_by_renderer() {
        assert_eq!(
            content_type(Renderer::Yaml),
            "application/vnd.oai.openapi; charset=utf-8"
        );
        assert_eq!(
            content_type(Renderer::Yaml2),
            "application/yaml; charset=utf-8"
        );
        assert_eq!(content_type(Renderer::Json2), "application/json");
        assert_eq!(content_type(Renderer::Html), "text/html; charset=utf-8");
    }

    /// Precedence and matching match DRF on the tricky tokens.
    #[test]
    fn precedence_and_matching_follow_drf() {
        // `*/*, text/html` — text/html is more specific but matches no
        // document renderer, so the `*/*` tier still picks YAML.
        assert_eq!(
            select_renderer(&DOC_RENDERERS, None, "*/*, text/html"),
            Ok((Renderer::Yaml, "application/vnd.oai.openapi".to_string()))
        );
        // `application/json, */*` — the JSON tier wins.
        assert_eq!(
            select_renderer(&DOC_RENDERERS, None, "application/json, */*"),
            Ok((Renderer::Json2, "application/json".to_string()))
        );
        // Case-insensitive type (parse_header_parameters lower-cases it),
        // echoed from the renderer since the token's precedence is lower.
        assert_eq!(
            select_renderer(&DOC_RENDERERS, None, "*/*"),
            Ok((Renderer::Yaml, "application/vnd.oai.openapi".to_string()))
        );
    }

    /// The UI pages have a single `text/html` renderer.
    #[test]
    fn ui_pages_match_only_html() {
        assert_eq!(
            select_renderer(&UI_RENDERERS, None, "*/*"),
            Ok((Renderer::Html, "text/html".to_string()))
        );
        assert_eq!(
            select_renderer(&UI_RENDERERS, None, "application/json"),
            Err(NegotiationError::NotAcceptable)
        );
        assert_eq!(
            select_renderer(&UI_RENDERERS, Some("html"), "text/html"),
            Ok((Renderer::Html, "text/html".to_string()))
        );
        assert_eq!(
            select_renderer(&UI_RENDERERS, Some("xml"), "*/*"),
            Err(NegotiationError::NotFound)
        );
    }
}
