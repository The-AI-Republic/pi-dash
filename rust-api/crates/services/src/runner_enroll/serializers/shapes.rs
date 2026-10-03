//! Pod / runner / dev-machine serializer shapes (D-13).
//!
//! Port of `apps/api/pi_dash/runner/serializers.py`:
//!
//! * `PodSerializer` (`:36-72`, incl `get_runner_count` `:71-72`)
//! * `RunnerLiveStateSerializer` (`:75-101`, nested under
//!   `RunnerSerializer.live_state`)
//! * `DevMachineSerializer` (`:104-130`, incl the annotated fields)
//! * `PodMiniSerializer` (`:133-142`)
//! * `DevMachineMiniSerializer` (`:145-153`)
//! * `RunnerSerializer` (`:156-220`, incl `live_state` null)
//! * `RunnerEnrollRequestSerializer` (`:223-238`)
//! * `RUNNER_NAME_CHARSET` (`:27-33`) shared with the view regex
//!   `_RUNNER_NAME_RE` (`runner/views/enrollment.py:541`)
//!
//! These are pure kernels in the `space::serializers::lite` style: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! output order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`.
//! Datetimes cross this boundary already rendered as DRF `iso-8601` strings
//! (formatting owns to the DB edge), so rendering here is a byte-exact
//! passthrough. JSON blobs (`capabilities`, `dev_metadata`, `usage`) pass
//! through by reference.
//!
//! `input_tokens` / `output_tokens` / `total_tokens` are model `@property`
//! projections over the `usage` blob, computed at render time exactly as
//! DRF renders the properties via `ReadOnlyField`, through the canonical
//! D-15 usage kernel (`pidash_types::runner_runs::flat_token_fields`,
//! `runner/services/usage.py:165-173` — never forked here). Inherited
//! limit: usage-counter strings parse ASCII-only (no provider emits `Nd`).
//!
//! Request validation ([`validate_enroll_request`]) ports DRF 3.15.2 field
//! mechanics (`CharField.run_validation` / `to_internal_value`,
//! `UUIDField.to_internal_value`, `Serializer.to_internal_value`) byte for
//! byte, verified against the repo-pinned Django 4.2.30 / DRF 3.15.2.
//!
//! Ported bugs / quirks (translate, don't redesign):
//!
//! * QUIRK-runner-count (`serializers.py:71-72` vs
//!   `runner/views/pods.py:226-228`): `get_runner_count` is an unfiltered
//!   `pod.runners.count()` — it INCLUDES revoked runners — while the pod
//!   delete guard excludes them and its comment claims the two match. The
//!   comment is wrong; the shape takes the count as given and the query
//!   layer must count unfiltered (D13-F2 `runner_count_mismatch`).
//! * QUIRK-name-dollar: both name patterns anchor with `$`, which also
//!   matches just before one trailing `\n` (`'a\n'` is valid, `'a\n\n'`
//!   is not — probe-verified). Every call site strips first
//!   (`enrollment.py:580`, `machine_commands.py:111`, DRF
//!   `trim_whitespace`), so the arm is unreachable live; the kernel keeps
//!   it for exact equivalence.
//! * QUIRK-surrogate: DRF's `ProhibitSurrogateCharactersValidator` reports
//!   lone surrogates per field, but lone surrogates never survive JSON
//!   parsing into this layer (`serde_json` rejects them at the envelope,
//!   like the D-20 `json_cpython` precedent in the api crate). Envelope
//!   acceptance is the handler layer's concern, not this kernel's.
//! * QUIRK-coerce-inf: `coerce_token(float('inf'))` raises an uncaught
//!   `OverflowError` in Python (only `TypeError`/`ValueError` are caught).
//!   `jsonb` cannot store non-finite numbers, so the input is unreachable
//!   from the database; the canonical kernel maps it to `None` like `NaN`.
//!
//! `read_only_fields` (`serializers.py:57-68, 129, 199-220`) constrain
//! writes, of which this port has none; the enroll request serializer is
//! the only write shape here.

use pidash_types::runner_runs::flat_token_fields;
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Wire field order (Meta.fields order == JSON key order, D13-F2)
// ---------------------------------------------------------------------------

/// `PodSerializer.Meta.fields` (`serializers.py:46-58`), wire order.
pub const POD_WIRE_FIELDS: [&str; 11] = [
    "id",
    "name",
    "description",
    "is_default",
    "workspace",
    "project",
    "project_identifier",
    "created_by",
    "runner_count",
    "created_at",
    "updated_at",
];

/// `RunnerLiveStateSerializer.Meta.fields` (`serializers.py:86-99`), wire
/// order.
pub const LIVE_STATE_WIRE_FIELDS: [&str; 14] = [
    "observed_run_id",
    "last_event_at",
    "last_event_kind",
    "last_event_summary",
    "agent_pid",
    "agent_subprocess_alive",
    "approvals_pending",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "usage",
    "llm_model",
    "turn_count",
    "updated_at",
];

/// `DevMachineSerializer.Meta.fields` (`serializers.py:114-127`), wire
/// order.
pub const DEV_MACHINE_WIRE_FIELDS: [&str; 12] = [
    "id",
    "host_label",
    "label",
    "visibility",
    "runner_count",
    "online_runner_count",
    "control_online",
    "last_seen_at",
    "last_heartbeat_at",
    "revoked_at",
    "created_at",
    "updated_at",
];

/// `PodMiniSerializer.Meta.fields` (`serializers.py:140`), wire order.
pub const POD_MINI_WIRE_FIELDS: [&str; 5] =
    ["id", "name", "is_default", "project", "project_identifier"];

/// `DevMachineMiniSerializer.Meta.fields` (`serializers.py:151`), wire
/// order.
pub const DEV_MACHINE_MINI_WIRE_FIELDS: [&str; 3] = ["id", "host_label", "label"];

/// `RunnerSerializer.Meta.fields` (`serializers.py:165-190`), wire order.
pub const RUNNER_WIRE_FIELDS: [&str; 24] = [
    "id",
    "name",
    "status",
    "host_label",
    "provisioning",
    "os",
    "arch",
    "runner_version",
    "dev_metadata",
    "protocol_version",
    "capabilities",
    "last_heartbeat_at",
    "owner",
    "dev_machine",
    "dev_machine_detail",
    "visibility",
    "pod",
    "pod_detail",
    "live_state",
    "enrolled_at",
    "revoked_at",
    "revoked_reason",
    "created_at",
    "updated_at",
];

/// `RunnerEnrollRequestSerializer` declared fields
/// (`serializers.py:226-238`) in validation / error-body order.
pub const ENROLL_WIRE_FIELDS: [&str; 7] = [
    "enrollment_token",
    "dev_machine_id",
    "host_label",
    "name",
    "os",
    "arch",
    "version",
];

// ---------------------------------------------------------------------------
// Pod shapes
// ---------------------------------------------------------------------------

/// A `Pod` row for full rendering (`serializers.py:36-72`): `created_by`
/// is nullable (`SET_NULL`, `runner/models.py:81-86`); `runner_count` is
/// the caller-supplied `pod.runners.count()` — UNFILTERED, revoked
/// runners included (QUIRK-runner-count above).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub is_default: bool,
    pub workspace: &'a str,
    pub project: &'a str,
    pub project_identifier: &'a str,
    pub created_by: Option<&'a str>,
    pub runner_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `PodSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PodView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub is_default: bool,
    pub workspace: &'a str,
    pub project: &'a str,
    pub project_identifier: &'a str,
    pub created_by: Option<&'a str>,
    pub runner_count: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// Port of `PodSerializer` (`serializers.py:36-72`).
pub fn pod_to_representation<'a>(row: &'a PodRow<'a>) -> PodView<'a> {
    PodView {
        id: row.id,
        name: row.name,
        description: row.description,
        is_default: row.is_default,
        workspace: row.workspace,
        project: row.project,
        project_identifier: row.project_identifier,
        created_by: row.created_by,
        runner_count: row.runner_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// A `Pod` row for mini rendering (`serializers.py:133-142`): `project`
/// is the FK uuid, `project_identifier` the slug via
/// `source="project.identifier"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodMiniRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub is_default: bool,
    pub project: &'a str,
    pub project_identifier: &'a str,
}

/// `PodMiniSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PodMiniView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub is_default: bool,
    pub project: &'a str,
    pub project_identifier: &'a str,
}

