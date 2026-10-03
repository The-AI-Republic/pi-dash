//! App workspace serializer extras: user links, recent visits, home/user
//! preferences, stickies (D-24 serializers C, PIDASHCONV-602).
//!
//! Port of `apps/api/pi_dash/app/serializers/workspace.py:196-383`:
//!
//! * `WorkspaceUserLinkSerializer` (`:196-247`): `__all__`, read-only
//!   `workspace`/`owner`; `to_internal_value` scheme prefix (`:202-207`),
//!   `validate_url` (`:209-216`), dup guards on `create` (`:218-232`) and
//!   `update` (`:234-246`).
//! * Recent-visit family (`:249-345`): `IssueRecentVisitSerializer`,
//!   `ProjectRecentVisitSerializer`, `PageRecentVisitSerializer`,
//!   `get_entity_model_and_serializer`, `WorkspaceRecentVisitSerializer`.
//! * `WorkspaceHomePreferenceSerializer` (`:347-352`).
//! * `StickySerializer` (`:354-377`) + the `validate_html_content` /
//!   `validate_binary_data` semantics
//!   (`pi_dash/utils/content_validator.py`).
//! * `WorkspaceUserPreferenceSerializer` (`:379-383`).
//!
//! Fixture oracle: F-W24-03
//! (`rust-api/fixtures/app_workspace/serializers/extras.golden.json`).
//!
//! Pure kernels in the `app_project::ser_member` style: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! output order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`/UUID field); a null FK renders `null`.
//! Datetimes cross this boundary already rendered as DRF iso-8601 strings
//! (formatting belongs to the DB edge), so rendering here is a byte-exact
//! passthrough. JSON blobs pass through by reference.
//!
//! Wire order for `__all__` (`:196`, `:354`) is declared fields first, then
//! model fields with the non-relational fields before the forward relations
//! (`ModelSerializer.get_default_field_names`), verified live against the
//! project venv (test settings, no DB needed for field construction):
//! [`USER_LINK_WIRE_FIELDS`] / [`STICKY_WIRE_FIELDS`].
//!
//! Raised details vs live envelopes. The kernels in this module emit the
//! details the Python code *raises*, exactly as F-W24-03 records them
//! (bare-string dicts). Live DRF reshapes two of them on the way out
//! (verified live; the handler layer owns the wire envelopes):
//!
//! * `validate_url` raises `{"error": "Invalid URL format."}`; DRF nests
//!   field-validator failures under the field, so `serializer.errors` is
//!   `{"url": {"error": "Invalid URL format."}}`.
//! * `StickySerializer.validate` raises `{"error": "html content is not
//!   valid"}`; `Serializer.run_validation` funnels it through
//!   `as_serializer_error`, so the wire body is
//!   `{"error": ["html content is not valid"]}` (same list-wrap for the
//!   `description_binary` arm).
//!
//! Single-owner notes (never fork a helper). The
//! `validate_html_content` / `validate_binary_data` transcription is owned
//! by [`pidash_types::v1_assets::sticky`] (D-21, merged) and reused here;
//! only the error *bodies* differ (this layer emits the raised details,
//! that layer the live list-wrapped wire bodies). The Django
//! `URLValidator` transcription below is necessarily local: the
//! `api`-crate transcriptions (`assistant::common`, module-link handlers)
//! sit above this crate in the `types → db → services → api` graph, and
//! per-domain transcription of Django validators is merged precedent. Its
//! verdicts are pinned against live Django by the `django_url_truth_table`
//! test (228 vectors).
//!
//! Ported bugs / divergences (translate, don't redesign; listed for the PR):
//!
//! * BUG-prefix-mutate (`workspace.py:202-207`):
//!   `to_internal_value` mutates the input dict in place
//!   (`data["url"] = "http://" + url`). [`prefix_user_link_url`] takes
//!   `&mut` for the same reason.
//! * BUG-prefix-500 (`:204`): a truthy non-string `url` has no
//!   `.startswith`, so `to_internal_value` raises `AttributeError` (500).
//!   [`LinkPrefixError::AttributeError`] models that branch.
//! * BUG-dup-lookup (`:223-227`): `create` reads `workspace_id`/`owner_id`
//!   from `validated_data`, but both fields are read-only (`:200`), so
//!   they are `None` unless the view passes `save()` kwargs — the lookup
//!   is then `url AND workspace_id IS NULL AND owner_id IS NULL`, which
//!   matches nothing. [`UserLinkDupLookup`] keeps the `None`s (a Django
//!   `None` filter means `IS NULL`, never "ignore this column").
//! * BUG-queryset (`:282-287`): `get_project_members` returns the raw
//!   `QuerySet`, not a `list`. DRF renders both as the same JSON array,
//!   so the wire shape is a plain id list (no observable difference).
//! * BUG-read-only-extras (`:329`, `:351`, `:383`):
//!   `WorkspaceRecentVisitSerializer`, `WorkspaceHomePreferenceSerializer`
//!   and `WorkspaceUserPreferenceSerializer` name fields in
//!   `read_only_fields` that are not in their `Meta.fields`; DRF ignores
//!   the extras silently. The consts below carry them verbatim.
//! * BUG-binary-dead (`:371-374`): `description_binary` is a `BinaryField`,
//!   which DRF maps to `ModelField(read_only=True)` — validated data never
//!   carries the key, so the binary arm never runs through the serializer
//!   (verified live: invalid binary input validates clean). The arm itself
//!   is transcribed exactly and tested directly.
//! * BUG-page-project-id (`:305-306`): `get_project_id` prefers an
//!   annotated `obj.project_id` (`hasattr`, true only for annotated rows —
//!   `Page` has no `project` FK, only the `projects` M2M) and falls back to
//!   the first related project. [`page_visit_project_id`] keeps the branch
//!   order.
//!
//! Out of scope (documented, not ported): per-field `required`/`allow_null`
//! enforcement belongs to the handler layer (handler goldens); entity
//! fetching for `entity_data` (`objects.get`) is a handler query — this
//! module owns the dispatch map and the `None` rules.

use pidash_types::v1_assets::sticky as sticky_kernel;
use serde::Serialize;
use serde_json::{Map, Value};

// ================================================================
// WorkspaceUserLinkSerializer (workspace.py:196-247)
// ================================================================

/// `WorkspaceUserLinkSerializer` wire keys in output order
/// (`workspace.py:196-200`, `fields = "__all__"`: declared `id` first via
/// `BaseSerializer` (`base.py:8-9`), then non-relational model fields, then
/// forward relations — verified live).
pub const USER_LINK_WIRE_FIELDS: [&str; 12] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "workspace",
    "project",
    "owner",
];

/// `WorkspaceUserLinkSerializer.Meta.read_only_fields`
/// (`workspace.py:200`). `id`/`created_at`/`updated_at` are additionally
/// read-only via `BaseSerializer`/auto fields (verified live); `deleted_at`
/// is writable.
pub const USER_LINK_READ_ONLY_FIELDS: &[&str] = &["workspace", "owner"];

/// `validate_url` rejection message (`workspace.py:214`).
pub const USER_LINK_INVALID_URL_MESSAGE: &str = "Invalid URL format.";

/// Dup-URL rejection on `create` and `update` (`workspace.py:230,244`).
pub const USER_LINK_DUPLICATE_MESSAGE: &str = "URL already exists for this workspace and owner";

/// Python truthiness over a JSON input value, for the `to_internal_value`
/// guards (`if url`, `if "description_html" in data and
/// data["description_html"]`, ...). Mirrors CPython: `None`/`False`/`0`/
/// `""`/`[]`/`{}` are falsy, everything else truthy.
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Failure of [`prefix_user_link_url`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPrefixError {
    /// A truthy non-string `url` has no `.startswith`
    /// (`workspace.py:204`): CPython raises `AttributeError`, which the
    /// handler layer renders as a generic 500.
    AttributeError,
}

/// `to_internal_value` scheme step (`workspace.py:202-207`): a present,
/// truthy `url` without an `http://`/`https://` prefix gains an `http://`
/// prefix, written back into the input map (the in-place mutation bug,
/// ported exactly). Missing and falsy values pass through untouched; the
/// required check fires later at field level. The prefix test is
/// case-sensitive, so e.g. `HTTP://…` is double-prefixed and then fails
/// `validate_url` (verified live).
pub fn prefix_user_link_url(input: &mut Map<String, Value>) -> Result<(), LinkPrefixError> {
    // `data.get("url", "")` — a missing key behaves as "" (falsy).
    let replacement = match input.get("url") {
        None => None,
        Some(value) if !is_python_truthy(value) => None,
        Some(Value::String(text))
            if text.starts_with("http://") || text.starts_with("https://") =>
        {
            None
        }
        Some(Value::String(text)) => Some(format!("http://{text}")),
        Some(_) => return Err(LinkPrefixError::AttributeError),
    };
    if let Some(prefixed) = replacement {
        input.insert("url".to_owned(), Value::String(prefixed));
    }
    Ok(())
}

/// `validate_url` (`workspace.py:209-216`): Django's `URLValidator`
/// accepts anything it accepts; any failure raises the `{"error": …}`
/// dict. Returns the raised detail (F-W24-03 form); live DRF nests it
/// under `"url"` in `serializer.errors` (see module docs).
pub fn validate_user_link_url(value: &str) -> Result<(), Value> {
    if django_url_valid(value) {
        Ok(())
    } else {
        Err(invalid_user_link_url_body())
    }
}

/// Raised body for the `validate_url` rejection (`workspace.py:214`).
pub fn invalid_user_link_url_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(USER_LINK_INVALID_URL_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// Raised body for the `create`/`update` dup-URL rejection
/// (`workspace.py:230,244`); both sites share the one message.
pub fn duplicate_user_link_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(USER_LINK_DUPLICATE_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// The duplicate-URL lookup both `create` and `update` run before saving.
/// `None` filter values mean `IS NULL` in Django, never "ignore this
/// column" — so a `None` `workspace_id`/`owner_id` (the BUG-dup-lookup
/// case) matches nothing on these `NOT NULL` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserLinkDupLookup<'a> {
    /// `validated_data.get("url")` (`:221,237`).
    pub url: Option<&'a str>,
    /// `validated_data.get("workspace_id")` on create (`:225`);
    /// `instance.workspace_id` on update (`:240`).
    pub workspace_id: Option<&'a str>,
    /// `validated_data.get("owner_id")` on create (`:226`);
    /// `instance.owner` (same id) on update (`:240`).
    pub owner_id: Option<&'a str>,
    /// `update` excludes the instance itself (`.exclude(pk=instance.id)`,
    /// `:243`); `None` on create (no exclusion).
    pub exclude_pk: Option<&'a str>,
}

/// `create` dup guard (`workspace.py:218-232`): filter
/// `(url, workspace_id, owner_id)` straight from `validated_data`;
/// `exists()` rejects with [`duplicate_user_link_body`].
pub fn user_link_create_lookup<'a>(
    url: Option<&'a str>,
    workspace_id: Option<&'a str>,
    owner_id: Option<&'a str>,
) -> UserLinkDupLookup<'a> {
    UserLinkDupLookup {
        url,
        workspace_id,
        owner_id,
        exclude_pk: None,
    }
}

