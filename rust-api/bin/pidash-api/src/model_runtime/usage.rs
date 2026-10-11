//! Usage accounting: the accumulator, limit checks, and per-response
//! extraction (`pydantic_ai/usage.py`, `RequestUsage.extract` through
//! genai-prices 0.1.9).
//!
//! Three ported quirks live here, all probed, all covered by golden
//! replay tests below:
//!
//! - Accumulation drops `output_audio_tokens` (`_incr_usage_tokens` never
//!   adds it, so a run's report always shows `0` while the per-response
//!   value lands in `details`).
//! - Extraction fails whole-link on any invalid value (negative, bool,
//!   non-finite) and on any mapped destination outside the eight
//!   `RequestUsage` counters — `output_reasoning_tokens` (present on
//!   every `chat` table) zeroes the whole response's counters whenever a
//!   reasoning model reports `reasoning_tokens`.
//! - Provider selection is URL-pattern-first over the snapshot table in
//!   order, then provider id, then the lane fallback (`openai` /
//!   `anthropic`); every step's failure falls through to zeros+details.

use super::schemas::Golden;
use pidash_jobs::dispatch_execute::UsageLimits;
use serde_json::{Map, Value};

/// A usage counter value: integers stay integers, floats stay floats
/// (providers report ints; floats survive end to end when they appear).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UsageNumber {
    Int(i64),
    Float(f64),
}

impl Default for UsageNumber {
    /// Zero as an integer (fresh counters).
    fn default() -> Self {
        UsageNumber::Int(0)
    }
}

impl UsageNumber {
    /// Numeric addition: int+int stays int (saturating — token counts
    /// cannot reach `i64::MAX` in practice); any float operand yields a
    /// float (`add_usage_values` with JSON inputs, which are never
    /// `Decimal`).
    pub fn add(self, other: UsageNumber) -> UsageNumber {
        match (self, other) {
            (UsageNumber::Int(left), UsageNumber::Int(right)) => {
                UsageNumber::Int(left.saturating_add(right))
            }
            (left, right) => UsageNumber::Float(left.as_f64() + right.as_f64()),
        }
    }

    /// Render as a JSON number.
    pub fn to_json(self) -> Value {
        match self {
            UsageNumber::Int(value) => Value::Number(value.into()),
            UsageNumber::Float(value) => serde_json::Number::from_f64(value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        }
    }

    fn as_f64(self) -> f64 {
        match self {
            UsageNumber::Int(value) => value as f64,
            UsageNumber::Float(value) => value,
        }
    }

    fn as_i64(self) -> i64 {
        match self {
            UsageNumber::Int(value) => value,
            UsageNumber::Float(value) => value as i64,
        }
    }
}

/// Per-response usage (`RequestUsage`): the eight counters plus the
/// lane-specific `details` pre-pass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestUsage {
    pub input_tokens: UsageNumber,
    pub cache_write_tokens: UsageNumber,
    pub cache_read_tokens: UsageNumber,
    pub output_tokens: UsageNumber,
    pub input_audio_tokens: UsageNumber,
    pub cache_audio_read_tokens: UsageNumber,
    pub output_audio_tokens: UsageNumber,
    pub details: Map<String, Value>,
}

impl RequestUsage {
    /// All-zero usage with empty details (the `usage is None` / failed
    /// extraction outcome, before the caller overlays `details`).
    pub fn zeros() -> Self {
        RequestUsage::default()
    }
}

/// Accumulated run usage (`RunUsage`): per-response counters plus the
/// request/tool-call counts the loop maintains.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunUsage {
    pub input_tokens: UsageNumber,
    pub cache_write_tokens: UsageNumber,
    pub cache_read_tokens: UsageNumber,
    pub output_tokens: UsageNumber,
    pub input_audio_tokens: UsageNumber,
    pub cache_audio_read_tokens: UsageNumber,
    /// Always zero: `_incr_usage_tokens` never accumulates the
    /// per-response `output_audio_tokens` (probed quirk).
    pub output_audio_tokens: UsageNumber,
    pub details: Map<String, Value>,
    pub requests: i64,
    pub tool_calls: i64,
}