/// Port of `PodMiniSerializer` (`serializers.py:133-142`).
pub fn pod_mini_to_representation<'a>(row: &'a PodMiniRow<'a>) -> PodMiniView<'a> {
    PodMiniView {
        id: row.id,
        name: row.name,
        is_default: row.is_default,
        project: row.project,
        project_identifier: row.project_identifier,
    }
}

// ---------------------------------------------------------------------------
// Dev-machine shapes
// ---------------------------------------------------------------------------

/// A `DevMachine` row for mini rendering (`serializers.py:145-153`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevMachineMiniRow<'a> {
    pub id: &'a str,
    pub host_label: &'a str,
    pub label: &'a str,
}

/// `DevMachineMiniSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevMachineMiniView<'a> {
    pub id: &'a str,
    pub host_label: &'a str,
    pub label: &'a str,
}

/// Port of `DevMachineMiniSerializer` (`serializers.py:145-153`).
pub fn dev_machine_mini_to_representation<'a>(
    row: &'a DevMachineMiniRow<'a>,
) -> DevMachineMiniView<'a> {
    DevMachineMiniView {
        id: row.id,
        host_label: row.host_label,
        label: row.label,
    }
}

/// A `DevMachine` row for full rendering (`serializers.py:104-130`).
/// `runner_count` / `online_runner_count` / `last_heartbeat_at` are
/// queryset annotations, not columns: `None` means the queryset did not
/// annotate them and the key is OMITTED (DRF `SkipField` — no default).
/// `last_heartbeat_at` is tri-state: omitted when unannotated, `null`
/// when `Max()` over zero rows, else the rendered datetime.
/// `control_online` defaults to `false` when unannotated
/// (`BooleanField(default=False)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevMachineRow<'a> {
    pub id: &'a str,
    pub host_label: &'a str,
    pub label: &'a str,
    pub visibility: i64,
    pub runner_count: Option<i64>,
    pub online_runner_count: Option<i64>,
    pub control_online: Option<bool>,
    pub last_seen_at: Option<&'a str>,
    pub last_heartbeat_at: Option<Option<&'a str>>,
    pub revoked_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `DevMachineSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevMachineView<'a> {
    pub id: &'a str,
    pub host_label: &'a str,
    pub label: &'a str,
    pub visibility: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_runner_count: Option<i64>,
    pub control_online: bool,
    pub last_seen_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<Option<&'a str>>,
    pub revoked_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// Port of `DevMachineSerializer` (`serializers.py:104-130`).
pub fn dev_machine_to_representation<'a>(row: &'a DevMachineRow<'a>) -> DevMachineView<'a> {
    DevMachineView {
        id: row.id,
        host_label: row.host_label,
        label: row.label,
        visibility: row.visibility,
        runner_count: row.runner_count,
        online_runner_count: row.online_runner_count,
        control_online: row.control_online.unwrap_or(false),
        last_seen_at: row.last_seen_at,
        last_heartbeat_at: row.last_heartbeat_at,
        revoked_at: row.revoked_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

// ---------------------------------------------------------------------------
// Live-state + runner shapes
// ---------------------------------------------------------------------------

/// A `RunnerLiveState` row for rendering (`serializers.py:75-101`). All
/// columns are nullable (`runner/models.py:1454-1500`); `usage` is the
/// raw JSON blob from which [`flat_token_fields`] projects the three
/// flat counters at render time (model `@property`s, `:1516-1527`).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveStateRow<'a> {
    pub observed_run_id: Option<&'a str>,
    pub last_event_at: Option<&'a str>,
    pub last_event_kind: Option<&'a str>,
    pub last_event_summary: Option<&'a str>,
    pub agent_pid: Option<i64>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i64>,
    pub usage: &'a Value,
    pub llm_model: Option<&'a str>,
    pub turn_count: Option<i64>,
    pub updated_at: &'a str,
}

/// `RunnerLiveStateSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveStateView<'a> {
    pub observed_run_id: Option<&'a str>,
    pub last_event_at: Option<&'a str>,
    pub last_event_kind: Option<&'a str>,
    pub last_event_summary: Option<&'a str>,
    pub agent_pid: Option<i64>,
    pub agent_subprocess_alive: Option<bool>,
    pub approvals_pending: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub usage: &'a Value,
    pub llm_model: Option<&'a str>,
    pub turn_count: Option<i64>,
    pub updated_at: &'a str,
}

/// Port of `RunnerLiveStateSerializer` (`serializers.py:75-101`).
pub fn live_state_to_representation<'a>(row: &'a LiveStateRow<'a>) -> LiveStateView<'a> {
    let flats = flat_token_fields(row.usage);
    LiveStateView {
        observed_run_id: row.observed_run_id,
        last_event_at: row.last_event_at,
        last_event_kind: row.last_event_kind,
        last_event_summary: row.last_event_summary,
        agent_pid: row.agent_pid,
        agent_subprocess_alive: row.agent_subprocess_alive,
        approvals_pending: row.approvals_pending,
        input_tokens: flats.input_tokens,
        output_tokens: flats.output_tokens,
        total_tokens: flats.total_tokens,
        usage: row.usage,
        llm_model: row.llm_model,
        turn_count: row.turn_count,
        updated_at: row.updated_at,
    }
}

/// A `Runner` row for full rendering (`serializers.py:156-220`).
/// `status` / `provisioning` carry the stored `TextChoices` values
/// (rendered verbatim); `visibility` the stored int. `dev_machine` is
/// nullable (`SET_NULL`, legacy enrollments); `dev_machine_detail` and
/// `live_state` render `null` — key present — when the FK / reverse
/// one-to-one row is missing. `pod` is non-null (`PROTECT`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerRow<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub status: &'a str,
    pub host_label: &'a str,
    pub provisioning: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub runner_version: &'a str,
    pub dev_metadata: &'a Value,
    pub protocol_version: i64,
    pub capabilities: &'a Value,
    pub last_heartbeat_at: Option<&'a str>,
    pub owner: &'a str,
    pub dev_machine: Option<&'a str>,
    pub dev_machine_detail: Option<DevMachineMiniRow<'a>>,
    pub visibility: i64,
    pub pod: &'a str,
    pub pod_detail: PodMiniRow<'a>,
    pub live_state: Option<LiveStateRow<'a>>,
    pub enrolled_at: Option<&'a str>,
    pub revoked_at: Option<&'a str>,
    pub revoked_reason: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// `RunnerSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunnerView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub status: &'a str,
    pub host_label: &'a str,
    pub provisioning: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub runner_version: &'a str,
    pub dev_metadata: &'a Value,
    pub protocol_version: i64,
    pub capabilities: &'a Value,
    pub last_heartbeat_at: Option<&'a str>,
    pub owner: &'a str,
    pub dev_machine: Option<&'a str>,
    pub dev_machine_detail: Option<DevMachineMiniView<'a>>,
    pub visibility: i64,
    pub pod: &'a str,
    pub pod_detail: PodMiniView<'a>,
    pub live_state: Option<LiveStateView<'a>>,
    pub enrolled_at: Option<&'a str>,
    pub revoked_at: Option<&'a str>,
    pub revoked_reason: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}