/// `update` dup guard (`workspace.py:234-246`): filter `(url,
/// instance.workspace_id, instance.owner)` excluding the instance's own
/// pk; `exists()` rejects with [`duplicate_user_link_body`]. A PATCH that
/// omits `url` passes `None`, which matches nothing (the `url` column is
/// `NOT NULL`) — unlike `ModuleLinkSerializer.update`, this `update` does
/// not call `validate_url`.
pub fn user_link_update_lookup<'a>(
    url: Option<&'a str>,
    instance_workspace_id: &'a str,
    instance_owner_id: &'a str,
    instance_pk: &'a str,
) -> UserLinkDupLookup<'a> {
    UserLinkDupLookup {
        url,
        workspace_id: Some(instance_workspace_id),
        owner_id: Some(instance_owner_id),
        exclude_pk: Some(instance_pk),
    }
}

// ================================================================
// Django URLValidator (django/core/validators.py, Django 4.2)
// ================================================================

/// `URLValidator.max_length` (`validators.py:113`): counted in characters
/// (`len(value)`), not bytes.
const MAX_URL_CHARS: usize = 2048;

/// `URLValidator.schemes` (`validators.py:112`).
const URL_SCHEMES: &[&str] = &["http", "https", "ftp", "ftps"];

/// Maximum hostname length (`validators.py:154-159`, RFC 1034 §3.1).
const MAX_HOSTNAME_CHARS: usize = 253;

/// Codepoints whose NFKC normalization introduces one of `/?#@:`
/// (`urllib/parse.py::_checknetloc`, reached from every `urlsplit` call).
/// Enumerated once with CPython's `unicodedata.normalize` over all
/// 0x110000 codepoints (single-char table; canonical composition can never
/// produce ASCII `/ ? # @ :`, so per-char membership is exactly the
/// whole-string check Django runs).
const NFKC_FORBIDDEN: [char; 19] = [
    '\u{2047}', '\u{2048}', '\u{2049}', '\u{2100}', '\u{2101}', '\u{2105}', '\u{2106}', '\u{2A74}',
    '\u{FE13}', '\u{FE16}', '\u{FE55}', '\u{FE56}', '\u{FE5F}', '\u{FE6B}', '\u{FF03}', '\u{FF0F}',
    '\u{FF1A}', '\u{FF1F}', '\u{FF20}',
];

/// Django's `URLValidator.__call__` (`validators.py:118-159`) as
/// `validate_url` runs it: length cap, unsafe chars, scheme allowlist,
/// `urlsplit` (which strict-validates bracketed hosts itself and runs the
/// NFKC check), the host regex, the IDN retry, the strict IPv6 check, then
/// the 253-char hostname cap. The `regex` crate has no lookaround, so the
/// `(?!-)` / `(?<!-)` guards are explicit first/last-char checks. Every
/// branch below is pinned by `django_url_truth_table` (228 live vectors).
fn django_url_valid(value: &str) -> bool {
    // `len(value) > self.max_length` — characters, not bytes.
    if value.chars().count() > MAX_URL_CHARS {
        return false;
    }
    // `self.unsafe_chars.intersection(value)`.
    if value.chars().any(|c| matches!(c, '\t' | '\r' | '\n')) {
        return false;
    }
    // `scheme = value.split("://")[0].lower()`.
    let scheme = value.split("://").next().unwrap_or("").to_lowercase();
    if !URL_SCHEMES.contains(&scheme.as_str()) {
        return false;
    }
    // `urlsplit(value)` netloc decomposition (`_splitnetloc`: authority
    // runs to the first `/?#`).
    let after = match value.split_once("://") {
        Some((_, rest)) => rest,
        None => return false,
    };
    let auth_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(auth_end);
    // Resource path: `(?:[/?#][^\s]*)?` — Python `\s` semantics.
    if rest.chars().any(is_python_space) {
        return false;
    }
    // `urlsplit` `ValueError` parity: bracket validation + NFKC check.
    // (Its leading-C0 strip and tab/CR/LF removal cannot matter here:
    // unsafe chars were already rejected and the scheme check precedes.)
    if !urlsplit_netloc_ok(authority) {
        return false;
    }
    // Userinfo + host + port decomposition (the regex minus host class).
    let Some(parts) = split_url_structure(authority) else {
        // The IDN retry re-runs the same structural match over an
        // ACE-encoded host: userinfo/port/path failures are never fixed
        // by it (punycode never removes `@`, `:` or whitespace), so a
        // structural failure is final.
        return false;
    };
    if parts.bracketed {
        // The urlsplit parity check already strict-parsed the literal
        // (same `ipaddress` verdict Django's `validate_ipv6_address`
        // re-checks on this path), and the strict parse implies the
        // regex `[0-9a-f:.]+` charset, so only the hostname cap remains.
        return hostname_within_cap(parts.host);
    }
    if url_host_matches(parts.host) {
        return hostname_within_cap(parts.host);
    }
    // IDN retry (`validators.py:131-140`): ACE-encode and re-match. Only
    // reachable for non-ASCII hosts (an all-ASCII netloc round-trips
    // byte-identical, so the retry is futile there and skipped); the cap
    // still applies to the ORIGINAL hostname. Host-only encoding is
    // verdict-equivalent to Django's whole-netloc `punycode()`: userinfo
    // already passed structurally (non-ASCII userinfo matches
    // `[^\s:@/]+` on the first pass) and ports are ASCII digits.
    if parts.host.is_ascii() {
        return false;
    }
    let ace = match idna::domain_to_ascii(parts.host) {
        Ok(ace) => ace,
        Err(_) => return false,
    };
    if !url_host_matches(&ace) {
        return false;
    }
    hostname_within_cap(parts.host)
}

/// Python `re` `\s` over `str`: ASCII whitespace plus `\x1c-\x1f`, `\x85`
/// and Unicode `White_Space`. (`char::is_whitespace` alone misses
/// `\x1c-\x1f`, which Python rejects in the path/userinfo.)
fn is_python_space(c: char) -> bool {
    matches!(c, '\x1c'..='\x1f') || c.is_whitespace()
}

/// `urlsplit()` `ValueError` parity (`urllib/parse.py`): the bracket
/// checks plus `_checknetloc`. Any failure maps to invalid (Django
/// catches `ValueError` at `validators.py:126-128`).
fn urlsplit_netloc_ok(netloc: &str) -> bool {
    let has_open = netloc.contains('[');
    let has_close = netloc.contains(']');
    if has_open != has_close {
        return false; // "Invalid IPv6 URL"
    }
    if has_open && !bracketed_netloc_ok(netloc) {
        return false;
    }
    // `_checknetloc`: early return for empty/ASCII; otherwise any char
    // whose NFKC form introduces `/?#@:` rejects. (Literal `@:?#` are
    // stripped before Django's check, but every table entry is non-ASCII,
    // so no stripping is needed here.)
    if !netloc.is_ascii()
        && netloc
            .chars()
            .any(|c| NFKC_FORBIDDEN.binary_search(&c).is_ok())
    {
        return false;
    }
    true
}

/// `_check_bracketed_netloc` (`urllib/parse.py`): the post-userinfo part
/// (after the LAST `@`) must either start its bracketed literal at once
/// (nothing before `[`, nothing but `:port` after `]`) or carry no `[` at
/// all — and the resulting hostname must be a valid bracketed host.
fn bracketed_netloc_ok(netloc: &str) -> bool {
    // `netloc.rpartition('@')[2]`.
    let hostport = netloc.rsplit('@').next().unwrap_or("");
    match hostport.find('[') {
        Some(0) => {
            let bracketed = &hostport[1..];
            match bracketed.find(']') {
                Some(end) => {
                    let port = &bracketed[end + 1..];
                    if !port.is_empty() && !port.starts_with(':') {
                        return false;
                    }
                    bracketed_host_ok(&bracketed[..end])
                }
                // `partition(']')` with no `]` yields `(whole, "", "")`.
                None => bracketed_host_ok(bracketed),
            }
        }
        // Data before `[`, or no `[` in the post-userinfo part (hostname
        // runs to the first `:`).
        Some(_) => false,
        None => bracketed_host_ok(hostport.split(':').next().unwrap_or("")),
    }
}

/// `_check_bracketed_host` (`urllib/parse.py`): IPvFuture literals match
/// the `v…` grammar, anything else must parse as an IP — and a bracketed
/// IPv4 is rejected (`strict_ipv6_ok` never accepts a bare quad).
fn bracketed_host_ok(hostname: &str) -> bool {
    if hostname.starts_with('v') {
        return ipvfuture_ok(hostname);
    }
    strict_ipv6_ok(hostname)
}