impl RunUsage {
    /// Fold one response's usage in (`RunUsage.incr`): the six counters
    /// plus the `details` merge. `output_audio_tokens` is deliberately
    /// NOT accumulated (see the field doc).
    pub fn incr(&mut self, response: &RequestUsage) {
        self.input_tokens = self.input_tokens.add(response.input_tokens);
        self.cache_write_tokens = self.cache_write_tokens.add(response.cache_write_tokens);
        self.cache_read_tokens = self.cache_read_tokens.add(response.cache_read_tokens);
        self.output_tokens = self.output_tokens.add(response.output_tokens);
        self.input_audio_tokens = self.input_audio_tokens.add(response.input_audio_tokens);
        self.cache_audio_read_tokens = self
            .cache_audio_read_tokens
            .add(response.cache_audio_read_tokens);
        for (key, value) in &response.details {
            // `if isinstance(value, (int, float))` (`usage.py`,
            // `_incr_usage_tokens`): anything else leaves the key
            // untouched. (Bools are ints: `get(key, 0) + True == 1`.)
            let incoming = match value {
                Value::Number(number) => number.clone(),
                Value::Bool(flag) => i64::from(*flag).into(),
                _ => continue,
            };
            let have = match self.details.remove(key) {
                // A non-numeric incumbent plus numeric incoming raises
                // `TypeError` in Python; unreachable from conforming
                // providers (only numerics ever merge in), so the key
                // keeps its value.
                Some(Value::Number(have)) => have,
                Some(Value::Bool(have)) => i64::from(have).into(),
                Some(kept) => {
                    self.details.insert(key.clone(), kept);
                    continue;
                }
                None => 0.into(),
            };
            self.details.insert(
                key.clone(),
                add_json_numbers(&have, &incoming).unwrap_or(Value::Null),
            );
        }
    }

    /// Derived total (`input_tokens + output_tokens`).
    pub fn total_tokens(&self) -> UsageNumber {
        self.input_tokens.add(self.output_tokens)
    }

    /// The `dataclasses.asdict` shape in field order, for
    /// [`pidash_jobs::dispatch_execute::RawUsage`] `fields`.
    pub fn as_report(&self) -> Map<String, Value> {
        let mut report = Map::with_capacity(10);
        report.insert("input_tokens".to_string(), self.input_tokens.to_json());
        report.insert(
            "cache_write_tokens".to_string(),
            self.cache_write_tokens.to_json(),
        );
        report.insert(
            "cache_read_tokens".to_string(),
            self.cache_read_tokens.to_json(),
        );
        report.insert("output_tokens".to_string(), self.output_tokens.to_json());
        report.insert(
            "input_audio_tokens".to_string(),
            self.input_audio_tokens.to_json(),
        );
        report.insert(
            "cache_audio_read_tokens".to_string(),
            self.cache_audio_read_tokens.to_json(),
        );
        report.insert(
            "output_audio_tokens".to_string(),
            self.output_audio_tokens.to_json(),
        );
        report.insert("details".to_string(), Value::Object(self.details.clone()));
        report.insert("requests".to_string(), Value::Number(self.requests.into()));
        report.insert(
            "tool_calls".to_string(),
            Value::Number(self.tool_calls.into()),
        );
        report
    }
}

/// Add two JSON numbers the way the `details` merge does: int+int stays
/// int (saturating), anything float-involved becomes a float. `None`
/// when the float sum is non-finite (unreachable from provider data).
fn add_json_numbers(left: &serde_json::Number, right: &serde_json::Number) -> Option<Value> {
    match (left.as_i64(), right.as_i64()) {
        (Some(left), Some(right)) => Some(Value::Number(left.saturating_add(right).into())),
        _ => {
            let sum = left.as_f64()? + right.as_f64()?;
            serde_json::Number::from_f64(sum).map(Value::Number)
        }
    }
}

/// A breached usage limit with its exact `UsageLimitExceeded` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitBreach(pub String);

/// `check_before_request`: requests at-or-over the cap, input/total
/// strictly over (no output check before the request).
pub fn check_before_request(limits: &UsageLimits, usage: &RunUsage) -> Result<(), LimitBreach> {
    if let Some(cap) = limits.request_limit {
        if usage.requests >= cap {
            return Err(LimitBreach(format!(
                "The next request would exceed the request_limit of {cap}"
            )));
        }
    }
    if let Some(cap) = limits.input_tokens_limit {
        if usage.input_tokens.as_i64() > cap {
            return Err(LimitBreach(format!(
                "The next request would exceed the input_tokens_limit of {cap} (input_tokens={})",
                usage.input_tokens.as_i64()
            )));
        }
    }
    if let Some(cap) = limits.total_tokens_limit {
        if usage.total_tokens().as_i64() > cap {
            return Err(LimitBreach(format!(
                "The next request would exceed the total_tokens_limit of {cap} (total_tokens={})",
                usage.total_tokens().as_i64()
            )));
        }
    }
    Ok(())
}