/// Port of `RunnerSerializer` (`serializers.py:156-220`).
pub fn runner_to_representation<'a>(row: &'a RunnerRow<'a>) -> RunnerView<'a> {
    RunnerView {
        id: row.id,
        name: row.name,
        status: row.status,
        host_label: row.host_label,
        provisioning: row.provisioning,
        os: row.os,
        arch: row.arch,
        runner_version: row.runner_version,
        dev_metadata: row.dev_metadata,
        protocol_version: row.protocol_version,
        capabilities: row.capabilities,
        last_heartbeat_at: row.last_heartbeat_at,
        owner: row.owner,
        dev_machine: row.dev_machine,
        dev_machine_detail: row
            .dev_machine_detail
            .as_ref()
            .map(dev_machine_mini_to_representation),
        visibility: row.visibility,
        pod: row.pod,
        pod_detail: pod_mini_to_representation(&row.pod_detail),
        live_state: row.live_state.as_ref().map(live_state_to_representation),
        enrolled_at: row.enrolled_at,
        revoked_at: row.revoked_at,
        revoked_reason: row.revoked_reason,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

// ---------------------------------------------------------------------------
// Runner-name kernel (one validator, both call sites)
// ---------------------------------------------------------------------------

/// `RUNNER_NAME_CHARSET.message` (`serializers.py:29-32`): the enroll
/// request path.
pub const RUNNER_NAME_CHARSET_MESSAGE: &str = "runner_name must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash";

/// The `invalid_runner_name` `error_description`
/// (`runner/views/enrollment.py:616-619`,
/// `runner/views/machine_commands.py:115-118`): the create + machine-command
/// paths. Same charset, different prefix (`name ...` vs `runner_name ...`).
pub const RUNNER_NAME_VIEW_MESSAGE: &str = "name must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash";

/// Shared kernel for `RUNNER_NAME_CHARSET`
/// (`^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$`, `serializers.py:28`) and
/// `_RUNNER_NAME_RE` (`^[A-Za-z0-9_][A-Za-z0-9_.\-]{0,127}$`,
/// `runner/views/enrollment.py:541`). The two patterns match the same set
/// (literal `-` at class end vs escaped `\-`; probe-verified).
///
/// Equivalence notes: Python `$` also matches just before one trailing
/// `\n`, so `'a\n'` is valid and `'a\n\n'` is not (QUIRK-name-dollar).
/// Length counts characters (non-ASCII is rejected by the class first, so
/// bytes-vs-chars never diverges).
pub fn runner_name_is_valid(name: &str) -> bool {
    // `$` before a single trailing newline (re.search / re.match parity).
    let core = name.strip_suffix('\n').unwrap_or(name);
    let mut chars = core.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    let mut len = 1;
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
            return false;
        }
        len += 1;
        if len > 128 {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Python scalar semantics (DRF validators + UUID/int parsing)
// ---------------------------------------------------------------------------

/// `str.strip()` parity: Python strips `str.isspace()` characters, which is
/// Unicode `White_Space` plus U+001C-U+001F (probe-verified set:
/// 0009-000D, 001C-001F, 0020, 0085, 00A0, 1680, 2000-200A, 2028-2029,
/// 202F, 205F, 3000).
fn py_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Code-point starts of the 68 Unicode decimal-digit (`Nd`) runs accepted
/// by Python `int()` (generated from `unicodedata.decimal`; every run is
/// exactly 10 consecutive code points with values 0-9).
const PY_DECIMAL_DIGIT_RUNS: [u32; 68] = [
    0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
    0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80,
    0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0,
    0xFF10, 0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450, 0x114D0,
    0x11650, 0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50, 0x11DA0, 0x11F50, 0x16A60,
    0x16AC0, 0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E4F0,
    0x1E950, 0x1FBF0,
];

/// Decimal digit value of `c` under Python `int()` semantics (`Nd`
/// category), or `None`.
fn py_decimal_value(c: char) -> Option<u32> {
    let cp = c as u32;
    // The ASCII run is hottest; the rest binary-searches the run table.
    if c.is_ascii_digit() {
        return Some(cp - 0x30);
    }
    let idx = PY_DECIMAL_DIGIT_RUNS
        .binary_search(&cp)
        .unwrap_or_else(|i| i.wrapping_sub(1));
    let start = *PY_DECIMAL_DIGIT_RUNS.get(idx)?;
    if cp >= start && cp < start + 10 {
        Some(cp - start)
    } else {
        None
    }
}

/// Digit value of `c` in the given radix under Python `int(s, radix)`
/// semantics: `Nd` digits map 0-9 in every radix; `a-f`/`A-F` are
/// ASCII-only (probe-verified: fullwidth letters rejected).
fn py_digit_value(c: char, radix: u32) -> Option<u32> {
    if let Some(v) = py_decimal_value(c) {
        return (v < radix).then_some(v);
    }
    if radix == 16 {
        match c {
            'a'..='f' => Some(u32::from(c as u8 - b'a') + 10),
            'A'..='F' => Some(u32::from(c as u8 - b'A') + 10),
            _ => None,
        }
    } else {
        None
    }
}

/// Port of `int(s, radix)` for radix 10/16 (UUID `hex=` path): strip the
/// `int()` whitespace set (exactly Rust `char::is_whitespace` — unlike
/// `str.strip()` it excludes U+001C-U+001F), one optional sign, an
/// optional radix-16 `0x`/`0X` prefix, digits with single underscores
/// strictly between digits. Returns `(negative, magnitude)`; `None` on
/// any `ValueError` shape. The magnitude accumulates in `u128`
/// (checked): overflow exceeds the 32-hex-digit range, so `None` is the
/// correct outcome.
fn py_int_magnitude(s: &str, radix: u32) -> Option<(bool, u128)> {
    let t = s.trim_matches(|c: char| c.is_whitespace());
    let (negative, mut digits) = match t.strip_prefix(['+', '-']) {
        Some(rest) => (t.starts_with('-'), rest),
        None => (false, t),
    };
    // Radix 16 only: skip one optional `0x`/`0X` prefix, where the `0` is
    // any Nd char with decimal value 0 and the `x` is ASCII-only. The
    // skipped prefix counts as "digit seen", so a leading `_` may follow.
    let mut allow_leading_underscore = false;
    if radix == 16 {
        let mut it = digits.chars();
        if let (Some(z), Some(x)) = (it.next(), it.next()) {
            if py_decimal_value(z) == Some(0) && matches!(x, 'x' | 'X') {
                digits = it.as_str();
                allow_leading_underscore = true;
            }
        }
    }
    let mut chars = digits.chars().peekable();
    // At least one leading digit; `_` may neither lead nor trail (except
    // directly after a skipped `0x` prefix, which counts as digit-seen).
    match chars.peek() {
        Some(&c) if py_digit_value(c, radix).is_some() => {}
        Some('_') if allow_leading_underscore => {}
        _ => return None,
    }
    let mut mag: u128 = 0;
    let mut prev_underscore = false;
    for c in chars {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let d = py_digit_value(c, radix)?;
        if prev_underscore {
            prev_underscore = false;
        }
        mag = mag
            .checked_mul(u128::from(radix))?
            .checked_add(u128::from(d))?;
    }
    if prev_underscore {
        return None;
    }
    Some((negative, mag))
}

/// Port of `str(float)` (CPython short-repr, `float_repr_style short`):
/// shortest round-trip digits, fixed notation for `10^-4 <= |v| < 10^16`,
/// scientific with a signed ≥2-digit exponent otherwise, `.0` on integral
/// fixed values. The input is value-normalized shortest digits (the same
/// digit string CPython's `repr` starts from); this only re-applies
/// CPython's notation thresholds (`1e20` -> `1e+20`, `1e-5` -> `1e-05`),
/// verified by a 20k-vector differential against `repr()`.
fn py_float_str(ryu: &str) -> String {
    // Split sign / mantissa / exponent of the shortest-digit rendering.
    let (negative, body) = match ryu.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, ryu),
    };
    let (mantissa, exp): (&str, i32) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse().unwrap_or(0)),
        None => (body, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    let mut digits = format!("{int_part}{frac_part}");
    // Value = digits × 10^(exp - frac_len); strip leading zeros for decpt.
    let frac_len = frac_part.len() as i32;
    let stripped = digits.trim_start_matches('0');
    if stripped.is_empty() {
        return if negative {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    digits = stripped.to_owned();
    let decpt = digits.len() as i32 + (exp - frac_len);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    // CPython `format_float_short`: exponent iff decpt > 16 or decpt <= -4.
    if decpt > 16 || decpt <= -4 {
        let e = decpt - 1;
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        let ae = e.unsigned_abs();
        if ae < 10 {
            out.push('0');
        }
        out.push_str(&ae.to_string());
    } else if decpt <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decpt) as usize));
        out.push_str(&digits);
    } else if decpt as usize >= digits.len() {
        out.push_str(&digits);
        out.push_str(&"0".repeat((decpt as usize) - digits.len()));
        out.push_str(".0");
    } else {
        let d = decpt as usize;
        out.push_str(&digits[..d]);
        out.push('.');
        out.push_str(&digits[d..]);
    }
    out
}

/// `str()` of an `f64` the way DRF's `CharField` sees it: finite values
/// via [`py_float_str`] over value-normalized shortest digits (immune to
/// `arbitrary_precision` literal preservation: `100.00` -> `"100.0"`);
/// non-finite per Python (`inf` / `-inf` / `nan`).
fn py_float_value_str(f: f64) -> String {
    if f.is_finite() {
        let short = serde_json::Number::from_f64(f).expect("finite").to_string();
        py_float_str(&short)
    } else if f.is_nan() {
        "nan".to_owned()
    } else if f.is_sign_negative() {
        "-inf".to_owned()
    } else {
        "inf".to_owned()
    }
}