/// The IPvFuture grammar (`\Av[a-fA-F0-9]+\..+\Z`, `parse.py`): `v`, hex,
/// a dot, then at least one char. (An IPvFuture host can never pass the
/// URL regex — `v` is outside `[0-9a-f:.]` — but `u[]@v1.fe` style inputs
/// with brackets in the *userinfo* pass urlsplit on this branch and then
/// match the domain rules; pinned live.)
fn ipvfuture_ok(hostname: &str) -> bool {
    let rest = &hostname[1..];
    let hex_len = rest.bytes().take_while(u8::is_ascii_hexdigit).count();
    if hex_len == 0 {
        return false;
    }
    let tail = &rest[hex_len..];
    if !tail.starts_with('.') {
        return false;
    }
    let after_dot = &tail[1..];
    // `.` (regex dot) matches anything but `\n`, which cannot reach here.
    !after_dot.is_empty() && !after_dot.contains('\n')
}

/// `ipaddress.ip_address` acceptance for bracketed use
/// (`_check_bracketed_host` and Django's `validate_ipv6_address` agree):
/// strict IPv6 parse, plus the leading-zero rule `ipaddress` applies to
/// an embedded dotted quad (`::ffff:01.2.3.4` fails) which Rust's parser
/// would otherwise accept.
fn strict_ipv6_ok(content: &str) -> bool {
    if content.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    if let Some(tail) = content.rsplit(':').next().filter(|tail| tail.contains('.')) {
        return is_strict_ipv4_tail(tail);
    }
    true
}

/// Strict dotted quad (no leading zeros, 0-255): shared by the embedded
/// tail check above and Django's `ipv4_re` below.
fn is_strict_ipv4_tail(tail: &str) -> bool {
    let parts: Vec<&str> = tail.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.len() <= 3
            && part.bytes().all(|b| b.is_ascii_digit())
            && !(part.len() > 1 && part.starts_with('0'))
            && part.parse::<u16>().is_ok_and(|n| n <= 255)
    })
}

/// Post-userinfo authority with a validated port: the-userinfo, bracket
/// and port arms of the Django host regex. Host *classification* is the
/// separate [`url_host_matches`] step (the regex needs it twice: first
/// match plus IDN retry).
struct UrlAuthority<'a> {
    host: &'a str,
    bracketed: bool,
}

/// Userinfo (`(?:[^\s:@/]+(?::[^\s:@/]*)?@)?`, after the LAST `@`),
/// bracketed-vs-bare split and the `(?::[0-9]{1,5})?` port rule. Returns
/// the raw host for classification.
fn split_url_structure(authority: &str) -> Option<UrlAuthority<'_>> {
    let hostport = match authority.rfind('@') {
        Some(at) => {
            if !valid_url_userinfo(&authority[..at]) {
                return None;
            }
            &authority[at + 1..]
        }
        None => authority,
    };
    if let Some(rest) = hostport.strip_prefix('[') {
        let end = rest.find(']')?;
        let inside = &rest[..end];
        if inside.is_empty()
            || !inside
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':')
        {
            return None;
        }
        let after = &rest[end + 1..];
        if after.is_empty() {
            return Some(UrlAuthority {
                host: inside,
                bracketed: true,
            });
        }
        let port = after.strip_prefix(':')?;
        if !valid_url_port(port) {
            return None;
        }
        return Some(UrlAuthority {
            host: inside,
            bracketed: true,
        });
    }
    match hostport.split_once(':') {
        None => Some(UrlAuthority {
            host: hostport,
            bracketed: false,
        }),
        Some((host, port)) => {
            if !valid_url_port(port) {
                return None;
            }
            Some(UrlAuthority {
                host,
                bracketed: false,
            })
        }
    }
}

/// The `user:pass` part of the authority: a non-empty name without
/// whitespace/`:`/`@`/`/`, then an optional `:password` (possibly empty)
/// without those chars. A second colon fails (`u:s:s@…` is invalid live).
fn valid_url_userinfo(userinfo: &str) -> bool {
    if userinfo.is_empty() {
        return false;
    }
    let (name, password) = match userinfo.split_once(':') {
        Some((name, password)) => (name, Some(password)),
        None => (userinfo, None),
    };
    let clean = |part: &str| {
        !part.is_empty()
            && !part
                .chars()
                .any(|c| is_python_space(c) || c == ':' || c == '@' || c == '/')
    };
    if !clean(name) {
        return false;
    }
    if let Some(password) = password {
        if password
            .chars()
            .any(|c| is_python_space(c) || c == ':' || c == '@' || c == '/')
        {
            return false;
        }
    }
    true
}

/// `(?::[0-9]{1,5})?`: 1-5 ASCII digits (leading zeros fine — `:00080`
/// validates live).
fn valid_url_port(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

/// Host classification over a bare host
/// (`ipv4_re|ipv6_re|host_re`, case-insensitive): `localhost`, strict
/// IPv4, or dot-labels with the TLD rule. Matching is done without
/// lowercasing (lowering `İ` would change label lengths); ASCII
/// case-insensitivity is explicit in each check.
fn url_host_matches(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if is_django_ipv4(host) {
        return true;
    }
    valid_domain_labels(host)
}

/// Django `ipv4_re`: four dot-parts, each `0`, `25[0-5]`, `2[0-4][0-9]`,
/// `1[0-9]{1,2}` or `[1-9][0-9]?` — i.e. 0-255 with no leading zeros.
fn is_django_ipv4(host: &str) -> bool {
    is_strict_ipv4_tail(host)
}

/// Dot-labels (`hostname_re domain_re tld_re`): one trailing dot is
/// tolerated; every non-TLD label is 1-63 chars with alnum/`\u{a1}-\u{ffff}`
/// edges; the TLD is 2-63 chars of letters/hyphens or an `xn--` punycode
/// label, never starting or ending with a hyphen. Note the TLD allows NO
/// digits outside the punycode form (`example.c0m` is invalid live).
fn valid_domain_labels(host: &str) -> bool {
    let trimmed = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<&str> = trimmed.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (head, tld) = labels.split_at(labels.len() - 1);
    head.iter().all(|label| valid_domain_label(label)) && valid_tld_label(tld[0])
}

/// A label edge char: ASCII alnum or the `ul` range `\u{a1}-\u{ffff}`
/// (BMP-only — astral chars like emoji take the IDN path instead).
fn is_label_edge(c: char) -> bool {
    c.is_ascii_alphanumeric() || ('\u{a1}'..='\u{ffff}').contains(&c)
}

fn valid_domain_label(label: &str) -> bool {
    let chars: Vec<char> = label.chars().collect();
    if chars.is_empty() || chars.len() > 63 {
        return false;
    }
    if !is_label_edge(chars[0]) || !is_label_edge(chars[chars.len() - 1]) {
        return false;
    }
    chars.iter().all(|c| is_label_edge(*c) || *c == '-')
}

fn valid_tld_label(tld: &str) -> bool {
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    // `xn--[a-z0-9]{1,59}` (case-insensitive). `get` (not slicing:
    // byte 4 may split a multibyte char) then ASCII compare.
    if tld
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("xn--"))
    {
        let rest = &tld[4..];
        return !rest.is_empty()
            && rest.len() <= 59
            && rest.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    let chars: Vec<char> = tld.chars().collect();
    if chars.len() < 2 || chars.len() > 63 {
        return false;
    }
    chars
        .iter()
        .all(|c| c.is_ascii_alphabetic() || ('\u{a1}'..='\u{ffff}').contains(c) || *c == '-')
}

/// The 253-char hostname cap (`validators.py:157-159`) over
/// `splitted_url.hostname` (lowercased, brackets stripped, `None` when
/// empty — unreachable on a matched host, kept for fidelity). `_hostinfo`
/// agrees with the structural split above on every regex-passing shape
/// (bracketed-at-0 unwraps to the same inside; bare cuts at the same
/// first `:`), so the cap input is identical to Django's.
fn hostname_within_cap(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    host.to_lowercase().chars().count() <= MAX_HOSTNAME_CHARS
}

/// One `workspace_user_links` row for
/// [`user_link_to_representation`]: UUID/FK ids and datetimes cross
/// already rendered as strings (see module docs); `metadata` passes
/// through by reference. `title` is nullable (`CharField(null=True)`,
/// `db/models/workspace.py:417`); `project` is nullable
/// (`WorkspaceBaseModel.project`, `:187`); `created_by`/`updated_by` are
/// nullable audit FKs.
pub struct UserLinkRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub owner: &'a str,
}

/// `WorkspaceUserLinkSerializer` read shape in [`USER_LINK_WIRE_FIELDS`]
/// order.
#[derive(Debug, Serialize)]
pub struct UserLinkView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub project: Option<&'a str>,
    pub owner: &'a str,
}

/// Port of `WorkspaceUserLinkSerializer.to_representation`
/// (`workspace.py:196-200`, default `ModelSerializer` rendering).
pub fn user_link_to_representation<'a>(row: &'a UserLinkRow<'a>) -> UserLinkView<'a> {
    UserLinkView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        title: row.title,
        url: row.url,
        metadata: row.metadata,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        project: row.project,
        owner: row.owner,
    }
}

// ================================================================
// Recent-visit family (workspace.py:249-345)
// ================================================================

/// `IssueRecentVisitSerializer.Meta.fields` (`workspace.py:255-265`),
/// wire order. Plain `ModelSerializer` (NOT `BaseSerializer`), so `id`
/// renders via the auto `UUIDField` (same `str(uuid)` wire form).
pub const ISSUE_VISIT_WIRE_FIELDS: [&str; 9] = [
    "id",
    "name",
    "state",
    "priority",
    "assignees",
    "type",
    "sequence_id",
    "project_id",
    "project_identifier",
];