/// `check_tokens` after a response: input/output/total strictly over.
pub fn check_tokens(limits: &UsageLimits, usage: &RunUsage) -> Result<(), LimitBreach> {
    if let Some(cap) = limits.input_tokens_limit {
        if usage.input_tokens.as_i64() > cap {
            return Err(LimitBreach(format!(
                "Exceeded the input_tokens_limit of {cap} (input_tokens={})",
                usage.input_tokens.as_i64()
            )));
        }
    }
    if let Some(cap) = limits.output_tokens_limit {
        if usage.output_tokens.as_i64() > cap {
            return Err(LimitBreach(format!(
                "Exceeded the output_tokens_limit of {cap} (output_tokens={})",
                usage.output_tokens.as_i64()
            )));
        }
    }
    if let Some(cap) = limits.total_tokens_limit {
        if usage.total_tokens().as_i64() > cap {
            return Err(LimitBreach(format!(
                "Exceeded the total_tokens_limit of {cap} (total_tokens={})",
                usage.total_tokens().as_i64()
            )));
        }
    }
    Ok(())
}

/// `check_before_tool_call`: the projected call count strictly over.
pub fn check_before_tool_call(
    limits: &UsageLimits,
    projected_tool_calls: i64,
) -> Result<(), LimitBreach> {
    if let Some(cap) = limits.tool_calls_limit {
        if projected_tool_calls > cap {
            return Err(LimitBreach(format!(
                "The next tool call(s) would exceed the tool_calls_limit of {cap} (tool_calls={projected_tool_calls})."
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// genai-prices 0.1.9 `_reported_usage_keys`: mapping destinations
/// outside this set are skipped (never observed in the golden tables —
/// asserted at load so a future table cannot silently change meaning).
const REPORTED_USAGE_KEYS: [&str; 58] = [
    "audio_seconds",
    "cache_audio_read_tokens",
    "cache_audio_write_1h_tokens",
    "cache_audio_write_5m_tokens",
    "cache_audio_write_tokens",
    "cache_image_read_tokens",
    "cache_image_write_1h_tokens",
    "cache_image_write_5m_tokens",
    "cache_image_write_tokens",
    "cache_read_tokens",
    "cache_text_read_tokens",
    "cache_text_write_1h_tokens",
    "cache_text_write_5m_tokens",
    "cache_text_write_tokens",
    "cache_video_read_tokens",
    "cache_video_write_1h_tokens",
    "cache_video_write_5m_tokens",
    "cache_video_write_tokens",
    "cache_write_1h_tokens",
    "cache_write_5m_tokens",
    "cache_write_tokens",
    "code_executions",
    "input_annotated_document_pages",
    "input_audio_seconds",
    "input_audio_tokens",
    "input_audio_tool_tokens",
    "input_characters",
    "input_document_pages",
    "input_image_tokens",
    "input_image_tool_tokens",
    "input_pixels",
    "input_text_messages",
    "input_text_tokens",
    "input_text_tool_tokens",
    "input_tokens",
    "input_tool_tokens",
    "input_video_tokens",
    "input_video_tool_tokens",
    "output_audio_citation_tokens",
    "output_audio_reasoning_tokens",
    "output_audio_seconds",
    "output_audio_tokens",
    "output_citation_tokens",
    "output_image_citation_tokens",
    "output_image_reasoning_tokens",
    "output_image_tokens",
    "output_reasoning_tokens",
    "output_text_citation_tokens",
    "output_text_reasoning_tokens",
    "output_text_tokens",
    "output_tokens",
    "output_video_citation_tokens",
    "output_video_reasoning_tokens",
    "output_video_tokens",
    "rerank_searches",
    "social_searches",
    "storage_searches",
    "web_searches",
];

/// Provider-table load failure (golden drift or a bad pattern).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTableError(pub String);

impl std::fmt::Display for ProviderTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "usage provider table invalid: {}", self.0)
    }
}

impl std::error::Error for ProviderTableError {}

/// One extractor mapping: a path from the usage root to a destination.
#[derive(Debug, Clone)]
struct Mapping {
    path: Vec<PathSeg>,
    dest: String,
    required: bool,
}

/// A path segment: an object key or an array-item match.
#[derive(Debug, Clone)]
enum PathSeg {
    Key(String),
    ArrayMatch { field: String, equals: String },
}

/// One snapshot provider: URL pattern plus per-flavor mappings.
#[derive(Debug, Clone)]
struct ProviderEntry {
    id: String,
    pattern: regex::Regex,
    chat: Option<Vec<Mapping>>,
    default: Option<Vec<Mapping>>,
}

/// The compiled usage-extractor table (golden `usage_providers`).
#[derive(Debug, Clone)]
pub struct ProviderTable {
    entries: Vec<ProviderEntry>,
}

impl ProviderTable {
    /// Compile the golden provider table (patterns in snapshot order).
    pub fn load(golden: &Golden) -> Result<Self, ProviderTableError> {
        let providers = golden
            .document()
            .get("usage_providers")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderTableError("missing `usage_providers`".to_string()))?;
        let mut entries = Vec::with_capacity(providers.len());
        for provider in providers {
            let id = provider
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderTableError("provider without `id`".to_string()))?;
            let pattern_text = provider
                .get("api_pattern")
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderTableError(format!("provider `{id}` without pattern")))?;
            // `re.match` anchors at the start only; the wrapper keeps
            // top-level alternations inside the anchor.
            let pattern = regex::Regex::new(&format!("^(?:{pattern_text})"))
                .map_err(|err| ProviderTableError(format!("provider `{id}` pattern: {err}")))?;
            entries.push(ProviderEntry {
                id: id.to_string(),
                pattern,
                chat: mappings(provider, "chat")?,
                default: mappings(provider, "default")?,
            });
        }
        Ok(ProviderTable { entries })
    }

    /// Extract one OpenAI-lane (`chat`) response usage.
    ///
    /// `usage` is the raw `usage` object (or `None` for a missing/null
    /// usage, which yields zeros); `model` is recorded for parity but
    /// plays no part in extraction (genai-prices never matches on it).
    pub fn extract_openai(
        &self,
        provider_url: &str,
        model: &str,
        usage: Option<&Map<String, Value>>,
    ) -> RequestUsage {
        let _ = model;
        let Some(usage) = usage else {
            return RequestUsage::zeros();
        };
        let details = openai_details(usage);
        for (provider_id, url) in [
            (None, Some(provider_url)),
            (Some("openai"), None),
            (Some("openai"), None),
        ] {
            if let Some(found) = self.extract_link(provider_id, url, "chat", usage) {
                let mut usage = found;
                usage.details = details.clone();
                return usage;
            }
        }
        let mut zeros = RequestUsage::zeros();
        zeros.details = details;
        zeros
    }

    /// Extract one Anthropic-lane (`default`) response usage.
    ///
    /// Details are the four flat usage keys (`_extract_usage_details`;
    /// compaction iterations only exist on streamed deltas, which this
    /// non-streaming runtime never sees).
    pub fn extract_anthropic(
        &self,
        provider_url: &str,
        usage: &Map<String, Value>,
    ) -> RequestUsage {
        let mut details = Map::new();
        for key in [
            "input_tokens",
            "output_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ] {
            if let Some(Value::Number(number)) = usage.get(key) {
                details.insert(key.to_string(), Value::Number(number.clone()));
            }
        }
        for (provider_id, url) in [
            (None, Some(provider_url)),
            (Some("anthropic"), None),
            (Some("anthropic"), None),
        ] {
            if let Some(found) = self.extract_link(provider_id, url, "default", usage) {
                let mut usage = found;
                usage.details = details.clone();
                return usage;
            }
        }
        let mut zeros = RequestUsage::zeros();
        zeros.details = details;
        zeros
    }

    /// One `find_provider` + `extract_usage` chain link: `None` on any
    /// failure (unknown provider, missing flavor, unresolvable path,
    /// invalid value, unknown destination — the `except Exception`
    /// analog).
    fn extract_link(
        &self,
        provider_id: Option<&str>,
        provider_url: Option<&str>,
        flavor: &str,
        usage: &Map<String, Value>,
    ) -> Option<RequestUsage> {
        let entry = match (provider_id, provider_url) {
            (Some(id), _) => self.entries.iter().find(|entry| entry.id == id)?,
            (None, Some(url)) => self
                .entries
                .iter()
                .find(|entry| entry.pattern.is_match(url))?,
            (None, None) => return None,
        };
        let mappings = match flavor {
            "chat" => entry.chat.as_ref()?,
            _ => entry.default.as_ref()?,
        };
        let mut values: std::collections::BTreeMap<String, UsageNumber> =
            std::collections::BTreeMap::new();
        let mut supported = 0;
        let mut values_set = false;
        for mapping in mappings {
            if !REPORTED_USAGE_KEYS.contains(&mapping.dest.as_str()) {
                continue;
            }
            supported += 1;
            let Some(value) = resolve_path(&mapping.path, usage) else {
                if mapping.required {
                    return None;
                }
                continue;
            };
            let number = validated_number(&value)?;
            match values.remove(&mapping.dest) {
                Some(have) => {
                    values.insert(mapping.dest.clone(), have.add(number));
                }
                None => {
                    values.insert(mapping.dest.clone(), number);
                }
            }
            values_set = true;
        }
        if supported > 0 && !values_set {
            return None;
        }
        let mut usage = RequestUsage::zeros();
        for (dest, number) in values {
            match dest.as_str() {
                "input_tokens" => usage.input_tokens = number,
                "cache_write_tokens" => usage.cache_write_tokens = number,
                "cache_read_tokens" => usage.cache_read_tokens = number,
                "output_tokens" => usage.output_tokens = number,
                "input_audio_tokens" => usage.input_audio_tokens = number,
                "cache_audio_read_tokens" => usage.cache_audio_read_tokens = number,
                "output_audio_tokens" => usage.output_audio_tokens = number,
                // Any other reported destination (e.g.
                // `output_reasoning_tokens`) fails the whole link — the
                // `RequestUsage(**kwargs)` `TypeError` analog.
                _ => return None,
            }
        }
        Some(usage)
    }
}