/// `str()` of a JSON number the way DRF's `CharField` sees it
/// (`str(data)` after the `isinstance(data, (str, int, float))` gate):
/// integers render exactly, floats via [`py_float_value_str`]. The
/// `>u64`-integer residue (`2**100` renders lossy here, exact in Django)
/// is an envelope-layer precision divergence (cf. D-20), not this kernel's.
fn py_json_number_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) => py_float_value_str(f),
        // `arbitrary_precision` only: the literal is unrepresentable as
        // f64 (huge int, `1e999`). Parse it as f64: infinite -> `inf` /
        // `-inf`, finite -> shortest (lossy for `>u64` ints, see above).
        None => py_float_value_str(n.to_string().parse().unwrap_or(f64::NAN)),
    }
}

// ---------------------------------------------------------------------------
// Enroll request validation (`RunnerEnrollRequestSerializer`, :223-238)
// ---------------------------------------------------------------------------

/// DRF `Field` default messages used by the enroll body
/// (`rest_framework/fields.py`, pinned 3.15.2).
pub const ERR_REQUIRED: &str = "This field is required.";
/// `Field.default_error_messages['null']`.
pub const ERR_NULL: &str = "This field may not be null.";
/// `CharField.default_error_messages['blank']`.
pub const ERR_BLANK: &str = "This field may not be blank.";
/// `CharField.default_error_messages['invalid']`.
pub const ERR_INVALID_STRING: &str = "Not a valid string.";
/// `UUIDField.default_error_messages['invalid']`.
pub const ERR_INVALID_UUID: &str = "Must be a valid UUID.";
/// `ProhibitNullCharactersValidator.message`
/// (`django/core/validators.py`, pinned Django 4.2.30).
pub const ERR_NULL_CHARS: &str = "Null characters are not allowed.";
/// `Serializer.errors` null-body rewrite (`serializers.py:577-581`).
pub const ERR_NO_DATA: &str = "No data provided";
/// `NON_FIELD_ERRORS_KEY` (`non_field_errors`, default settings).
pub const NON_FIELD_ERRORS: &str = "non_field_errors";

/// `CharField(max_length=…)` failure
/// (`rest_framework/fields.py:726`, `MaxLengthValidator`).
fn err_max_length(max_length: usize) -> String {
    format!("Ensure this field has no more than {max_length} characters.")
}

/// `CharField(min_length=…)` failure (`fields.py:727`,
/// `MinLengthValidator`).
fn err_min_length(min_length: usize) -> String {
    format!("Ensure this field has at least {min_length} characters.")
}

/// `Serializer.to_internal_value` non-dict failure
/// (`serializers.py:483-489`): `type(data).__name__`.
fn err_not_a_dict(py_type: &str) -> String {
    format!("Invalid data. Expected a dictionary, but got {py_type}.")
}

/// Python `type(data).__name__` for a JSON body value.
fn py_type_name(body: &Value) -> &'static str {
    match body {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Validated `POST /api/v1/runner/runners/enroll/` body: every field in
/// declaration order. `dev_machine_id` is `None` when absent
/// (`required=False`, no default); `name`/`os`/`arch`/`version` fall back
/// to `""` (declared defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollValidated {
    pub enrollment_token: String,
    pub dev_machine_id: Option<String>,
    pub host_label: String,
    pub name: String,
    pub os: String,
    pub arch: String,
    pub version: String,
}

/// Field limits for the enroll body (`serializers.py:226-238`).
struct CharSpec {
    max_length: usize,
    min_length: Option<usize>,
    required: bool,
    allow_blank: bool,
    charset: bool,
}

const ENROLL_TOKEN_SPEC: CharSpec = CharSpec {
    max_length: 128,
    min_length: Some(16),
    required: true,
    allow_blank: false,
    charset: false,
};
const HOST_LABEL_SPEC: CharSpec = CharSpec {
    max_length: 255,
    min_length: None,
    required: true,
    allow_blank: false,
    charset: false,
};
const NAME_SPEC: CharSpec = CharSpec {
    max_length: 128,
    min_length: None,
    required: false,
    allow_blank: true,
    charset: true,
};
const OS_ARCH_VERSION_SPEC: CharSpec = CharSpec {
    max_length: 32,
    min_length: None,
    required: false,
    allow_blank: true,
    charset: false,
};

/// Push one message onto a field's error list (DRF collects per field).
fn push_error(errors: &mut serde_json::Map<String, Value>, field: &str, message: String) {
    errors
        .entry(field.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("error list")
        .push(Value::String(message));
}

/// Port of DRF 3.15.2 `CharField` validation (`fields.py:749-766`):
/// missing -> required/default; `None` -> null; whitespace-only (Python
/// strip set) -> blank unless `allow_blank` (validators skipped either
/// way); bool/list/dict -> invalid; str/int/float coerce via `str()`
/// then strip; then validators in order — custom charset, max-length,
/// min-length, null-characters — collecting every failure. Lengths count
/// code points. Returns the validated string, or `None` on failure /
/// when the field stays absent (optional, no default — never for the
/// char fields here, all of which default to `""`).
fn validate_char_field(
    errors: &mut serde_json::Map<String, Value>,
    field: &str,
    value: Option<&Value>,
    spec: &CharSpec,
) -> Option<String> {
    let Some(v) = value else {
        if spec.required {
            push_error(errors, field, ERR_REQUIRED.to_owned());
            return None;
        }
        return Some(String::new());
    };
    if v.is_null() {
        push_error(errors, field, ERR_NULL.to_owned());
        return None;
    }
    // `run_validation`: `data == '' or str(data).strip() == ''`. Only
    // strings can strip to empty (`str()` of every other JSON scalar is
    // non-blank), so the test narrows to strings.
    if let Value::String(s) = v {
        if py_trim(s).is_empty() {
            if !spec.allow_blank {
                push_error(errors, field, ERR_BLANK.to_owned());
                return None;
            }
            return Some(String::new());
        }
    }
    let coerced = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => py_json_number_str(n),
        _ => {
            push_error(errors, field, ERR_INVALID_STRING.to_owned());
            return None;
        }
    };
    let text = py_trim(&coerced).to_owned();
    let before = errors.len();
    // Validator order is `self.validators` order: custom first, then the
    // `__init__`-appended max / min, then the null-characters guard
    // (`fields.py:731-747`). Every failure is collected.
    if spec.charset && !runner_name_is_valid(&text) {
        push_error(errors, field, RUNNER_NAME_CHARSET_MESSAGE.to_owned());
    }
    if text.chars().count() > spec.max_length {
        push_error(errors, field, err_max_length(spec.max_length));
    }
    if let Some(min_length) = spec.min_length {
        if text.chars().count() < min_length {
            push_error(errors, field, err_min_length(min_length));
        }
    }
    if text.contains('\0') {
        push_error(errors, field, ERR_NULL_CHARS.to_owned());
    }
    if errors.len() != before {
        return None;
    }
    Some(text)
}

/// Port of `uuid.UUID(hex=s)` (`uuid.py`): global case-sensitive
/// `urn:`/`uuid:` removal, brace-strip, hyphen removal, 32-char check,
/// then `int(hex, 16)` (sign / underscores / `Nd` digits per
/// [`py_int_magnitude`]). `-` cannot survive hyphen removal, so the
/// result is never negative; the `0 <= int < 2**128` range holds by
/// construction (32 hex digits).
fn py_uuid_hex(s: &str) -> Option<u128> {
    let no_urn = s.replace("urn:", "").replace("uuid:", "");
    let no_braces = no_urn.trim_matches(|c| c == '{' || c == '}');
    let hex: String = no_braces.chars().filter(|c| *c != '-').collect();
    if hex.chars().count() != 32 {
        return None;
    }
    let (negative, mag) = py_int_magnitude(&hex, 16)?;
    if negative {
        return None;
    }
    Some(mag)
}