/// `ProjectRecentVisitSerializer.Meta.fields` (`workspace.py:280`),
/// wire order.
pub const PROJECT_VISIT_WIRE_FIELDS: [&str; 5] =
    ["id", "name", "logo_props", "project_members", "identifier"];

/// `PageRecentVisitSerializer.Meta.fields` (`workspace.py:296-303`),
/// wire order.
pub const PAGE_VISIT_WIRE_FIELDS: [&str; 6] = [
    "id",
    "name",
    "logo_props",
    "project_id",
    "owned_by",
    "project_identifier",
];

/// `WorkspaceRecentVisitSerializer.Meta.fields` (`workspace.py:328`),
/// wire order.
pub const RECENT_VISIT_WIRE_FIELDS: [&str; 5] = [
    "id",
    "entity_name",
    "entity_identifier",
    "entity_data",
    "visited_at",
];

/// `WorkspaceRecentVisitSerializer.Meta.read_only_fields`
/// (`workspace.py:329`). None of these are in `Meta.fields`
/// (BUG-read-only-extras); DRF ignores them silently — carried verbatim.
pub const RECENT_VISIT_READ_ONLY_FIELDS: &[&str] =
    &["workspace", "owner", "created_by", "updated_by"];

/// `get_entity_model_and_serializer` (`workspace.py:314-320`): the known
/// entity names. Anything else maps to `(None, None)` (case-sensitive —
/// `"ISSUE"` misses live).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentVisitEntity {
    Issue,
    Page,
    Project,
}

/// Dispatch half of `get_entity_model_and_serializer`: `issue`/`page`/
/// `project` resolve, anything else (including `""` and wrong case) is
/// `None`, and `entity_data` is then `None` without a fetch (`:337`).
pub fn recent_visit_entity(entity_type: &str) -> Option<RecentVisitEntity> {
    match entity_type {
        "issue" => Some(RecentVisitEntity::Issue),
        "page" => Some(RecentVisitEntity::Page),
        "project" => Some(RecentVisitEntity::Project),
        _ => None,
    }
}

impl RecentVisitEntity {
    /// The entity model of the dispatched pair (`:316-318`).
    pub fn model_name(self) -> &'static str {
        match self {
            RecentVisitEntity::Issue => "Issue",
            RecentVisitEntity::Page => "Page",
            RecentVisitEntity::Project => "Project",
        }
    }

    /// The entity serializer of the dispatched pair (`:316-318`).
    pub fn serializer_name(self) -> &'static str {
        match self {
            RecentVisitEntity::Issue => "IssueRecentVisitSerializer",
            RecentVisitEntity::Page => "PageRecentVisitSerializer",
            RecentVisitEntity::Project => "ProjectRecentVisitSerializer",
        }
    }
}

/// `WorkspaceRecentVisitSerializer.get_entity_data` (`workspace.py:331-344`)
/// over an already-fetched entity rendering: an unknown `entity_type`
/// (no dispatch) or a missing row (`DoesNotExist`, including a `NULL`
/// `entity_identifier`, which matches nothing) both render `entity_data`
/// as `None`; only a known entity with a found row renders its
/// serializer output. The fetch itself (`entity_model.objects.get(pk=…)`)
/// is a handler query.
pub fn recent_visit_entity_data(entity_type: &str, fetched: Option<Value>) -> Option<Value> {
    match (recent_visit_entity(entity_type), fetched) {
        (Some(_), Some(data)) => Some(data),
        _ => None,
    }
}

/// `PageRecentVisitSerializer.get_project_id` (`workspace.py:305-306`):
/// an annotated `obj.project_id` wins when the attribute exists
/// (`hasattr` — true only for annotated rows, since `Page` has no
/// `project` FK, only the `projects` M2M at `db/models/page.py:52`).
/// Otherwise the first related project id (`None` when there is none).
/// `annotated` is `Some` exactly when the attribute exists
/// (BUG-page-project-id: branch order matters, not just the values).
pub fn page_visit_project_id<'a>(
    annotated: Option<Option<&'a str>>,
    fallback_first_id: Option<&'a str>,
) -> Option<&'a str> {
    match annotated {
        Some(value) => value,
        None => fallback_first_id,
    }
}

/// One `Issue` row for [`issue_visit_to_representation`]: `state`/`type`
/// are nullable FKs; `priority` renders its stored choice value (`None`
/// renders `null`); `assignees` is the live-assignee id array
/// (`get_assignees`, `:271-272`: `issue_assignee__deleted_at__isnull=True`,
/// resolved by the caller); `project_id` is the raw FK attname (DRF
/// `ReadOnlyField`, UUID rendered as string).
pub struct IssueVisitRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub assignees: &'a [&'a str],
    pub issue_type: Option<&'a str>,
    pub sequence_id: i64,
    pub project_id: &'a str,
    pub project_identifier: Option<&'a str>,
}

/// `IssueRecentVisitSerializer` read shape in [`ISSUE_VISIT_WIRE_FIELDS`]
/// order. `get_project_identifier` (`:267-269`) is
/// `obj.project.identifier or None` (resolved by the caller);
/// `get_assignees` (`:271-272`) is the live-assignee id array.
#[derive(Debug, Serialize)]
pub struct IssueVisitView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub state: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub assignees: Vec<&'a str>,
    #[serde(rename = "type")]
    pub issue_type: Option<&'a str>,
    pub sequence_id: i64,
    pub project_id: &'a str,
    pub project_identifier: Option<&'a str>,
}

/// Port of `IssueRecentVisitSerializer.to_representation`
/// (`workspace.py:249-273`).
pub fn issue_visit_to_representation<'a>(row: &'a IssueVisitRow<'a>) -> IssueVisitView<'a> {
    IssueVisitView {
        id: row.id,
        name: row.name,
        state: row.state,
        priority: row.priority,
        assignees: row.assignees.to_vec(),
        issue_type: row.issue_type,
        sequence_id: row.sequence_id,
        project_id: row.project_id,
        project_identifier: row.project_identifier,
    }
}

/// One `Project` row for [`project_visit_to_representation`].
/// `get_project_members` (`workspace.py:282-287`) selects
/// `ProjectMember(project_id, member__is_bot=False, is_active=True)`
/// member ids; it returns the raw `QuerySet` (BUG-queryset), which DRF
/// renders as a plain JSON array — the caller resolves the ids.
pub struct ProjectVisitRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_members: &'a [&'a str],
    pub identifier: &'a str,
}

/// `ProjectRecentVisitSerializer` read shape in
/// [`PROJECT_VISIT_WIRE_FIELDS`] order.
#[derive(Debug, Serialize)]
pub struct ProjectVisitView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_members: Vec<&'a str>,
    pub identifier: &'a str,
}

/// Port of `ProjectRecentVisitSerializer.to_representation`
/// (`workspace.py:275-287`).
pub fn project_visit_to_representation<'a>(row: &'a ProjectVisitRow<'a>) -> ProjectVisitView<'a> {
    ProjectVisitView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_members: row.project_members.to_vec(),
        identifier: row.identifier,
    }
}

/// One `Page` row for [`page_visit_to_representation`]: `project_id` is
/// [`page_visit_project_id`]'s resolved value; `project_identifier` is
/// the first related project's identifier or `None`
/// (`get_project_identifier`, `:308-311`).
pub struct PageVisitRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_id: Option<&'a str>,
    pub owned_by: &'a str,
    pub project_identifier: Option<&'a str>,
}

/// `PageRecentVisitSerializer` read shape in [`PAGE_VISIT_WIRE_FIELDS`]
/// order.
#[derive(Debug, Serialize)]
pub struct PageVisitView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub logo_props: &'a Value,
    pub project_id: Option<&'a str>,
    pub owned_by: &'a str,
    pub project_identifier: Option<&'a str>,
}

/// Port of `PageRecentVisitSerializer.to_representation`
/// (`workspace.py:290-311`).
pub fn page_visit_to_representation<'a>(row: &'a PageVisitRow<'a>) -> PageVisitView<'a> {
    PageVisitView {
        id: row.id,
        name: row.name,
        logo_props: row.logo_props,
        project_id: row.project_id,
        owned_by: row.owned_by,
        project_identifier: row.project_identifier,
    }
}

/// One `user_recent_visits` row for [`recent_visit_to_representation`]:
/// `entity_identifier` is nullable (`UUIDField(null=True)`,
/// `recent_visit.py:23`); `entity_data` is [`recent_visit_entity_data`]'s
/// resolved value.
pub struct RecentVisitRow<'a> {
    pub id: &'a str,
    pub entity_name: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<Value>,
    pub visited_at: &'a str,
}

/// `WorkspaceRecentVisitSerializer` read shape in
/// [`RECENT_VISIT_WIRE_FIELDS`] order.
#[derive(Debug, Serialize)]
pub struct RecentVisitView<'a> {
    pub id: &'a str,
    pub entity_name: &'a str,
    pub entity_identifier: Option<&'a str>,
    pub entity_data: Option<Value>,
    pub visited_at: &'a str,
}

/// Port of `WorkspaceRecentVisitSerializer.to_representation`
/// (`workspace.py:323-344`).
pub fn recent_visit_to_representation<'a>(row: &'a RecentVisitRow<'a>) -> RecentVisitView<'a> {
    RecentVisitView {
        id: row.id,
        entity_name: row.entity_name,
        entity_identifier: row.entity_identifier,
        entity_data: row.entity_data.clone(),
        visited_at: row.visited_at,
    }
}

// ================================================================
// WorkspaceHomePreferenceSerializer (workspace.py:347-352)
// ================================================================

/// `WorkspaceHomePreferenceSerializer.Meta.fields` (`workspace.py:350`),
/// wire order. The declared `BaseSerializer.id` is dropped (explicit
/// `Meta.fields` lists win over declared fields — verified live: exactly
/// these 3 keys render).
pub const HOME_PREF_WIRE_FIELDS: [&str; 3] = ["key", "is_enabled", "sort_order"];

