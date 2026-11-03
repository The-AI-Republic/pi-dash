#![forbid(unsafe_code)]

//! Loop payload shapes: user-facing whitelist, admin full shape, cadence label.
//!
//! Ports (all under `apps/api/pi_dash/`):
//!
//! * `loop/serializers.py:16-23` (`_FREQ_LABELS`) and `:26-32`
//!   ([`interval_label`]). The `FREQ=` prefix match is case-sensitive
//!   (`freq=daily` falls through to `"periodically"`, fixture-noted); the
//!   FREQ value itself is uppercased before lookup. The first `FREQ=`
//!   part wins — Python `return`s inside the loop.
//! * `loop/serializers.py:35-43` ([`public_job_payload`]). Deliberate
//!   whitelist: `prompt`, `min_role`, the admin `name`, and anything saying
//!   "loop" never reach a normal user (design §9.1). `name` is `public_name`.
//! * `loop/admin_views.py:43-59` ([`job_payload`]). Full 14-key job shape;
//!   `dtstart`/`created_at`/`updated_at` render `isoformat()` or `null`.
//!   The `stats` rollup on detail GET (`:135-149`) belongs to the handlers
//!   layer, not this shape.
//!
//! Key order notes for the future handlers layer (Python dict insertion
//! order). Sibling crates (`api`, `jobs`) enable `serde_json/preserve_order`,
//! so under `cargo test --workspace` feature unification turns it on for
//! these tests too; the tests below therefore establish byte-identity in a
//! sorted-keys canonical form (both sides canonicalized through the same
//! recursive sorter), which is also the form the goldens are stored in:
//!
//! * public: `slug, name, description, interval_label, enabled`
//! * admin: `id, slug, name, public_name, public_description, prompt,
//!   min_role, enabled, is_builtin, dtstart, rrule, tzid, created_at,
//!   updated_at`

use serde_json::{json, Value};

/// `loop/serializers.py:26-32`: plain-language cadence from an RRULE FREQ.
/// Falls back to `"periodically"` for anything unrecognized, for a missing
/// `FREQ=` part, and for an empty input (`(rrule or "").split(";")`).
pub fn interval_label(rrule: &str) -> &'static str {
    for part in rrule.split(';') {
        if let Some(freq) = part.strip_prefix("FREQ=") {
            return match freq.to_uppercase().as_str() {
                "MINUTELY" => "every few minutes",
                "HOURLY" => "hourly",
                "DAILY" => "daily",
                "WEEKLY" => "weekly",
                "MONTHLY" => "monthly",
                "YEARLY" => "yearly",
                _ => "periodically",
            };
        }
    }
    "periodically"
}

/// `public_job_payload` output keys, Python dict order
/// (`loop/serializers.py:35-43`).
pub const PUBLIC_JOB_KEYS: &[&str] = &["slug", "name", "description", "interval_label", "enabled"];

/// `_job_payload` output keys, Python dict order
/// (`loop/admin_views.py:43-59`).
pub const ADMIN_JOB_KEYS: &[&str] = &[
    "id",
    "slug",
    "name",
    "public_name",
    "public_description",
    "prompt",
    "min_role",
    "enabled",
    "is_builtin",
    "dtstart",
    "rrule",
    "tzid",
    "created_at",
    "updated_at",
];

/// A `LoopJob` row borrowed for the user-facing card. Only whitelisted
/// columns are present — there is no `prompt`, `min_role`, or admin `name`
/// field to leak by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicJobRow<'a> {
    pub slug: &'a str,
    pub public_name: &'a str,
    pub public_description: &'a str,
    pub rrule: &'a str,
}

/// `loop/serializers.py:35-43`: user-facing job card, whitelisted keys only.
pub fn public_job_payload(job: &PublicJobRow<'_>, enabled: bool) -> Value {
    json!({
        "slug": job.slug,
        "name": job.public_name,
        "description": job.public_description,
        "interval_label": interval_label(job.rrule),
        "enabled": enabled,
    })
}