/// Render a `u128` as `str(uuid)` (lowercase hyphenated 8-4-4-4-12).
fn uuid_to_hex_verbose(mag: u128) -> String {
    let h = format!("{mag:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Port of DRF 3.15.2 `UUIDField.to_internal_value` (`fields.py:837-848`)
/// for the optional `dev_machine_id`: missing -> absent (no default);
/// `None` -> null; bool/int -> `uuid.UUID(int=…)` (`bool` is an `int`
/// subclass: `True` -> `...01`); str -> [`py_uuid_hex`]; float/list/dict
/// -> invalid. There is no blank check — `""` fails as invalid.
fn validate_uuid_field(
    errors: &mut serde_json::Map<String, Value>,
    field: &str,
    value: Option<&Value>,
) -> Option<Option<String>> {
    let Some(v) = value else {
        return Some(None);
    };
    if v.is_null() {
        push_error(errors, field, ERR_NULL.to_owned());
        return None;
    }
    let mag = match v {
        Value::Bool(b) => u128::from(*b as u8),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i < 0 {
                    push_error(errors, field, ERR_INVALID_UUID.to_owned());
                    return None;
                }
                i as u128
            } else if let Some(u) = n.as_u64() {
                u128::from(u)
            } else {
                push_error(errors, field, ERR_INVALID_UUID.to_owned());
                return None;
            }
        }
        Value::String(s) => match py_uuid_hex(s) {
            Some(mag) => mag,
            None => {
                push_error(errors, field, ERR_INVALID_UUID.to_owned());
                return None;
            }
        },
        _ => {
            push_error(errors, field, ERR_INVALID_UUID.to_owned());
            return None;
        }
    };
    Some(Some(uuid_to_hex_verbose(mag)))
}