fn mappings(provider: &Value, flavor: &str) -> Result<Option<Vec<Mapping>>, ProviderTableError> {
    let Some(list) = provider.get(flavor) else {
        return Ok(None);
    };
    if list.is_null() {
        return Ok(None);
    }
    let list = list
        .as_array()
        .ok_or_else(|| ProviderTableError(format!("`{flavor}` is not an array")))?;
    let mut mappings = Vec::with_capacity(list.len());
    for mapping in list {
        let path = mapping
            .get("path")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderTableError("mapping without `path`".to_string()))?;
        let mut segments = Vec::with_capacity(path.len());
        for segment in path {
            if let Some(key) = segment.as_str() {
                segments.push(PathSeg::Key(key.to_string()));
                continue;
            }
            let spec = segment
                .get("array_match")
                .ok_or_else(|| ProviderTableError("bad path segment".to_string()))?;
            let field = spec
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| ProviderTableError("array_match without `field`".to_string()))?;
            let equals = spec
                .get("match")
                .and_then(|rule| rule.get("equals"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderTableError("only `equals` array matches are supported".to_string())
                })?;
            segments.push(PathSeg::ArrayMatch {
                field: field.to_string(),
                equals: equals.to_string(),
            });
        }
        let dest = mapping
            .get("dest")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderTableError("mapping without `dest`".to_string()))?;
        if !REPORTED_USAGE_KEYS.contains(&dest) {
            return Err(ProviderTableError(format!(
                "destination `{dest}` outside the reported-usage keys"
            )));
        }
        mappings.push(Mapping {
            path: segments,
            dest: dest.to_string(),
            required: mapping
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok(Some(mappings))
}

/// Resolve an extractor path from the usage root (`_extract_path`):
/// `None` when the value is absent or unusable (the caller fails the
/// link when the mapping is `required`, skips it otherwise).
fn resolve_path(path: &[PathSeg], usage: &Map<String, Value>) -> Option<Value> {
    let (last, steps) = path.split_last()?;
    let mut data = Value::Object(usage.clone());
    for step in steps {
        match step {
            PathSeg::ArrayMatch { field, equals } => {
                let items = data.as_array()?;
                let found = items.iter().find(|item| {
                    let Some(object) = item.as_object() else {
                        return false;
                    };
                    let Some(candidate) = object.get(field) else {
                        return false;
                    };
                    if !is_truthy(candidate) {
                        return false;
                    }
                    candidate.as_str().is_some_and(|text| text == equals)
                })?;
                data = found.clone();
            }
            PathSeg::Key(key) => {
                data = data.as_object()?.get(key)?.clone();
            }
        }
    }
    match last {
        PathSeg::Key(key) => {
            let value = data.as_object()?.get(key)?.clone();
            // `_extract_path` accepts numbers and bools (`bool` is an
            // `Integral`; `validated_number` rejects it later, failing
            // the link); anything else counts as absent.
            if value.is_number() || value.is_boolean() {
                Some(value)
            } else {
                None
            }
        }
        PathSeg::ArrayMatch { .. } => None,
    }
}

/// Python truthiness for JSON values (the `ArrayMatch.extract` guard).
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            number.as_i64().is_some_and(|int| int != 0)
                || number.as_u64().is_some_and(|uint| uint != 0)
                || number.as_f64().is_some_and(|float| float != 0.0)
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Validate one extracted value (`validate_usage_value`): JSON integers
/// (non-negative, saturating to `i64`), finite non-negative floats;
/// bools, strings, nulls, containers, negatives, and non-finite floats
/// fail the whole link.
fn validated_number(value: &Value) -> Option<UsageNumber> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return None;
                }
                Some(UsageNumber::Int(int))
            } else if let Some(uint) = number.as_u64() {
                Some(UsageNumber::Int(i64::try_from(uint).unwrap_or(i64::MAX)))
            } else {
                let float = number.as_f64()?;
                if !float.is_finite() || float < 0.0 {
                    return None;
                }
                Some(UsageNumber::Float(float))
            }
        }
        _ => None,
    }
}