/// `WorkspaceHomePreferenceSerializer.Meta.read_only_fields`
/// (`workspace.py:351`). None of these are in `Meta.fields`
/// (BUG-read-only-extras); DRF ignores them silently — carried verbatim.
pub const HOME_PREF_READ_ONLY_FIELDS: &[&str] = &["workspace", "created_by", "updated_by"];

/// One `workspace_home_preferences` row for
/// [`home_pref_to_representation`].
pub struct HomePrefRow<'a> {
    pub key: &'a str,
    pub is_enabled: bool,
    pub sort_order: f64,
}

/// `WorkspaceHomePreferenceSerializer` read shape in
/// [`HOME_PREF_WIRE_FIELDS`] order.
#[derive(Debug, Serialize)]
pub struct HomePrefView<'a> {
    pub key: &'a str,
    pub is_enabled: bool,
    pub sort_order: f64,
}

/// Port of `WorkspaceHomePreferenceSerializer.to_representation`
/// (`workspace.py:347-352`).
pub fn home_pref_to_representation<'a>(row: &'a HomePrefRow<'a>) -> HomePrefView<'a> {
    HomePrefView {
        key: row.key,
        is_enabled: row.is_enabled,
        sort_order: row.sort_order,
    }
}

// ================================================================
// StickySerializer (workspace.py:354-377)
// ================================================================

/// `StickySerializer` wire keys in output order
/// (`workspace.py:354-359`, `fields = "__all__"`: declared `id` first via
/// `BaseSerializer`, then non-relational model fields, then forward
/// relations — verified live).
pub const STICKY_WIRE_FIELDS: [&str; 17] = [
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "description",
    "description_html",
    "description_stripped",
    "description_binary",
    "logo_props",
    "color",
    "background_color",
    "sort_order",
    "created_by",
    "updated_by",
    "workspace",
    "owner",
];

/// `StickySerializer.Meta.read_only_fields` (`workspace.py:358`).
/// `id`/`created_at`/`updated_at` are additionally read-only via
/// `BaseSerializer`/auto fields, and `description_binary` via its
/// `BinaryField → ModelField(read_only=True)` mapping (verified live).
pub const STICKY_READ_ONLY_FIELDS: &[&str] = &["workspace", "owner"];

/// `StickySerializer.Meta.extra_kwargs` (`workspace.py:359`): `name` is
/// not required (absent key validates; the model default/null applies on
/// save, which the models layer owns).
pub const STICKY_NAME_REQUIRED: bool = false;

/// Every serializer field ignored on write, re-exported from the v1
/// transcription (single owner): the declared read-only pair plus `id` /
/// `created_at` / `updated_at` (DRF read-only auto-fields) plus
/// `description_binary` (`BinaryField → ModelField(read_only=True)`).
/// Same model, same `Meta`: identical set (verified live for this
/// serializer too).
pub use sticky_kernel::STICKY_INPUT_IGNORED_FIELDS;

/// The writable-fields projection of `to_internal_value`, re-exported
/// from the v1 transcription (single owner): read-only keys are silently
/// dropped (verified live — no `"This field is read-only."` error exists
/// in DRF for input); every other key passes through untouched.
pub use sticky_kernel::strip_ignored_sticky_keys;

/// `StickySerializer.validate()` (`workspace.py:361-376`) over
/// post-field-validation values: `None` means the key is absent (or was
/// dropped as read-only — the BUG-binary-dead case), `Some` carries the
/// value. The HTML/binary verdicts reuse the v1 kernels (single owner);
/// only the error bodies are this layer's own: the *raised* details
/// (F-W24-03 form), not the live list-wrapped envelopes (see module
/// docs). Sanitized HTML replaces the input when cleaning succeeds
/// (`:368-369`).
pub fn sticky_validate_descriptions(
    html: Option<&str>,
    binary: Option<&str>,
) -> Result<Option<String>, Value> {
    match sticky_kernel::validate_sticky_descriptions(html, binary) {
        Ok(sanitized) => Ok(sanitized),
        Err(sticky_kernel::StickyValidateError::HtmlInvalid) => Err(sticky_html_invalid_body()),
        Err(sticky_kernel::StickyValidateError::BinaryInvalid) => Err(sticky_binary_invalid_body()),
    }
}