/// Port of `RunnerEnrollRequestSerializer` (`serializers.py:223-238`).
/// On success returns the validated body (defaults applied, UUID
/// normalized to `hex_verbose`); on failure the DRF 400 body
/// (`{"field": ["message", ...]}`, fields in declaration order;
/// `non_field_errors` for non-dict input). Unknown keys are ignored.
pub fn validate_enroll_request(body: &Value) -> Result<EnrollValidated, Value> {
    if body.is_null() {
        return Err(Value::Object(serde_json::Map::from_iter([(
            NON_FIELD_ERRORS.to_owned(),
            Value::Array(vec![Value::String(ERR_NO_DATA.to_owned())]),
        )])));
    }
    let Value::Object(obj) = body else {
        return Err(Value::Object(serde_json::Map::from_iter([(
            NON_FIELD_ERRORS.to_owned(),
            Value::Array(vec![Value::String(err_not_a_dict(py_type_name(body)))]),
        )])));
    };
    let mut errors = serde_json::Map::new();
    let enrollment_token = validate_char_field(
        &mut errors,
        "enrollment_token",
        obj.get("enrollment_token"),
        &ENROLL_TOKEN_SPEC,
    );
    let dev_machine_id =
        validate_uuid_field(&mut errors, "dev_machine_id", obj.get("dev_machine_id"));
    let host_label = validate_char_field(
        &mut errors,
        "host_label",
        obj.get("host_label"),
        &HOST_LABEL_SPEC,
    );
    let name = validate_char_field(&mut errors, "name", obj.get("name"), &NAME_SPEC);
    let os = validate_char_field(&mut errors, "os", obj.get("os"), &OS_ARCH_VERSION_SPEC);
    let arch = validate_char_field(&mut errors, "arch", obj.get("arch"), &OS_ARCH_VERSION_SPEC);
    let version = validate_char_field(
        &mut errors,
        "version",
        obj.get("version"),
        &OS_ARCH_VERSION_SPEC,
    );
    if !errors.is_empty() {
        return Err(Value::Object(errors));
    }
    // Every field validated: `Option` unwraps are total here.
    match (
        enrollment_token,
        dev_machine_id,
        host_label,
        name,
        os,
        arch,
        version,
    ) {
        (
            Some(enrollment_token),
            Some(dev_machine_id),
            Some(host_label),
            Some(name),
            Some(os),
            Some(arch),
            Some(version),
        ) => Ok(EnrollValidated {
            enrollment_token,
            dev_machine_id,
            host_label,
            name,
            os,
            arch,
            version,
        }),
        _ => Err(Value::Object(errors)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn pod_shape_golden() {
        let row = PodRow {
            id: "11111111-1111-4111-8111-111111111111",
            name: "default",
            description: "",
            is_default: true,
            workspace: "22222222-2222-4222-8222-222222222222",
            project: "33333333-3333-4333-8333-333333333333",
            project_identifier: "PDASHOSS01",
            created_by: None,
            // Includes revoked runners (QUIRK-runner-count).
            runner_count: 3,
            created_at: "2026-10-01T00:00:00+00:00",
            updated_at: "2026-10-02T01:02:03.123456+00:00",
        };
        let body = serde_json::to_value(pod_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), POD_WIRE_FIELDS);
        assert_eq!(
            body.to_string(),
            concat!(
                "{\"id\":\"11111111-1111-4111-8111-111111111111\",\"name\":\"default\",",
                "\"description\":\"\",\"is_default\":true,",
                "\"workspace\":\"22222222-2222-4222-8222-222222222222\",",
                "\"project\":\"33333333-3333-4333-8333-333333333333\",",
                "\"project_identifier\":\"PDASHOSS01\",\"created_by\":null,\"runner_count\":3,",
                "\"created_at\":\"2026-10-01T00:00:00+00:00\",",
                "\"updated_at\":\"2026-10-02T01:02:03.123456+00:00\"}",
            )
        );
    }

    #[test]
    fn pod_mini_shape_golden() {
        let row = PodMiniRow {
            id: "11111111-1111-4111-8111-111111111111",
            name: "gpu",
            is_default: false,
            project: "33333333-3333-4333-8333-333333333333",
            project_identifier: "PDASHOSS01",
        };
        let body = serde_json::to_value(pod_mini_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), POD_MINI_WIRE_FIELDS);
        assert_eq!(
            body,
            json!({
                "id": "11111111-1111-4111-8111-111111111111",
                "name": "gpu",
                "is_default": false,
                "project": "33333333-3333-4333-8333-333333333333",
                "project_identifier": "PDASHOSS01",
            })
        );
    }

    #[test]
    fn dev_machine_mini_shape_golden() {
        let row = DevMachineMiniRow {
            id: "44444444-4444-4444-8444-444444444444",
            host_label: "mac-mini",
            label: "Iain's Mac",
        };
        let body = serde_json::to_value(dev_machine_mini_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), DEV_MACHINE_MINI_WIRE_FIELDS);
        assert_eq!(
            body.to_string(),
            concat!(
                "{\"id\":\"44444444-4444-4444-8444-444444444444\",",
                "\"host_label\":\"mac-mini\",\"label\":\"Iain's Mac\"}",
            )
        );
    }

    #[test]
    fn dev_machine_shape_annotated_golden() {
        let row = DevMachineRow {
            id: "44444444-4444-4444-8444-444444444444",
            host_label: "mac-mini",
            label: "",
            visibility: 0,
            runner_count: Some(2),
            online_runner_count: Some(1),
            control_online: Some(true),
            last_seen_at: Some("2026-10-02T01:02:03+00:00"),
            last_heartbeat_at: Some(Some("2026-10-02T01:02:04+00:00")),
            revoked_at: None,
            created_at: "2026-10-01T00:00:00+00:00",
            updated_at: "2026-10-02T01:02:03+00:00",
        };
        let body = serde_json::to_value(dev_machine_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), DEV_MACHINE_WIRE_FIELDS);
        assert_eq!(body["visibility"], json!(0));
        assert_eq!(body["runner_count"], json!(2));
        assert_eq!(body["control_online"], json!(true));
        assert_eq!(body["revoked_at"], Value::Null);
    }

    #[test]
    fn dev_machine_shape_unannotated_omits_and_defaults() {
        let row = DevMachineRow {
            id: "44444444-4444-4444-8444-444444444444",
            host_label: "mac-mini",
            label: "",
            visibility: 0,
            runner_count: None,
            online_runner_count: None,
            control_online: None,
            last_seen_at: None,
            last_heartbeat_at: None,
            revoked_at: None,
            created_at: "2026-10-01T00:00:00+00:00",
            updated_at: "2026-10-02T01:02:03+00:00",
        };
        let body = serde_json::to_value(dev_machine_to_representation(&row)).unwrap();
        assert_eq!(
            keys(&body),
            [
                "id",
                "host_label",
                "label",
                "visibility",
                "control_online",
                "last_seen_at",
                "revoked_at",
                "created_at",
                "updated_at"
            ]
        );
        // `control_online` renders false (declared default), never omitted.
        assert_eq!(body["control_online"], json!(false));
    }

    #[test]
    fn dev_machine_shape_heartbeat_null_when_max_over_zero_rows() {
        let row = DevMachineRow {
            id: "44444444-4444-4444-8444-444444444444",
            host_label: "mac-mini",
            label: "",
            visibility: 0,
            runner_count: Some(0),
            online_runner_count: Some(0),
            control_online: Some(false),
            last_seen_at: None,
            // Annotated but `Max()` over zero rows: key present, null.
            last_heartbeat_at: Some(None),
            revoked_at: None,
            created_at: "2026-10-01T00:00:00+00:00",
            updated_at: "2026-10-02T01:02:03+00:00",
        };
        let body = serde_json::to_value(dev_machine_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), DEV_MACHINE_WIRE_FIELDS);
        assert_eq!(body["last_heartbeat_at"], Value::Null);
    }

    #[test]
    fn live_state_shape_projects_flats_from_usage() {
        let usage = json!({
            "input": 12000,
            "output": 800,
            "total": 12800,
            "raw": {"input_tokens": 12000},
        });
        let row = LiveStateRow {
            observed_run_id: Some("55555555-5555-4555-8555-555555555555"),
            last_event_at: Some("2026-10-02T01:02:03.5+00:00"),
            last_event_kind: Some("tool_call"),
            last_event_summary: None,
            agent_pid: Some(4242),
            agent_subprocess_alive: Some(true),
            approvals_pending: None,
            usage: &usage,
            llm_model: Some("claude-opus-4-6"),
            turn_count: Some(7),
            updated_at: "2026-10-02T01:02:05+00:00",
        };
        let body = serde_json::to_value(live_state_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), LIVE_STATE_WIRE_FIELDS);
        assert_eq!(body["input_tokens"], json!(12000));
        assert_eq!(body["output_tokens"], json!(800));
        assert_eq!(body["total_tokens"], json!(12800));
        assert_eq!(body["usage"], usage);
        assert_eq!(body["last_event_summary"], Value::Null);
    }

    #[test]
    fn live_state_shape_all_null_with_empty_usage() {
        let usage = json!({});
        let row = LiveStateRow {
            observed_run_id: None,
            last_event_at: None,
            last_event_kind: None,
            last_event_summary: None,
            agent_pid: None,
            agent_subprocess_alive: None,
            approvals_pending: None,
            usage: &usage,
            llm_model: None,
            turn_count: None,
            updated_at: "2026-10-02T01:02:05+00:00",
        };
        let body = serde_json::to_value(live_state_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), LIVE_STATE_WIRE_FIELDS);
        for key in [
            "observed_run_id",
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "llm_model",
        ] {
            assert_eq!(body[key], Value::Null, "{key}");
        }
        assert_eq!(body["usage"], json!({}));
    }

    fn runner_row_full<'a>(
        dev_metadata: &'a Value,
        capabilities: &'a Value,
        usage: &'a Value,
    ) -> RunnerRow<'a> {
        RunnerRow {
            id: "66666666-6666-4666-8666-666666666666",
            name: "runner_001",
            status: "online",
            host_label: "mac-mini",
            provisioning: "manual",
            os: "darwin",
            arch: "arm64",
            runner_version: "1.2.3",
            dev_metadata,
            protocol_version: 4,
            capabilities,
            last_heartbeat_at: Some("2026-10-02T01:02:03+00:00"),
            owner: "77777777-7777-4777-8777-777777777777",
            dev_machine: Some("44444444-4444-4444-8444-444444444444"),
            dev_machine_detail: Some(DevMachineMiniRow {
                id: "44444444-4444-4444-8444-444444444444",
                host_label: "mac-mini",
                label: "",
            }),
            visibility: 0,
            pod: "11111111-1111-4111-8111-111111111111",
            pod_detail: PodMiniRow {
                id: "11111111-1111-4111-8111-111111111111",
                name: "default",
                is_default: true,
                project: "33333333-3333-4333-8333-333333333333",
                project_identifier: "PDASHOSS01",
            },
            live_state: Some(LiveStateRow {
                observed_run_id: None,
                last_event_at: None,
                last_event_kind: None,
                last_event_summary: None,
                agent_pid: None,
                agent_subprocess_alive: None,
                approvals_pending: None,
                usage,
                llm_model: None,
                turn_count: None,
                updated_at: "2026-10-02T01:02:05+00:00",
            }),
            enrolled_at: Some("2026-10-01T00:00:01+00:00"),
            revoked_at: None,
            revoked_reason: "",
            created_at: "2026-10-01T00:00:00+00:00",
            updated_at: "2026-10-02T01:02:03+00:00",
        }
    }

    #[test]
    fn runner_shape_golden() {
        let dev_metadata = json!({"ide": "vscode"});
        let capabilities = json!(["docker", "gpu"]);
        let usage = json!({"input": 5, "output": 6});
        let row = runner_row_full(&dev_metadata, &capabilities, &usage);
        let body = serde_json::to_value(runner_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), RUNNER_WIRE_FIELDS);
        assert_eq!(keys(&body["pod_detail"]), POD_MINI_WIRE_FIELDS);
        assert_eq!(
            keys(&body["dev_machine_detail"]),
            DEV_MACHINE_MINI_WIRE_FIELDS
        );
        assert_eq!(keys(&body["live_state"]), LIVE_STATE_WIRE_FIELDS);
        assert_eq!(body["status"], json!("online"));
        assert_eq!(body["provisioning"], json!("manual"));
        assert_eq!(body["visibility"], json!(0));
        assert_eq!(body["protocol_version"], json!(4));
        assert_eq!(body["dev_metadata"], dev_metadata);
        assert_eq!(body["capabilities"], capabilities);
        assert_eq!(body["live_state"]["input_tokens"], json!(5));
        assert_eq!(body["live_state"]["output_tokens"], json!(6));
        assert_eq!(body["live_state"]["total_tokens"], Value::Null);
        assert_eq!(body["revoked_at"], Value::Null);
        assert_eq!(body["revoked_reason"], json!(""));
    }

    #[test]
    fn runner_shape_nulls_for_legacy_enrollment() {
        let dev_metadata = json!({});
        let capabilities = json!([]);
        let usage = json!({});
        let mut row = runner_row_full(&dev_metadata, &capabilities, &usage);
        // Legacy `pidash connect` enrollment: no dev machine, no live row.
        row.dev_machine = None;
        row.dev_machine_detail = None;
        row.live_state = None;
        row.last_heartbeat_at = None;
        row.enrolled_at = None;
        let body = serde_json::to_value(runner_to_representation(&row)).unwrap();
        assert_eq!(keys(&body), RUNNER_WIRE_FIELDS);
        assert_eq!(body["dev_machine"], Value::Null);
        assert_eq!(body["dev_machine_detail"], Value::Null);
        assert_eq!(body["live_state"], Value::Null);
        // `pod_detail` is always present (non-null FK).
        assert_eq!(body["pod_detail"]["name"], json!("default"));
    }

    #[test]
    fn runner_name_kernel_matches_f6_truth_table() {
        // D13-F6 `runner_name_re.cases` + `$`-before-newline edges.
        let x128 = "x".repeat(128);
        let x129 = "x".repeat(129);
        let valid = [
            "a",
            "_x",
            "runner_001",
            "9lives",
            "UPPER.ok-dash_under",
            "dot.",
            x128.as_str(),
            // QUIRK-name-dollar: `$` matches before one trailing newline.
            "a\n",
        ];
        for name in valid {
            assert!(runner_name_is_valid(name), "{name:?}");
        }
        let invalid = [
            ".hidden",
            "-dash",
            "has space",
            x129.as_str(),
            "",
            "semi;colon",
            "a\n\n",
            "a\nb",
            "\u{e9}",
            "under_score!",
        ];
        for name in invalid {
            assert!(!runner_name_is_valid(name), "{name:?}");
        }
    }

    #[test]
    fn runner_name_messages_differ_by_call_site() {
        assert_eq!(
            RUNNER_NAME_CHARSET_MESSAGE,
            concat!(
                "runner_name must start with a letter, digit, or underscore ",
                "and contain only letters, digits, underscore, dot, or dash",
            )
        );
        assert_eq!(
            RUNNER_NAME_VIEW_MESSAGE,
            concat!(
                "name must start with a letter, digit, or underscore ",
                "and contain only letters, digits, underscore, dot, or dash",
            )
        );
    }

    fn enroll_base() -> serde_json::Map<String, Value> {
        serde_json::Map::from_iter([
            ("enrollment_token".to_owned(), json!("t".repeat(16))),
            ("host_label".to_owned(), json!("h")),
        ])
    }

    fn enroll_body(pairs: &[(&str, Value)]) -> Value {
        let mut obj = enroll_base();
        for (k, v) in pairs {
            obj.insert((*k).to_owned(), v.clone());
        }
        Value::Object(obj)
    }

    #[test]
    fn enroll_f2_error_cases_byte_exact() {
        // D13-F2 `drf_error_bodies.enroll_validation_400.cases`.
        let cases: &[(&str, Value, Value)] = &[
            (
                "empty",
                json!({}),
                json!({
                    "enrollment_token": ["This field is required."],
                    "host_label": ["This field is required."],
                }),
            ),
            (
                "blank host",
                enroll_body(&[("host_label", json!(""))]),
                json!({"host_label": ["This field may not be blank."]}),
            ),
            (
                "short token",
                enroll_body(&[("enrollment_token", json!("short"))]),
                json!({"enrollment_token": ["Ensure this field has at least 16 characters."]}),
            ),
            (
                "long token",
                enroll_body(&[("enrollment_token", json!("t".repeat(129)))]),
                json!({"enrollment_token": ["Ensure this field has no more than 128 characters."]}),
            ),
            (
                "bad uuid",
                enroll_body(&[("dev_machine_id", json!("nope"))]),
                json!({"dev_machine_id": ["Must be a valid UUID."]}),
            ),
            (
                "long os",
                enroll_body(&[("os", json!("o".repeat(33)))]),
                json!({"os": ["Ensure this field has no more than 32 characters."]}),
            ),
            (
                "bad name charset",
                enroll_body(&[("name", json!(".hidden"))]),
                json!({"name": [RUNNER_NAME_CHARSET_MESSAGE]}),
            ),
        ];
        for (label, input, expected) in cases {
            let err = validate_enroll_request(input).expect_err(label);
            assert_eq!(&err.to_string(), &expected.to_string(), "{label}");
        }
    }

    #[test]
    fn enroll_defaults_and_extras() {
        // Missing name/os/arch/version default to ""; extras ignored.
        let body = enroll_body(&[("zzz", json!(1))]);
        assert_eq!(
            validate_enroll_request(&body),
            Ok(EnrollValidated {
                enrollment_token: "t".repeat(16),
                dev_machine_id: None,
                host_label: "h".to_owned(),
                name: String::new(),
                os: String::new(),
                arch: String::new(),
                version: String::new(),
            })
        );
        // Blank name passes (allow_blank skips validators).
        let body = enroll_body(&[("name", json!(""))]);
        let valid = validate_enroll_request(&body).unwrap();
        assert_eq!(valid.name, "");
    }

    #[test]
    fn enroll_error_key_order_is_declaration_order() {
        let body = json!({
            "os": "o".repeat(40),
            "name": ".bad",
            "dev_machine_id": "nope",
            "enrollment_token": "s",
            "version": "v".repeat(40),
            "arch": "a".repeat(40),
        });
        let err = validate_enroll_request(&body).unwrap_err();
        assert_eq!(keys(&err), ENROLL_WIRE_FIELDS);
        assert_eq!(
            err.to_string(),
            format!(
                "{{\"enrollment_token\":[\"Ensure this field has at least 16 characters.\"],\
                 \"dev_machine_id\":[\"Must be a valid UUID.\"],\
                 \"host_label\":[\"This field is required.\"],\
                 \"name\":[\"{RUNNER_NAME_CHARSET_MESSAGE}\"],\
                 \"os\":[\"Ensure this field has no more than 32 characters.\"],\
                 \"arch\":[\"Ensure this field has no more than 32 characters.\"],\
                 \"version\":[\"Ensure this field has no more than 32 characters.\"]}}"
            )
        );
    }

    #[test]
    fn enroll_multi_error_order_within_field() {
        // Validators collect in order: charset, max-length, min-length,
        // null-characters. A 129-char name fails charset AND max-length.
        let body = enroll_body(&[("name", json!("x".repeat(129)))]);
        let err = validate_enroll_request(&body).unwrap_err();
        assert_eq!(
            err,
            json!({"name": [
                RUNNER_NAME_CHARSET_MESSAGE,
                "Ensure this field has no more than 128 characters.",
            ]})
        );
        // Null characters append after the earlier failures.
        let body = enroll_body(&[("host_label", json!("a\x00b"))]);
        let err = validate_enroll_request(&body).unwrap_err();
        assert_eq!(
            err,
            json!({"host_label": ["Null characters are not allowed."]})
        );
    }

    #[test]
    fn enroll_whitespace_and_coercion_edges() {
        // Numbers coerce via `str()`; bools/lists fail; null fails null.
        for (value, want) in [
            (json!(123), "123"),
            (json!(12.5), "12.5"),
            (json!(1e20), "1e+20"),
        ] {
            let body = enroll_body(&[("host_label", value)]);
            assert_eq!(validate_enroll_request(&body).unwrap().host_label, want);
        }
        for (value, message) in [
            (json!("   "), "This field may not be blank."),
            (json!(true), "Not a valid string."),
            (json!(["x"]), "Not a valid string."),
            (json!(null), "This field may not be null."),
        ] {
            let body = enroll_body(&[("host_label", value)]);
            assert_eq!(
                validate_enroll_request(&body).unwrap_err(),
                json!({"host_label": [message]})
            );
        }
        // Whitespace-only name is blank-allowed: valid as "".
        let body = enroll_body(&[("name", json!("   \t "))]);
        assert_eq!(validate_enroll_request(&body).unwrap().name, "");
        // Stripped values validate (token pads to length, name trims).
        let body = enroll_body(&[
            (
                "enrollment_token",
                json!("  ".to_owned() + &"t".repeat(20) + "  "),
            ),
            ("host_label", json!("  myhost  ")),
            ("name", json!("abc\n")),
        ]);
        let valid = validate_enroll_request(&body).unwrap();
        assert_eq!(valid.enrollment_token, "t".repeat(20));
        assert_eq!(valid.host_label, "myhost");
        assert_eq!(valid.name, "abc");
    }

    #[test]
    fn enroll_uuid_forms() {
        let canonical = "12345678-1234-5678-1234-567812345678";
        let upper = canonical.to_uppercase();
        // Accepted: hyphenated/upper/braced/urn/bare/misplaced-hyphen.
        for raw in [
            canonical,
            upper.as_str(),
            "{12345678-1234-5678-1234-567812345678}",
            "urn:uuid:12345678-1234-5678-1234-567812345678",
            "12345678123456781234567812345678",
            "1234567-81234-5678-1234-567812345678",
            "{{12345678-1234-5678-1234-567812345678}}",
        ] {
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body)
                    .unwrap()
                    .dev_machine_id
                    .as_deref(),
                Some(canonical),
                "{raw}"
            );
        }
        // `int(hex, 16)` edges: plus sign, underscores, padding.
        // Each input is 32 chars holding the same 31 hex digits.
        let shifted = "01234567-8123-4567-8123-456781234567";
        for raw in [
            "+1234567812345678123456781234567",
            "123456781234567812345678123456_7",
            " 1234567812345678123456781234567",
        ] {
            assert_eq!(raw.chars().count(), 32, "{raw}");
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body)
                    .unwrap()
                    .dev_machine_id
                    .as_deref(),
                Some(shifted),
                "{raw}"
            );
        }
        // Nd digits parse as hex too (arabic-indic x32 -> 0x12345678...).
        {
            let body = enroll_body(&[("dev_machine_id", json!("١٢٣٤٥٦٧٨".repeat(4)))]);
            assert_eq!(
                validate_enroll_request(&body)
                    .unwrap()
                    .dev_machine_id
                    .as_deref(),
                Some(canonical)
            );
        }
        // Ints (and bools, an `int` subclass) take the `int=` path.
        for (raw, want) in [
            (json!(0), "00000000-0000-0000-0000-000000000000"),
            (json!(123), "00000000-0000-0000-0000-00000000007b"),
            (json!(true), "00000000-0000-0000-0000-000000000001"),
            (json!(false), "00000000-0000-0000-0000-000000000000"),
        ] {
            let body = enroll_body(&[("dev_machine_id", raw)]);
            assert_eq!(
                validate_enroll_request(&body)
                    .unwrap()
                    .dev_machine_id
                    .as_deref(),
                Some(want)
            );
        }
        // Rejected: bad hex, wrong length, uppercase URN (case-sensitive
        // strip), unknown `int()` shapes.
        for raw in [
            "nope",
            "",
            "   ",
            "z1235678123456781234567812345678",
            "-5",
            "URN:UUID:12345678-1234-5678-1234-567812345678",
            "123456781234567812345678123456789",
        ] {
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body).unwrap_err(),
                json!({"dev_machine_id": ["Must be a valid UUID."]}),
                "{raw}"
            );
        }
        // U+001C is not stripped by `int()`: an FS-padded 32-char input
        // is rejected even though the remaining 31 chars are valid hex.
        {
            let raw = "\u{1c}".to_owned() + &"1".repeat(31);
            assert_eq!(raw.chars().count(), 32);
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body).unwrap_err(),
                json!({"dev_machine_id": ["Must be a valid UUID."]})
            );
        }
        // `int(hex, 16)` skips one optional `0x`/`0X` prefix, so 32-char
        // prefixed inputs are accepted and normalized (DRF-oracle values).
        for (raw, want) in [
            (
                "0x123456781234567812345678123456".to_owned(),
                "00123456-7812-3456-7812-345678123456",
            ),
            (
                "0X123456781234567812345678123456".to_owned(),
                "00123456-7812-3456-7812-345678123456",
            ),
            (
                "\u{0660}x123456781234567812345678123456".to_owned(),
                "00123456-7812-3456-7812-345678123456",
            ),
            (
                "0x_".to_owned() + &"1".repeat(29),
                "00011111-1111-1111-1111-111111111111",
            ),
        ] {
            assert_eq!(raw.chars().count(), 32, "{raw}");
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body)
                    .unwrap()
                    .dev_machine_id
                    .as_deref(),
                Some(want),
                "{raw}"
            );
        }
        // Prefix misses (all 32 chars) are rejected.
        for raw in [
            "1x".to_owned() + &"1".repeat(30),
            "00x".to_owned() + &"1".repeat(29),
            "0xx".to_owned() + &"1".repeat(29),
            "0x".to_owned() + &"1".repeat(29) + "_",
            "_0x".to_owned() + &"1".repeat(29),
        ] {
            assert_eq!(raw.chars().count(), 32, "{raw}");
            let body = enroll_body(&[("dev_machine_id", json!(raw))]);
            assert_eq!(
                validate_enroll_request(&body).unwrap_err(),
                json!({"dev_machine_id": ["Must be a valid UUID."]}),
                "{raw}"
            );
        }
        for raw in [json!(-5), json!(1.5), json!([]), json!({})] {
            let body = enroll_body(&[("dev_machine_id", raw)]);
            assert_eq!(
                validate_enroll_request(&body).unwrap_err(),
                json!({"dev_machine_id": ["Must be a valid UUID."]})
            );
        }
        let body = enroll_body(&[("dev_machine_id", json!(null))]);
        assert_eq!(
            validate_enroll_request(&body).unwrap_err(),
            json!({"dev_machine_id": ["This field may not be null."]})
        );
    }

    #[test]
    fn enroll_non_dict_bodies() {
        for (input, datatype) in [
            (json!([1]), "list"),
            (json!("hello"), "str"),
            (json!(5), "int"),
            (json!(1.5), "float"),
            (json!(true), "bool"),
        ] {
            assert_eq!(
                validate_enroll_request(&input).unwrap_err(),
                json!({"non_field_errors": [format!(
                    "Invalid data. Expected a dictionary, but got {datatype}."
                )]})
            );
        }
        assert_eq!(
            validate_enroll_request(&Value::Null).unwrap_err(),
            json!({"non_field_errors": ["No data provided"]})
        );
    }

    #[test]
    fn py_trim_matches_python_strip_set() {
        // Probe-generated set of chars `str.strip()` removes.
        let stripped: std::collections::HashSet<u32> = [
            0x0009, 0x000a, 0x000b, 0x000c, 0x000d, 0x001c, 0x001d, 0x001e, 0x001f, 0x0020, 0x0085,
            0x00a0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008,
            0x2009, 0x200a, 0x2028, 0x2029, 0x202f, 0x205f, 0x3000,
        ]
        .into_iter()
        .collect();
        let mut probes: Vec<u32> = (0..0x2100).collect();
        probes.extend([0x3000, 0x180e, 0x200b, 0xfeff]);
        for cp in probes {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let padded = format!("a{c}");
            let lone = c.to_string();
            let is_stripped = py_trim(&padded) == "a" && py_trim(&lone).is_empty();
            assert_eq!(is_stripped, stripped.contains(&cp), "U+{cp:04X}");
        }
    }

    #[test]
    fn py_int_vectors() {
        // (input, radix) -> (negative, magnitude); probe-verified.
        let ones40 = "1".repeat(40);
        let valid: &[(&str, u32, bool, u128)] = &[
            ("123", 10, false, 123),
            ("  12 ", 10, false, 12),
            ("1_000", 10, false, 1000),
            ("+42", 10, false, 42),
            ("-42", 10, true, 42),
            ("-0", 10, true, 0),
            ("١٢٣", 10, false, 123),
            ("１２", 16, false, 18),
            ("ab", 16, false, 0xab),
            ("AB", 16, false, 0xab),
            ("+1_2", 16, false, 18),
            ("١٢٣", 16, false, 0x123),
            ("0", 10, false, 0),
            ("ffffffffffffffffffffffffffffffff", 16, false, u128::MAX),
            // Radix-16 optional `0x`/`0X` prefix (after the sign; `0` is any
            // Nd zero, `x` ASCII-only; a leading `_` is allowed after it).
            ("0x10", 16, false, 16),
            ("0X_ABC", 16, false, 2748),
            ("+0x10", 16, false, 16),
            ("-0x10", 16, true, 16),
            ("0x_10", 16, false, 16),
            ("\u{0660}x10", 16, false, 16),
            ("0x0", 16, false, 0),
        ];
        for (s, radix, neg, mag) in valid {
            assert_eq!(
                py_int_magnitude(s, *radix),
                Some((*neg, *mag)),
                "{s:?}/{radix}"
            );
        }
        let invalid: &[(&str, u32)] = &[
            ("", 10),
            ("12.5", 10),
            ("0x10", 10),
            ("1__0", 10),
            ("_1", 10),
            ("1_", 10),
            ("+", 10),
            ("-", 16),
            ("_", 10),
            ("²", 10),
            ("Ⅷ", 16),
            ("ａｂ", 16),
            ("ＡＢ", 16),
            ("\x0012", 10),
            ("12\x00", 10),
            ("\x1c12", 10),
            ("12\x1f", 10),
            ("\x1c12", 16),
            ("12\x1f", 16),
            // Radix-16 prefix misses: non-zero `0`, doubled/misplaced
            // prefix, bare prefix, bad underscores, intervening chars.
            ("1x10", 16),
            ("00x10", 16),
            ("0xx10", 16),
            ("0x", 16),
            ("0x_", 16),
            ("0x__1", 16),
            ("0x1__2", 16),
            ("0x+10", 16),
            ("0x10_", 16),
            ("_0x10", 16),
            ("+_0x1", 16),
            ("0_x10", 16),
            ("0Xx10", 16),
            ("0xX10", 16),
            ("zz", 16),
            (ones40.as_str(), 10), // overflows u128
        ];
        for (s, radix) in invalid {
            assert_eq!(py_int_magnitude(s, *radix), None, "{s:?}/{radix}");
        }
    }

    #[test]
    fn py_float_vectors_match_repr() {
        // (f64, `repr`) pairs captured from CPython.
        let cases: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-1.0, "-1.0"),
            (12.5, "12.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1_234_567_890_123_456.0, "1234567890123456.0"),
            (1e20, "1e+20"),
            (1.5e-5, "1.5e-05"),
            (1e-4, "0.0001"),
            (0.0001, "0.0001"),
            (1e21, "1e+21"),
            (1.2345678901234567e300, "1.2345678901234567e+300"),
            (5e-324, "5e-324"),
            (2.2250738585072014e-308, "2.2250738585072014e-308"),
            (100.0, "100.0"),
            (123456.789, "123456.789"),
            (1e22, "1e+22"),
            (123_456_789_012_345_680.0, "1.2345678901234568e+17"),
        ];
        for (f, want) in cases {
            let n = serde_json::Number::from_f64(*f).unwrap();
            assert_eq!(&py_json_number_str(&n), want, "{f}");
        }
        // Integers render exactly.
        for (n, want) in [
            (serde_json::Number::from(123), "123"),
            (serde_json::Number::from(-5), "-5"),
            (serde_json::Number::from(u64::MAX), "18446744073709551615"),
        ] {
            assert_eq!(&py_json_number_str(&n), want);
        }
    }

    #[test]
    fn py_float_value_str_pins_mapping() {
        // Value-normalized mapping, pinned directly (a services-graph
        // unit test cannot construct a literal-preserving `Number`).
        for (f, want) in [
            (100.0, "100.0"),
            (12.5, "12.5"),
            (1e20, "1e+20"),
            (-0.0, "-0.0"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ] {
            assert_eq!(&py_float_value_str(f), want, "{f}");
        }
    }
}