/// The OpenAI-lane `details` pre-pass: non-standard integer usage keys
/// (bools count — `isinstance(True, int)`) plus the
/// `completion_tokens_details` dump.
fn openai_details(usage: &Map<String, Value>) -> Map<String, Value> {
    let mut details = Map::new();
    for (key, value) in usage {
        if matches!(
            key.as_str(),
            "prompt_tokens"
                | "completion_tokens"
                | "input_tokens"
                | "output_tokens"
                | "total_tokens"
        ) {
            continue;
        }
        if value.is_number() || value.is_boolean() {
            details.insert(key.clone(), value.clone());
        }
    }
    if let Some(Value::Object(nested)) = usage.get("completion_tokens_details") {
        for (key, value) in nested {
            if !value.is_null() {
                details.insert(key.clone(), value.clone());
            }
        }
    }
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ProviderTable {
        let golden = Golden::load().expect("golden parses");
        ProviderTable::load(&golden).expect("table compiles")
    }

    /// Replay every golden usage vector through the extractor.
    #[test]
    fn usage_vectors_replay_matches_golden() {
        let golden = Golden::load().expect("golden parses");
        let vectors = golden
            .document()
            .get("usage_vectors")
            .and_then(Value::as_array)
            .expect("usage vectors");
        assert_eq!(vectors.len(), 66, "vector count");
        let table = table();
        for vector in vectors {
            let lane = vector["lane"].as_str().unwrap_or("?");
            let url = vector["provider_url"].as_str().unwrap_or("?");
            let shape = vector["shape"].as_str().unwrap_or("?");
            let usage = vector["usage"].as_object().expect("usage object");
            let got = match lane {
                "openai" => table.extract_openai(url, "gpt-4o", Some(usage)),
                _ => table.extract_anthropic(url, usage),
            };
            let expected = vector["extracted"].as_object().expect("extracted");
            for key in [
                "input_tokens",
                "cache_write_tokens",
                "cache_read_tokens",
                "output_tokens",
                "input_audio_tokens",
                "cache_audio_read_tokens",
                "output_audio_tokens",
            ] {
                let want = expected.get(key).expect("expected counter");
                let have = match key {
                    "input_tokens" => got.input_tokens,
                    "cache_write_tokens" => got.cache_write_tokens,
                    "cache_read_tokens" => got.cache_read_tokens,
                    "output_tokens" => got.output_tokens,
                    "input_audio_tokens" => got.input_audio_tokens,
                    "cache_audio_read_tokens" => got.cache_audio_read_tokens,
                    _ => got.output_audio_tokens,
                }
                .to_json();
                assert_eq!(&have, want, "{lane} {url} {shape}: counter `{key}`");
            }
            assert_eq!(
                &Value::Object(got.details.clone()),
                expected.get("details").expect("expected details"),
                "{lane} {url} {shape}: details"
            );
        }
    }

    #[test]
    fn accumulation_drops_output_audio() {
        let mut run = RunUsage::default();
        run.incr(&RequestUsage {
            input_tokens: UsageNumber::Int(100),
            output_tokens: UsageNumber::Int(20),
            output_audio_tokens: UsageNumber::Int(2),
            details: Map::from_iter([("audio_tokens".to_string(), Value::from(2))]),
            ..RequestUsage::zeros()
        });
        assert_eq!(run.output_audio_tokens, UsageNumber::Int(0));
        assert_eq!(run.input_tokens, UsageNumber::Int(100));
        assert_eq!(
            run.details.get("audio_tokens"),
            Some(&Value::Number(2.into()))
        );
        assert_eq!(run.total_tokens(), UsageNumber::Int(120));
    }

    #[test]
    fn details_merge_skips_non_numeric() {
        // `if isinstance(value, (int, float))` (`_incr_usage_tokens`):
        // a string-valued detail never lands (first-seen) nor
        // clobbers (later-seen); a first-seen bool coerces to int.
        let mut run = RunUsage::default();
        run.incr(&RequestUsage {
            details: Map::from_iter([
                ("note".to_string(), Value::String("x".to_owned())),
                ("flag".to_string(), Value::Bool(true)),
                ("count".to_string(), Value::from(2)),
            ]),
            ..RequestUsage::zeros()
        });
        assert!(!run.details.contains_key("note"));
        assert_eq!(run.details.get("flag"), Some(&Value::Number(1.into())));
        assert_eq!(run.details.get("count"), Some(&Value::Number(2.into())));
        run.incr(&RequestUsage {
            details: Map::from_iter([
                ("note".to_string(), Value::String("y".to_owned())),
                ("flag".to_string(), Value::from(4)),
                ("count".to_string(), Value::Null),
            ]),
            ..RequestUsage::zeros()
        });
        assert!(!run.details.contains_key("note"));
        assert_eq!(run.details.get("flag"), Some(&Value::Number(5.into())));
        assert_eq!(run.details.get("count"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn limit_messages_match_python() {
        let limits = UsageLimits {
            request_limit: Some(1),
            tool_calls_limit: Some(0),
            input_tokens_limit: Some(100),
            output_tokens_limit: Some(50),
            total_tokens_limit: Some(120),
        };
        let usage = RunUsage {
            requests: 1,
            ..RunUsage::default()
        };
        assert_eq!(
            check_before_request(&limits, &usage).unwrap_err(),
            LimitBreach("The next request would exceed the request_limit of 1".to_string())
        );
        let usage = RunUsage {
            input_tokens: UsageNumber::Int(101),
            ..RunUsage::default()
        };
        assert_eq!(
            check_before_request(&limits, &usage).unwrap_err(),
            LimitBreach(
                "The next request would exceed the input_tokens_limit of 100 (input_tokens=101)"
                    .to_string()
            )
        );
        let usage = RunUsage {
            input_tokens: UsageNumber::Int(100),
            output_tokens: UsageNumber::Int(50),
            ..RunUsage::default()
        };
        assert_eq!(
            check_tokens(&limits, &usage),
            Err(LimitBreach(
                "Exceeded the total_tokens_limit of 120 (total_tokens=150)".to_string()
            ))
        );
        assert_eq!(
            check_before_tool_call(&limits, 1).unwrap_err(),
            LimitBreach(
                "The next tool call(s) would exceed the tool_calls_limit of 0 (tool_calls=1)."
                    .to_string()
            )
        );
    }

    #[test]
    fn anthropic_input_sums_cache_tokens() {
        let table = table();
        let usage = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 20,
            "cache_creation_input_tokens": 50,
            "cache_read_input_tokens": 30,
        });
        let got = table.extract_anthropic(
            "https://api.anthropic.com",
            usage.as_object().expect("object"),
        );
        assert_eq!(got.input_tokens, UsageNumber::Int(180));
        assert_eq!(got.cache_write_tokens, UsageNumber::Int(50));
        assert_eq!(got.cache_read_tokens, UsageNumber::Int(30));
    }
}