/// Raised body for rejected HTML (`workspace.py:366`).
pub fn sticky_html_invalid_body() -> Value {
    Value::Object(
        [(
            "error".to_owned(),
            Value::String(sticky_kernel::HTML_INVALID_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// Raised body for rejected binary (`workspace.py:374`; the validator's
/// own message is discarded and replaced with this literal — same as the
/// v1 raise site).
pub fn sticky_binary_invalid_body() -> Value {
    Value::Object(
        [(
            "description_binary".to_owned(),
            Value::String(sticky_kernel::BINARY_INVALID_MESSAGE.to_owned()),
        )]
        .into_iter()
        .collect(),
    )
}

/// `description_binary` read rendering: DRF's `ModelField` returns
/// `model_field.value_to_string(obj)` for non-protected types, and
/// Django's `BinaryField.value_to_string` is standard-base64 ASCII
/// (verified live: `b"\x89PNG…"` renders `"iVBORw0…"`);
/// `None` is a protected type and renders `null`.
pub fn sticky_binary_to_string(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// One `stickies` row for [`sticky_to_representation`]:
/// `description`/`logo_props` pass through by reference;
/// `description_binary` crosses already base64-rendered (see
/// [`sticky_binary_to_string`]); `sort_order` is a float.
pub struct StickyRow<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: Option<&'a str>,
    pub description: &'a Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub logo_props: &'a Value,
    pub color: Option<&'a str>,
    pub background_color: Option<&'a str>,
    pub sort_order: f64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub owner: &'a str,
}

/// `StickySerializer` read shape in [`STICKY_WIRE_FIELDS`] order.
#[derive(Debug, Serialize)]
pub struct StickyView<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: Option<&'a str>,
    pub description: &'a Value,
    pub description_html: &'a str,
    pub description_stripped: Option<&'a str>,
    pub description_binary: Option<&'a str>,
    pub logo_props: &'a Value,
    pub color: Option<&'a str>,
    pub background_color: Option<&'a str>,
    pub sort_order: f64,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub owner: &'a str,
}

/// Port of `StickySerializer.to_representation`
/// (`workspace.py:354-360`, default `ModelSerializer` rendering).
pub fn sticky_to_representation<'a>(row: &'a StickyRow<'a>) -> StickyView<'a> {
    StickyView {
        id: row.id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        description: row.description,
        description_html: row.description_html,
        description_stripped: row.description_stripped,
        description_binary: row.description_binary,
        logo_props: row.logo_props,
        color: row.color,
        background_color: row.background_color,
        sort_order: row.sort_order,
        created_by: row.created_by,
        updated_by: row.updated_by,
        workspace: row.workspace,
        owner: row.owner,
    }
}

// ================================================================
// WorkspaceUserPreferenceSerializer (workspace.py:379-383)
// ================================================================

/// `WorkspaceUserPreferenceSerializer.Meta.fields` (`workspace.py:381`),
/// wire order (declared `id` dropped — same explicit-list rule as
/// [`HOME_PREF_WIRE_FIELDS`], verified live: exactly these 3 keys).
pub const USER_PREF_WIRE_FIELDS: [&str; 3] = ["key", "is_pinned", "sort_order"];

/// `WorkspaceUserPreferenceSerializer.Meta.read_only_fields`
/// (`workspace.py:382`). None of these are in `Meta.fields`
/// (BUG-read-only-extras); DRF ignores them silently — carried verbatim.
pub const USER_PREF_READ_ONLY_FIELDS: &[&str] = &["workspace", "created_by", "updated_by"];

/// One `workspace_user_preferences` row for
/// [`user_pref_to_representation`].
pub struct UserPrefRow<'a> {
    pub key: &'a str,
    pub is_pinned: bool,
    pub sort_order: f64,
}

/// `WorkspaceUserPreferenceSerializer` read shape in
/// [`USER_PREF_WIRE_FIELDS`] order.
#[derive(Debug, Serialize)]
pub struct UserPrefView<'a> {
    pub key: &'a str,
    pub is_pinned: bool,
    pub sort_order: f64,
}

/// Port of `WorkspaceUserPreferenceSerializer.to_representation`
/// (`workspace.py:379-383`).
pub fn user_pref_to_representation<'a>(row: &'a UserPrefRow<'a>) -> UserPrefView<'a> {
    UserPrefView {
        key: row.key,
        is_pinned: row.is_pinned,
        sort_order: row.sort_order,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    static FIXTURE: &str =
        include_str!("../../../../fixtures/app_workspace/serializers/extras.golden.json");

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    fn case<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|case| case["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("fixture lacks case {name}"))
    }

    fn fixture_str_list(section: &Value, key: &str) -> Vec<String> {
        section[key]
            .as_array()
            .unwrap_or_else(|| panic!("{key} array"))
            .iter()
            .map(|key| key.as_str().expect("key string").to_string())
            .collect()
    }

    /// Top-level JSON key order of a struct's serialization, read off the
    /// serialized string: struct serialization always emits declaration
    /// order.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    /// Byte-identical replay: `serde_json` builds with `preserve_order`,
    /// so the parsed golden keeps document order and the compact forms
    /// must match exactly.
    fn assert_byte_replay<T: serde::Serialize>(produced: &T, expected: &Value) {
        assert_eq!(
            serde_json::to_string(produced).expect("serializes"),
            serde_json::to_string(expected).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    #[test]
    fn fixture_serializer_metas_match() {
        let fixture = fixture();
        let serializers = &fixture["serializers"];
        // Explicit `Meta.fields` lists land verbatim in the wire consts.
        assert_eq!(
            fixture_str_list(&serializers["IssueRecentVisitSerializer"], "fields"),
            ISSUE_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["ProjectRecentVisitSerializer"], "fields"),
            PROJECT_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["PageRecentVisitSerializer"], "fields"),
            PAGE_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["WorkspaceRecentVisitSerializer"], "fields"),
            RECENT_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["WorkspaceHomePreferenceSerializer"], "fields"),
            HOME_PREF_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["WorkspaceUserPreferenceSerializer"], "fields"),
            USER_PREF_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        // `__all__` markers.
        assert_eq!(
            serializers["WorkspaceUserLinkSerializer"]["fields"],
            json!("__all__")
        );
        assert_eq!(serializers["StickySerializer"]["fields"], json!("__all__"));
        // Read-only lists, including the ignored extras (BUG).
        assert_eq!(
            fixture_str_list(&serializers["WorkspaceUserLinkSerializer"], "read_only"),
            USER_LINK_READ_ONLY_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["StickySerializer"], "read_only"),
            STICKY_READ_ONLY_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(&serializers["WorkspaceRecentVisitSerializer"], "read_only"),
            RECENT_VISIT_READ_ONLY_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(
                &serializers["WorkspaceHomePreferenceSerializer"],
                "read_only"
            ),
            HOME_PREF_READ_ONLY_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            fixture_str_list(
                &serializers["WorkspaceUserPreferenceSerializer"],
                "read_only"
            ),
            USER_PREF_READ_ONLY_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        // Sticky `name` not required.
        assert_eq!(
            serializers["StickySerializer"]["extra_kwargs"]["name"]["required"],
            json!(false)
        );
        assert_eq!(
            STICKY_NAME_REQUIRED,
            serializers["StickySerializer"]["extra_kwargs"]["name"]["required"]
                .as_bool()
                .expect("bool")
        );
        // Recent-visit serializers are plain `ModelSerializer`s (no
        // `BaseSerializer` id rule); the rest derive from `BaseSerializer`.
        for name in [
            "IssueRecentVisitSerializer",
            "ProjectRecentVisitSerializer",
            "PageRecentVisitSerializer",
        ] {
            assert!(
                serializers[name]["base"]
                    .as_str()
                    .expect("base string")
                    .contains("plain ModelSerializer"),
                "{name} base"
            );
        }
    }

    #[test]
    fn all_wire_orders_match_live() {
        // `__all__` expansion (declared + non-relational + forward
        // relations) captured live from the project venv.
        assert_eq!(
            USER_LINK_WIRE_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "title",
                "url",
                "metadata",
                "created_by",
                "updated_by",
                "workspace",
                "project",
                "owner"
            ]
        );
        assert_eq!(
            STICKY_WIRE_FIELDS,
            [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "description",
                "description_html",
                "description_stripped",
                "description_binary",
                "logo_props",
                "color",
                "background_color",
                "sort_order",
                "created_by",
                "updated_by",
                "workspace",
                "owner"
            ]
        );
    }

    #[test]
    fn views_emit_wire_order() {
        let metadata = json!({"a": 1});
        let row_for_link = UserLinkRow {
            id: "11111111-1111-1111-1111-111111111111",
            created_at: "2026-01-02T03:04:05Z",
            updated_at: "2026-01-03T04:05:06Z",
            deleted_at: None,
            title: Some("T"),
            url: "http://e.com/x",
            metadata: &metadata,
            created_by: None,
            updated_by: Some("44444444-4444-4444-4444-444444444444"),
            workspace: "22222222-2222-2222-2222-222222222222",
            project: None,
            owner: "33333333-3333-3333-3333-333333333333",
        };
        let link = user_link_to_representation(&row_for_link);
        assert_eq!(
            serialized_keys(&link),
            USER_LINK_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let description = json!({"x": [1, 2]});
        let logo = json!({});
        let row_for_sticky = StickyRow {
            id: "55555555-5555-5555-5555-555555555555",
            created_at: "2026-01-02T03:04:05Z",
            updated_at: "2026-01-03T04:05:06Z",
            deleted_at: None,
            name: None,
            description: &description,
            description_html: "<p>hi</p>",
            description_stripped: Some("hi"),
            description_binary: Some("iVBORw0KGgpyZXN0LWJ5dGVz"),
            logo_props: &logo,
            color: None,
            background_color: Some("#fff"),
            sort_order: 65535.0,
            created_by: None,
            updated_by: None,
            workspace: "22222222-2222-2222-2222-222222222222",
            owner: "33333333-3333-3333-3333-333333333333",
        };
        let sticky = sticky_to_representation(&row_for_sticky);
        assert_eq!(
            serialized_keys(&sticky),
            STICKY_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let assignees: &[&str] = &["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"];
        let row_for_issue = IssueVisitRow {
            id: "id",
            name: "n",
            state: None,
            priority: Some("urgent"),
            assignees,
            issue_type: None,
            sequence_id: 7,
            project_id: "p",
            project_identifier: Some("P"),
        };
        let issue = issue_visit_to_representation(&row_for_issue);
        assert_eq!(
            serialized_keys(&issue),
            ISSUE_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let members: &[&str] = &[];
        let row_for_project = ProjectVisitRow {
            id: "id",
            name: "n",
            logo_props: &logo,
            project_members: members,
            identifier: "P",
        };
        let project = project_visit_to_representation(&row_for_project);
        assert_eq!(
            serialized_keys(&project),
            PROJECT_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let row_for_page = PageVisitRow {
            id: "id",
            name: "n",
            logo_props: &logo,
            project_id: None,
            owned_by: "o",
            project_identifier: None,
        };
        let page = page_visit_to_representation(&row_for_page);
        assert_eq!(
            serialized_keys(&page),
            PAGE_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let row_for_recent = RecentVisitRow {
            id: "id",
            entity_name: "issue",
            entity_identifier: None,
            entity_data: None,
            visited_at: "2026-01-02T03:04:05Z",
        };
        let recent = recent_visit_to_representation(&row_for_recent);
        assert_eq!(
            serialized_keys(&recent),
            RECENT_VISIT_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let row_for_home = HomePrefRow {
            key: "recents",
            is_enabled: true,
            sort_order: 65535.0,
        };
        let home = home_pref_to_representation(&row_for_home);
        assert_eq!(
            serialized_keys(&home),
            HOME_PREF_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );

        let row_for_user_pref = UserPrefRow {
            key: "views",
            is_pinned: false,
            sort_order: 100.0,
        };
        let user_pref = user_pref_to_representation(&row_for_user_pref);
        assert_eq!(
            serialized_keys(&user_pref),
            USER_PREF_WIRE_FIELDS
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn link_read_replays_live_row() {
        // Exact `.data` of an in-memory `WorkspaceUserLink`, captured live.
        let metadata = json!({"a": 1});
        let row_for_view = UserLinkRow {
            id: "11111111-1111-1111-1111-111111111111",
            created_at: "2026-01-02T03:04:05Z",
            updated_at: "2026-01-03T04:05:06Z",
            deleted_at: None,
            title: Some("T"),
            url: "http://e.com/x",
            metadata: &metadata,
            created_by: None,
            updated_by: Some("44444444-4444-4444-4444-444444444444"),
            workspace: "22222222-2222-2222-2222-222222222222",
            project: None,
            owner: "33333333-3333-3333-3333-333333333333",
        };
        let view = user_link_to_representation(&row_for_view);
        assert_byte_replay(
            &view,
            &json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "created_at": "2026-01-02T03:04:05Z",
                "updated_at": "2026-01-03T04:05:06Z",
                "deleted_at": null,
                "title": "T",
                "url": "http://e.com/x",
                "metadata": {"a": 1},
                "created_by": null,
                "updated_by": "44444444-4444-4444-4444-444444444444",
                "workspace": "22222222-2222-2222-2222-222222222222",
                "project": null,
                "owner": "33333333-3333-3333-3333-333333333333"
            }),
        );
    }

    #[test]
    fn sticky_read_replays_live_row() {
        // Exact `.data` of an in-memory `Sticky`, captured live
        // (base64 `description_binary`, float `sort_order`).
        let description = json!({"x": [1, 2]});
        let logo = json!({});
        let row_for_view = StickyRow {
            id: "55555555-5555-5555-5555-555555555555",
            created_at: "2026-01-02T03:04:05Z",
            updated_at: "2026-01-03T04:05:06Z",
            deleted_at: None,
            name: None,
            description: &description,
            description_html: "<p>hi</p>",
            description_stripped: Some("hi"),
            description_binary: Some("iVBORw0KGgpyZXN0LWJ5dGVz"),
            logo_props: &logo,
            color: None,
            background_color: Some("#fff"),
            sort_order: 65535.0,
            created_by: None,
            updated_by: None,
            workspace: "22222222-2222-2222-2222-222222222222",
            owner: "33333333-3333-3333-3333-333333333333",
        };
        let view = sticky_to_representation(&row_for_view);
        assert_byte_replay(
            &view,
            &json!({
                "id": "55555555-5555-5555-5555-555555555555",
                "created_at": "2026-01-02T03:04:05Z",
                "updated_at": "2026-01-03T04:05:06Z",
                "deleted_at": null,
                "name": null,
                "description": {"x": [1, 2]},
                "description_html": "<p>hi</p>",
                "description_stripped": "hi",
                "description_binary": "iVBORw0KGgpyZXN0LWJ5dGVz",
                "logo_props": {},
                "color": null,
                "background_color": "#fff",
                "sort_order": 65535.0,
                "created_by": null,
                "updated_by": null,
                "workspace": "22222222-2222-2222-2222-222222222222",
                "owner": "33333333-3333-3333-3333-333333333333"
            }),
        );
    }

    #[test]
    fn link_prefix_golden() {
        let fixture = fixture();
        let golden = case(&fixture, "link prefixes http://");
        let mut input = Map::from_iter([("url".to_owned(), golden["input"]["url"].clone())]);
        prefix_user_link_url(&mut input).expect("prefixes");
        assert_eq!(
            input["url"], golden["output_url"],
            "golden output_url replay"
        );
    }

    #[test]
    fn link_prefix_edges() {
        // Missing key: untouched (required check fires later).
        let mut input = Map::new();
        prefix_user_link_url(&mut input).expect("missing ok");
        assert!(input.get("url").is_none());
        // Falsy values pass through untouched.
        for falsy in [
            json!(""),
            json!(null),
            json!(false),
            json!(0),
            json!([]),
            json!({}),
        ] {
            let mut input = Map::from_iter([("url".to_owned(), falsy.clone())]);
            prefix_user_link_url(&mut input).expect("falsy ok");
            assert_eq!(input["url"], falsy, "falsy passes through");
        }
        // Schemed values pass through untouched.
        for schemed in ["http://e.com", "https://e.com/x"] {
            let mut input = Map::from_iter([("url".to_owned(), json!(schemed))]);
            prefix_user_link_url(&mut input).expect("schemed ok");
            assert_eq!(input["url"], json!(schemed));
        }
        // Case-sensitive prefix test: `HTTP://…` is double-prefixed live.
        let mut input = Map::from_iter([("url".to_owned(), json!("HTTP://e.com/a"))]);
        prefix_user_link_url(&mut input).expect("prefixes");
        assert_eq!(input["url"], json!("http://HTTP://e.com/a"));
        // Truthy non-strings have no `.startswith` → AttributeError (500).
        for truthy in [json!(5), json!(true), json!(["x"]), json!({"u": 1})] {
            let mut input = Map::from_iter([("url".to_owned(), truthy)]);
            assert_eq!(
                prefix_user_link_url(&mut input),
                Err(LinkPrefixError::AttributeError)
            );
        }
    }

    #[test]
    fn link_bad_url_golden() {
        let fixture = fixture();
        let golden = case(&fixture, "link bad URL");
        let input = golden["input"]["url"].as_str().expect("url string");
        assert_eq!(
            validate_user_link_url(input),
            Err(golden["error"].clone()),
            "golden error replay"
        );
        assert_eq!(
            serde_json::to_string(&invalid_user_link_url_body()).expect("serializes"),
            r#"{"error":"Invalid URL format."}"#
        );
    }

    #[test]
    fn link_dup_goldens() {
        let fixture = fixture();
        let create = case(&fixture, "link dup on create");
        let update = case(&fixture, "link dup on update (excludes self pk)");
        // Both sites share the one message.
        assert_eq!(create["error"], update["error"]);
        assert_eq!(duplicate_user_link_body(), create["error"]);
        assert_eq!(
            serde_json::to_string(&duplicate_user_link_body()).expect("serializes"),
            r#"{"error":"URL already exists for this workspace and owner"}"#
        );
        // Create keeps validated_data as-is (BUG-dup-lookup: read-only
        // ids arrive as None without save() kwargs), no pk exclusion.
        assert_eq!(
            user_link_create_lookup(Some("http://e.com"), None, None),
            UserLinkDupLookup {
                url: Some("http://e.com"),
                workspace_id: None,
                owner_id: None,
                exclude_pk: None,
            }
        );
        // Update binds the instance scope and excludes self.
        assert_eq!(
            user_link_update_lookup(Some("http://e.com"), "ws", "owner", "pk"),
            UserLinkDupLookup {
                url: Some("http://e.com"),
                workspace_id: Some("ws"),
                owner_id: Some("owner"),
                exclude_pk: Some("pk"),
            }
        );
        // A PATCH omitting `url` looks up `None` (matches nothing).
        assert_eq!(user_link_update_lookup(None, "ws", "owner", "pk").url, None);
    }

    #[test]
    fn dispatch_golden() {
        let fixture = fixture();
        let golden = case(&fixture, "recent-visit dispatch");
        let map = &golden["output_map"];
        for entity_type in ["issue", "page", "project"] {
            let entity = recent_visit_entity(entity_type).expect("dispatches");
            assert_eq!(
                format!("({},{})", entity.model_name(), entity.serializer_name()),
                map[entity_type].as_str().expect("map string"),
                "{entity_type} pair"
            );
        }
        // Unknown entities and missing rows both render None.
        assert_eq!(recent_visit_entity("other"), None);
        assert_eq!(
            recent_visit_entity_data("other", Some(json!({"id": "x"}))),
            None
        );
        assert_eq!(
            recent_visit_entity_data("issue", None),
            None,
            "DoesNotExist → None"
        );
        assert_eq!(
            recent_visit_entity_data("issue", Some(json!({"id": "x"}))),
            Some(json!({"id": "x"}))
        );
    }

    #[test]
    fn dispatch_edges() {
        // Case-sensitive dispatch (verified live).
        assert_eq!(recent_visit_entity("ISSUE"), None);
        assert_eq!(recent_visit_entity("Issue"), None);
        assert_eq!(recent_visit_entity(""), None);
        assert_eq!(recent_visit_entity("view"), None);
        assert_eq!(recent_visit_entity("cycle"), None);
    }

    #[test]
    fn page_visit_project_id_branches() {
        // Annotated attribute wins even when its value is None…
        assert_eq!(page_visit_project_id(Some(None), Some("fb")), None);
        assert_eq!(
            page_visit_project_id(Some(Some("ann")), Some("fb")),
            Some("ann")
        );
        // …otherwise the first related project id (None when none).
        assert_eq!(page_visit_project_id(None, Some("fb")), Some("fb"));
        assert_eq!(page_visit_project_id(None, None), None);
    }

    #[test]
    fn sticky_goldens() {
        let fixture = fixture();
        // Rejected HTML raises the bare detail (F-W24-03 form).
        let bad_html = case(&fixture, "sticky bad html");
        assert_eq!(
            sticky_validate_descriptions(Some(&"x".repeat(10 * 1024 * 1024 + 1)), None),
            Err(bad_html["error"].clone())
        );
        assert_eq!(
            serde_json::to_string(&sticky_html_invalid_body()).expect("serializes"),
            r#"{"error":"html content is not valid"}"#
        );
        // Rejected binary raises under its own key.
        let bad_binary = case(&fixture, "sticky bad binary");
        assert_eq!(
            sticky_validate_descriptions(Some("<p>ok</p>"), Some("not-base64!!!___")),
            Err(bad_binary["error"].clone())
        );
        assert_eq!(
            serde_json::to_string(&sticky_binary_invalid_body()).expect("serializes"),
            r#"{"description_binary":"Invalid binary data"}"#
        );
        // Sanitized HTML replaces the input (verified live).
        assert_eq!(
            sticky_validate_descriptions(Some("<p>hi</p><script>e()</script>"), None),
            Ok(Some("<p>hi</p>".to_owned()))
        );
        // Falsy inputs skip validation and pass through unchanged.
        assert_eq!(
            sticky_validate_descriptions(Some(""), None),
            Ok(Some(String::new()))
        );
        assert_eq!(sticky_validate_descriptions(None, None), Ok(None));
        // The binary arm is dead through the serializer (read-only drop):
        // the projection removes the key before validate() runs.
        let projected = strip_ignored_sticky_keys(&Map::from_iter([
            ("description_html".to_owned(), json!("<p>hi</p>")),
            ("description_binary".to_owned(), json!("not-base64!!!___")),
            (
                "workspace".to_owned(),
                json!("22222222-2222-2222-2222-222222222222"),
            ),
        ]));
        assert_eq!(
            projected,
            Map::from_iter([("description_html".to_owned(), json!("<p>hi</p>"))])
        );
        assert_eq!(
            sticky_validate_descriptions(
                projected.get("description_html").and_then(Value::as_str),
                projected.get("description_binary").and_then(Value::as_str),
            ),
            Ok(Some("<p>hi</p>".to_owned()))
        );
    }

    #[test]
    fn sticky_binary_read_matches_live() {
        // Base64 rendering captured live from `.data`.
        assert_eq!(
            sticky_binary_to_string(b"\x89PNG\r\n\x1a\nrest-bytes"),
            "iVBORw0KGgpyZXN0LWJ5dGVz"
        );
        assert_eq!(sticky_binary_to_string(b"hi"), "aGk=");
    }

    #[test]
    fn django_url_truth_table() {
        // `binary_search` needs a sorted table; guard it against edits.
        assert!(NFKC_FORBIDDEN.windows(2).all(|pair| pair[0] < pair[1]));
        // `(input, Django URLValidator accepts)` — every verdict captured
        // by executing the live validator (Django 4.2, Python 3.12).
        let vectors: &[(&str, bool)] = &[
            ("http://", false),
            ("http://example.com/x", true),
            ("https://a.com/y", true),
            ("example.com/x", false),
            ("", false),
            ("HTTP://example.com/", true),
            ("Http://example.com/", true),
            ("ftp://example.com/f", true),
            ("ftps://example.com/f", true),
            ("FTP://EXAMPLE.COM/F", true),
            ("javascript:alert(1)", false),
            ("http:/example.com/", false),
            ("http:example.com", false),
            ("://example.com/", false),
            ("http://", false),
            ("https://", false),
            (" http://example.com/", false),
            ("http://example.com/ ", false),
            ("mailto:x@y.com", false),
            ("data:text/plain,hi", false),
            ("http+unix://x/y", false),
            ("http://exa\tmple.com/", false),
            ("http://example.com/\n", false),
            ("http://example.com/a\rb", false),
            ("http://user@example.com/", true),
            ("http://user:pass@example.com/", true),
            ("http://user:@example.com/", true),
            ("http://:pass@example.com/", false),
            ("http://@example.com/", false),
            ("http://u:p@ss@example.com/", false),
            ("http://us er@example.com/", false),
            ("http://u:s:s@example.com/", false),
            ("http://a@b@c.example.com/", false),
            ("http://user@example.com:8080/x", true),
            ("http://1.2.3.4/", true),
            ("http://0.0.0.0/", true),
            ("http://255.255.255.255/", true),
            ("http://256.1.1.1/", false),
            ("http://01.2.3.4/", false),
            ("http://1.2.3/", false),
            ("http://1.2.3.4.5/", false),
            ("http://1.2.3.4a/", false),
            ("http://999.999.999.999/", false),
            ("http://[::1]/", true),
            ("http://[::1]:8080/x", true),
            ("http://[2001:db8::1]/", true),
            ("http://[2001:DB8::1]/", true),
            ("http://[::ffff:1.2.3.4]/", true),
            ("http://[::1::2]/", false),
            ("http://[gg]/", false),
            ("http://[]/", false),
            ("http://[::1", false),
            ("http://::1]/", false),
            ("http://[::1]extra/", false),
            ("http://[fe80::1%eth0]/", false),
            ("http://[1:2:3:4:5:6:7:8]/", true),
            ("http://[1:2:3:4:5:6:7:8:9]/", false),
            ("http://localhost/", true),
            ("http://LOCALHOST:3000/x", true),
            ("http://localhostx/", false),
            ("http://a.bc/", true),
            ("http://a.b/", false),
            ("http://example.123/", false),
            ("http://example.c0m/", false),
            ("http://-example.com/", false),
            ("http://example-.com/", false),
            ("http://ex-ample.com/", true),
            ("http://e.com/", true),
            ("http://1.com/", true),
            ("http://a/", false),
            ("http://com/", false),
            ("http://example.com./", true),
            ("http://example..com/", false),
            ("http://.example.com/", false),
            ("http://example.com../", false),
            ("http://xn--bcher-kva.example/", true),
            ("http://xn--/", false),
            ("http://ab--cd.com/", true),
            (
                "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/",
                true,
            ),
            (
                "http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com/",
                false,
            ),
            (
                "http://a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.com/",
                true,
            ),
            ("http://under_score.com/", false),
            ("http://ex ample.com/", false),
            ("http://example.com:80/", true),
            ("http://example.com:/", false),
            ("http://example.com:123456/", false),
            ("http://example.com:abc/", false),
            ("http://example.com:12a/", false),
            ("http://example.com:0/", true),
            ("http://a:b:c/", false),
            ("http://example.com:80:90/", false),
            ("http://example.com", true),
            ("http://example.com?x=1&y=2", true),
            ("http://example.com#frag", true),
            ("http://example.com/a/b/../c", true),
            ("http://example.com/a b", false),
            ("http://example.com/a b", false),
            ("http://example.com/a\u{b}b", false),
            ("http://example.com/a\u{c}b", false),
            ("http://example.com/a\u{1c}b", false),
            ("http://example.com/ab", false),
            ("http:///path", false),
            ("http://?x", false),
            ("http://#f", false),
            ("http://bücher.de/", true),
            ("http://BÜCHER.DE/", true),
            ("http://中文.com/", true),
            ("http://áb.com/", true),
            ("http://😀.com/", true),
            ("http://user@bücher.de/", true),
            ("http://example.com:8080", true),
            ("https://example.com./a", true),
            ("http://example.com./", true),
            ("HtTpS://MiXeD.CoM/PaTh", true),
            ("http://example.com/%20x", true),
            ("http://[::ffff:999.999.999.999]/", false),
            ("http://münchen.de:8080/p?q=1#f", true),
            ("http://münchen.de.:8080/", true),
            ("http://u[x@e.com/", false),
            ("http://e.com/[x", true),
            ("http://u]y@e.com/", false),
            ("http://e.com/a[b", true),
            ("http://[::1]x/", false),
            ("http://x[::1]/", false),
            ("http://e.com././", true),
            ("http://?@/", false),
            ("http://:@/", false),
            ("http://u:p@/", false),
            ("http://u@[::1]/", true),
            ("http://u@[::1]:80/", true),
            ("http://u@[gg]/", false),
            ("http://e.com/a[b/c]d", true),
            ("ftp://u:p@h.io:21/x", true),
            ("http://e.com/%", true),
            ("http://e.c_m/", false),
            ("http://_dmarc.e.com/", false),
            ("http://e.com/a%20b", true),
            ("http://e.com/é", true),
            ("http://-x.io/", false),
            ("http://x-.io/", false),
            ("http://xn--a.io/", true),
            ("http://a.xn--p1ai/", true),
            ("http://[::FFFF:1.2.3.4]/", true),
            ("http://[::ffff:1.2.3.256]/", false),
            ("HTTP://E.COM", true),
            ("http://e.com:00080/", true),
            ("http://e.com:+80/", false),
            ("http://e.com:-80/", false),
            ("http://e.com: 80/", false),
            ("http://[::1]:x/", false),
            ("http://u@h/p?a=b@c", false),
            ("http://h/@u", false),
            ("ws://e.com/", false),
            ("wss://e.com/", false),
            ("http://e.com/ x/", false),
            ("http://e.com/\u{7f}/", true),
            ("http://\u{7f}e.com/", false),
            ("http://e.com/\u{1}", true),
            ("http://e.c/", false),
            ("http://e.co/", true),
            ("http://u:p@[::1]:8080/x", true),
            ("http://[0:0:0:0:0:0:0:1]/", true),
            ("http://[0:0:0:0:0:0:0:1", false),
            ("http://e.com:80/x?y#z", true),
            ("https://sub.domain.example.co.uk:8443/a?b=c#d", true),
            ("http://u[x]y@e.com/", false),
            ("http://e.com]a[/", false),
            ("http://[::1]]/", false),
            ("http://[[::1]]/", false),
            ("http://[[::1]/", false),
            ("http://[::1[]/", false),
            ("http://u@[::1", false),
            ("http://e.com]/", false),
            ("http://[e.com]/", false),
            ("http://[1.2.3.4]/", false),
            ("http://e＠x.com/", false),
            ("http://e／x.com/", false),
            ("http://e＃x.com/", false),
            ("http://e？x.com/", false),
            ("http://e：x.com/", false),
            ("http://e⁇x.com/", false),
            ("http://ﬁ.com/", true),
            ("http://eﬁx.com/", true),
            ("http://e﹫x.com/", false),
            ("http://e。com/", true),
            ("http://u[]@v1.fe/", true),
            ("http://u[]@e.com/", false),
            ("http://u[ab]@vA9.ZZ/", true),
            ("http://u[]@v1.fe:80/x", true),
            ("http://[v1.fe]/", false),
            ("http://[vG.fe]/", false),
            ("http://u@[v1.fe]/", false),
            ("http://[::ffff:01.2.3.4]/", false),
            ("http://[1::01]/", true),
            ("http://[::01]/", true),
            ("http://[0000::0001]/", true),
            ("http://[fe80::1%eth0]/", false),
            ("http://[1.2.3.4]/", false),
            ("http://[01.2.3.4]/", false),
            ("http://[::ffff:1.2.3.256]/", false),
            ("http://[1:2:3:4:5:6:7:8]/", true),
            ("http://[1:2:3:4:5:6:7]/", false),
            ("http://[1::2::3]/", false),
            ("http://[:1]/", false),
            ("http://[1:]/", false),
            ("http://[1:2:3:4:5:6:1.2.3.4]/", true),
            ("http://[1:2:3:4:5:6:1.2.3.256]/", false),
            ("http://[::ffff:1.2.3]/", false),
            ("http://u[]@[::1]:80/", true),
            ("http://u[v1.fe]@e.com/", false),
            ("http://[v]/", false),
            ("http://[v1.]/", false),
            ("http://[v.1]/", false),
            ("http://a.éa踏/", true),
            ("http://a.é-踏/", true),
            ("http://a.éé-/", false),
        ];
        for (input, expected) in vectors {
            assert_eq!(
                django_url_valid(input),
                *expected,
                "live verdict for {input:?}"
            );
            assert_eq!(
                validate_user_link_url(input).is_ok(),
                *expected,
                "serializer verdict for {input:?}"
            );
        }
        // Length boundaries, built programmatically (char-counted caps).
        let long: Vec<(String, bool)> = vec![
            (format!("http://{}.com/", "a".repeat(2041)), false),
            (format!("http://{}.com/", "a".repeat(2042)), false),
            (format!("http://{}.com/", "é".repeat(2000)), false),
            (format!("http://{}.com/", "a".repeat(250)), false),
            (format!("http://e.com/{}", "p".repeat(2048 - 13)), true),
            (format!("http://e.com/{}", "p".repeat(2049 - 13)), false),
            (format!("http://e.com/{}", "é".repeat(2048 - 13)), true),
            (format!("http://e.com/{}", "é".repeat(2049 - 13)), false),
            (
                format!(
                    "http://{}.{}.{}.{}.com/",
                    "a".repeat(63),
                    "b".repeat(63),
                    "c".repeat(63),
                    "d".repeat(57)
                ),
                true,
            ),
            (
                format!(
                    "http://{}.{}.{}.{}.com/",
                    "a".repeat(63),
                    "b".repeat(63),
                    "c".repeat(63),
                    "d".repeat(58)
                ),
                false,
            ),
            (
                format!(
                    "http://{}.{}.{}.{}.com/",
                    "é".repeat(63),
                    "ü".repeat(63),
                    "ü".repeat(63),
                    "ü".repeat(58)
                ),
                false,
            ),
            (
                format!(
                    "http://{}.{}.{}.{}.com/",
                    "é".repeat(63),
                    "ü".repeat(63),
                    "ü".repeat(63),
                    "ü".repeat(59)
                ),
                false,
            ),
        ];
        for (input, expected) in &long {
            assert_eq!(
                django_url_valid(input),
                *expected,
                "live verdict for {}-char input",
                input.chars().count()
            );
        }
    }
}