/// A `LoopJob` row borrowed for the admin shape. Datetimes are pre-rendered
/// DRF `isoformat` strings; nullable columns are `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminJobRow<'a> {
    pub id: &'a str,
    pub slug: &'a str,
    pub name: &'a str,
    pub public_name: &'a str,
    pub public_description: &'a str,
    pub prompt: &'a str,
    pub min_role: i32,
    pub enabled: bool,
    pub is_builtin: bool,
    pub dtstart: Option<&'a str>,
    pub rrule: &'a str,
    pub tzid: &'a str,
    pub created_at: Option<&'a str>,
    pub updated_at: Option<&'a str>,
}

fn opt_str(value: Option<&str>) -> Value {
    match value {
        Some(s) => Value::String(s.to_owned()),
        None => Value::Null,
    }
}

/// `loop/admin_views.py:43-59`: full 14-key admin job shape.
pub fn job_payload(job: &AdminJobRow<'_>) -> Value {
    json!({
        "id": job.id,
        "slug": job.slug,
        "name": job.name,
        "public_name": job.public_name,
        "public_description": job.public_description,
        "prompt": job.prompt,
        "min_role": job.min_role,
        "enabled": job.enabled,
        "is_builtin": job.is_builtin,
        "dtstart": opt_str(job.dtstart),
        "rrule": job.rrule,
        "tzid": job.tzid,
        "created_at": opt_str(job.created_at),
        "updated_at": opt_str(job.updated_at),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/../../fixtures/loop/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Field-for-field equality plus byte-identical replay in canonical
    /// (sorted-keys) form. Both sides are canonicalized through the same
    /// recursive sorter before the string comparison, so the check is
    /// hermetic: it passes whether or not `serde_json/preserve_order` is
    /// enabled by workspace feature unification (`api` and `jobs` enable
    /// it, so `cargo test --workspace` turns it on for these tests too).
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> =
                    map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(entries.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden output"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch (canonical sorted-keys form)"
        );
    }

    #[test]
    fn interval_label_replays_golden() {
        // Fixture serializers/interval_label.golden.json:
        // `loop/serializers.py:16-32`, 13 vectors.
        let golden = fixture("serializers/interval_label.golden.json");
        let vectors = golden
            .get("vectors")
            .and_then(Value::as_array)
            .expect("vectors array");
        assert_eq!(vectors.len(), 13, "fixture vector count");
        for vector in vectors {
            let rrule = vector
                .get("rrule")
                .and_then(Value::as_str)
                .expect("vector rrule");
            let label = vector
                .get("label")
                .and_then(Value::as_str)
                .expect("vector label");
            assert_eq!(interval_label(rrule), label, "rrule {rrule:?}");
        }
    }

    #[test]
    fn interval_label_first_freq_wins() {
        // Python returns inside the loop: the first `FREQ=` part decides,
        // even when a later part names a known frequency.
        assert_eq!(
            interval_label("FREQ=FORTNIGHTLY;FREQ=DAILY"),
            "periodically"
        );
        assert_eq!(interval_label("FREQ=HOURLY;FREQ=DAILY"), "hourly");
    }

    #[test]
    fn public_job_payload_replays_golden() {
        // Fixture serializers/public_job_payload.golden.json:
        // `loop/serializers.py:35-43`.
        let golden = fixture("serializers/public_job_payload.golden.json");
        let input = golden.get("input").expect("input object");
        let req = |key: &str| {
            input
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("golden input lacks string key {key}"))
        };
        let row = PublicJobRow {
            slug: req("slug"),
            public_name: req("public_name"),
            public_description: req("public_description"),
            rrule: req("rrule"),
        };
        assert_replay(
            &public_job_payload(&row, true),
            golden.get("enabled_true").expect("enabled_true"),
        );
        assert_replay(
            &public_job_payload(&row, false),
            golden.get("enabled_false").expect("enabled_false"),
        );
        // Whitelist proof: the secret-bearing input keys never appear.
        let produced = public_job_payload(&row, true);
        for leaked in [
            "prompt",
            "min_role",
            "public_name",
            "public_description",
            "rrule",
        ] {
            assert!(
                produced.get(leaked).is_none(),
                "whitelist leak: {leaked} present"
            );
        }
        // Set-equality over keys: sorted before comparing so the check
        // holds whether or not `preserve_order` is unified on.
        let mut keys: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected_keys: Vec<&str> = PUBLIC_JOB_KEYS.to_vec();
        expected_keys.sort_unstable();
        assert_eq!(keys, expected_keys, "exactly the 5 whitelisted keys");
        assert_eq!(keys.len(), PUBLIC_JOB_KEYS.len(), "exactly 5 keys");
    }

    #[test]
    fn admin_job_payload_replays_golden() {
        // Full 14-key shape replayed against the live-recorded row in
        // handlers/admin_crud.golden.json `admin_list.body[0]`
        // (`loop/admin_views.py:43-59` via the real list GET).
        let crud = fixture("handlers/admin_crud.golden.json");
        let row_body = crud
            .get("admin_list")
            .and_then(|v| v.get("body"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .expect("admin_list body row");
        let req = |key: &str| {
            row_body
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("golden row lacks string key {key}"))
        };
        let opt = |key: &str| match row_body.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.as_str()),
            Some(other) => panic!("golden key {key} is not a string/null: {other}"),
        };
        let row = AdminJobRow {
            id: req("id"),
            slug: req("slug"),
            name: req("name"),
            public_name: req("public_name"),
            public_description: req("public_description"),
            prompt: req("prompt"),
            min_role: row_body
                .get("min_role")
                .and_then(Value::as_i64)
                .expect("min_role") as i32,
            enabled: row_body
                .get("enabled")
                .and_then(Value::as_bool)
                .expect("enabled"),
            is_builtin: row_body
                .get("is_builtin")
                .and_then(Value::as_bool)
                .expect("is_builtin"),
            dtstart: opt("dtstart"),
            rrule: req("rrule"),
            tzid: req("tzid"),
            created_at: opt("created_at"),
            updated_at: opt("updated_at"),
        };
        assert_replay(&job_payload(&row), row_body);

        // Key contract against handlers/admin_job_payload.golden.json
        // (`loop/admin_views.py:43-59; stats rollup 135-149`): exactly the
        // 14 shape keys, no `stats` (detail-GET appendage, handlers layer).
        let shape = fixture("handlers/admin_job_payload.golden.json");
        let mut expected: Vec<&str> = shape
            .get("keys")
            .and_then(Value::as_array)
            .expect("keys array")
            .iter()
            .filter_map(Value::as_str)
            .filter(|k| !k.starts_with('+'))
            .collect();
        expected.sort_unstable();
        let produced = job_payload(&row);
        let mut actual: Vec<&str> = produced
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "14-key admin shape");
        assert_eq!(ADMIN_JOB_KEYS.len(), 14, "ADMIN_JOB_KEYS count");
    }

    #[test]
    fn admin_job_payload_null_datetimes() {
        // `job.dtstart.isoformat() if job.dtstart else None`
        // (`loop/admin_views.py:54,57-58`): absent datetimes render null,
        // keys still present.
        let row = AdminJobRow {
            id: "c9645dfc-7bc8-4c6a-bca8-31b6515e0c99",
            slug: "no-dates",
            name: "Admin name",
            public_name: "Pub name",
            public_description: "desc",
            prompt: "do the thing",
            min_role: 15,
            enabled: true,
            is_builtin: false,
            dtstart: None,
            rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
            tzid: "UTC",
            created_at: None,
            updated_at: None,
        };
        let produced = job_payload(&row);
        for key in ["dtstart", "created_at", "updated_at"] {
            assert_eq!(produced.get(key), Some(&Value::Null), "{key} is null");
        }
        assert_eq!(
            produced.as_object().expect("object").len(),
            14,
            "keys present even when null"
        );
    }
}
