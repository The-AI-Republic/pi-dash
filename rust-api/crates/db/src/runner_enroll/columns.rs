//! D-13 model columns + pure methods (`runner/models.py`).
//!
//! Translation of the five D-13-owned models plus their enums and
//! revoke-reason constants: column lists in Django field-definition
//! order with defaults, `db_table`, ordering, constraints and indexes,
//! the manager semantics as `WHERE` predicates, and the model-method
//! cores (`Pod.save` denorm fill/enforcement, `default_for_project_id`,
//! `Runner.save` single-project auto-resolve, `project`/`project_id`,
//! `mark_heartbeat`, `MachineToken.revoke`, `DevMachine.__str__`).
//! Fixture source of truth: `rust-api/fixtures/runner_enroll/models/`
//! `columns.json` (D13-F1, recorded by PIDASHCONV-578, traced to exact
//! Python lines in `rust-api/fixtures/runner_enroll/TRACE.md`); the
//! `#[cfg(test)]` suite asserts these consts equal the fixture column
//! lists table by table, plus constraints, indexes, enums and the
//! pure-method decision matrices.
//!
//! Manager map (`runner/models.py`):
//! - `Pod.objects` = `PodManager` (`:42-49`): `deleted_at IS NULL`
//!   ([`pod::active_scope`]); `Pod.all_objects` is the plain manager
//!   ([`pod::all_objects_scope`]).
//! - `DevMachine`, `Runner`, `MachineToken`, `RunnerForceRefresh`
//!   declare no manager, so `objects` is the plain default manager
//!   with no scope.
//!
//! SQL here is Django-shaped (quoted identifiers, `$n` params) and
//! covers only this module's own reads/writes: the
//! `default_for_project_id` lookup, the `Runner.save` single-project
//! probe, `mark_heartbeat` and `MachineToken.revoke`. Every other
//! D-13 query lives in the queries layer (PIDASHCONV-582…584).
//!
//! Explicitly not here (translate, don't redesign):
//! - `Runner.revoke` (`:542-685`) — services-C (PIDASHCONV-588) owns
//!   it with its cross-domain cascade.
//! - `RunnerSession` / `MachineSession` / `AgentRun` columns —
//!   D-14/D-15 own those tables; D-13 pins the SQL it needs via F5/F8.
//! - `Pod.clean` (`:127-143`) shares [`pod::resolve_workspace_id`]
//!   with `Pod.save`; no call site runs `full_clean()`, so `save`
//!   carries the enforcement (`:145-159`).

use sea_query::{Alias, Condition, Expr};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Pod
// ---------------------------------------------------------------------------

/// `runner/models.py:52-176`, table `pod`.
pub mod pod {
    use super::{Alias, Condition, Expr, Uuid};

    /// Physical table (`Meta.db_table`, `:99`).
    pub const TABLE: &str = "pod";
    /// Default ordering (`Meta.ordering`, `:100`).
    pub const ORDERING: &[&str] = &["-is_default", "created_at"];
    /// Columns in Django field-definition order (`:70-93`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "workspace_id",
        "project_id",
        "name",
        "description",
        "created_by_id",
        "is_default",
        "deleted_at",
        "created_at",
        "updated_at",
    ];
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `workspace_id` has no field default — `save()` auto-fills it from
    /// the project (`:151-154`); `name` is required with no default.
    pub const DEFAULTS: &[Option<&str>] = &[
        None,          // id (uuid4 pk, client-side)
        None,          // workspace_id (required FK; save() auto-fills)
        None,          // project_id (required FK)
        None,          // name (required, no default)
        Some(""),      // description
        None,          // created_by_id (SET_NULL)
        Some("false"), // is_default
        None,          // deleted_at
        None,          // created_at (auto_now_add)
        None,          // updated_at (auto_now)
    ];
    /// Cap on pods per project (`MAX_PER_PROJECT`, `:68`). Recorded, not
    /// enforced: no in-model check and the D-13 create path does not
    /// check it either (fixture `Pod_MAX_PER_PROJECT`).
    pub const MAX_PER_PROJECT: i32 = 20;
    /// `name` width (`:81`).
    pub const NAME_MAX_LENGTH: usize = 128;
    /// `description` width and default (`:82`).
    pub const DESCRIPTION_MAX_LENGTH: usize = 512;
    /// `deleted_at` carries `db_index=True` (`:91`).
    pub const DELETED_AT_DB_INDEX: bool = true;

    /// Partial unique: pod names unique per project among live rows
    /// (`:105-109`).
    pub const UNIQUE_NAME_PER_PROJECT: &str = "pod_unique_name_per_project_when_active";
    /// Columns of [`UNIQUE_NAME_PER_PROJECT`].
    pub const UNIQUE_NAME_PER_PROJECT_COLUMNS: &[&str] = &["project_id", "name"];
    /// `WHERE` of [`UNIQUE_NAME_PER_PROJECT`].
    pub const UNIQUE_NAME_PER_PROJECT_WHERE: &str = "deleted_at IS NULL";
    /// Partial unique: one default pod per project among live rows
    /// (`:113-117`).
    pub const ONE_DEFAULT_PER_PROJECT: &str = "pod_one_default_per_project_when_active";
    /// Columns of [`ONE_DEFAULT_PER_PROJECT`].
    pub const ONE_DEFAULT_PER_PROJECT_COLUMNS: &[&str] = &["project_id"];
    /// `WHERE` of [`ONE_DEFAULT_PER_PROJECT`].
    pub const ONE_DEFAULT_PER_PROJECT_WHERE: &str = "is_default AND deleted_at IS NULL";
    /// Composite index (`:120`).
    pub const PROJECT_IS_DEFAULT_INDEX: &str = "pod_project_is_def_idx";
    /// Columns of [`PROJECT_IS_DEFAULT_INDEX`].
    pub const PROJECT_IS_DEFAULT_INDEX_COLUMNS: &[&str] = &["project_id", "is_default"];
    /// Composite index (`:121`).
    pub const WORKSPACE_IS_DEFAULT_INDEX: &str = "pod_workspc_is_def_idx";
    /// Columns of [`WORKSPACE_IS_DEFAULT_INDEX`].
    pub const WORKSPACE_IS_DEFAULT_INDEX_COLUMNS: &[&str] = &["workspace_id", "is_default"];

    /// Default read scope: live rows only (`deleted_at IS NULL`).
    ///
    /// Mirrors `PodManager.get_queryset` (`:42-49`).
    pub fn active_scope() -> Condition {
        Condition::all().add(Expr::col(Alias::new("deleted_at")).is_null())
    }

    /// Unscoped reads: no `WHERE` at all, sees tombstones.
    ///
    /// Mirrors `Pod.all_objects` (plain `Manager`, `:96`).
    pub fn all_objects_scope() -> Condition {
        Condition::all()
    }

    /// `ValidationError` field key for the workspace-denorm mismatch
    /// (`:143,158`).
    pub const WORKSPACE_MISMATCH_FIELD: &str = "workspace";
    /// `ValidationError` message for the workspace-denorm mismatch
    /// (`:143,158`).
    pub const WORKSPACE_MISMATCH_MESSAGE: &str = "pod.workspace must match pod.project.workspace";

    /// `Pod.save`/`clean` workspace-denorm violation (`:127-159`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PodWorkspaceMismatch;

    impl std::fmt::Display for PodWorkspaceMismatch {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}: {}",
                WORKSPACE_MISMATCH_FIELD, WORKSPACE_MISMATCH_MESSAGE
            )
        }
    }

    impl std::error::Error for PodWorkspaceMismatch {}

    /// `Pod.save` workspace fill + enforcement (`:145-159`), shared with
    /// `Pod.clean` (`:127-143`).
    ///
    /// `project_workspace_id` is the loaded `project.workspace_id`
    /// (`None` exactly when `project_id` itself is `None`, in which case
    /// Python skips the block entirely and the later insert fails on the
    /// `NOT NULL` project FK). Returns the `workspace_id` to persist:
    /// auto-filled from the project when omitted, kept when it matches,
    /// or [`PodWorkspaceMismatch`] on mismatch. The project row load
    /// (`self.project`, one indexed pk lookup) stays with the caller.
    pub fn resolve_workspace_id(
        workspace_id: Option<Uuid>,
        project_workspace_id: Option<Uuid>,
    ) -> Result<Option<Uuid>, PodWorkspaceMismatch> {
        match (workspace_id, project_workspace_id) {
            (_, None) => Ok(workspace_id),
            (None, Some(project_workspace_id)) => Ok(Some(project_workspace_id)),
            (Some(workspace_id), Some(project_workspace_id)) => {
                if workspace_id == project_workspace_id {
                    Ok(Some(workspace_id))
                } else {
                    Err(PodWorkspaceMismatch)
                }
            }
        }
    }

    /// `Pod.default_for_project_id` (`:173-176`):
    /// `objects.filter(project_id, is_default=True).first()` — default
    /// manager, so tombstones excluded; `None` when missing. `first()`
    /// applies `Meta.ordering` (`-is_default` is vacuous under the
    /// `is_default` filter but still emitted) with `LIMIT 1`. `$1` is
    /// the project id.
    pub const DEFAULT_FOR_PROJECT_ID_SQL: &str = "SELECT \"pod\".\"id\", \"pod\".\"workspace_id\", \"pod\".\"project_id\", \"pod\".\"name\", \"pod\".\"description\", \"pod\".\"created_by_id\", \"pod\".\"is_default\", \"pod\".\"deleted_at\", \"pod\".\"created_at\", \"pod\".\"updated_at\" FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"project_id\" = $1 AND \"pod\".\"is_default\") ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1";

    /// `Pod.__str__` (`:124-125`). `project_id` is `None` only on an
    /// unsaved instance, where Python renders `None`.
    pub fn display(name: &str, project_id: Option<&Uuid>) -> String {
        match project_id {
            Some(project_id) => format!("{name} (project={project_id})"),
            None => format!("{name} (project=None)"),
        }
    }
}

// ---------------------------------------------------------------------------
// DevMachine
// ---------------------------------------------------------------------------

/// `runner/models.py:331-379`, table `dev_machine`.
pub mod dev_machine {
    /// Physical table (`Meta.db_table`, `:367`).
    pub const TABLE: &str = "dev_machine";
    /// Default ordering (`Meta.ordering`, `:368`).
    pub const ORDERING: &[&str] = &["-last_seen_at", "-created_at"];
    /// Columns in Django field-definition order (`:338-364`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "owner_id",
        "host_label",
        "label",
        "visibility",
        "provisioning",
        "last_seen_at",
        "revoked_at",
        "created_at",
        "updated_at",
    ];
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,           // id (uuid4 pk, client-side)
        None,           // owner_id (required FK)
        Some(""),       // host_label (mutable display hint, not unique)
        Some(""),       // label
        Some("0"),      // visibility (Visibility.PRIVATE)
        Some("manual"), // provisioning (RunnerProvisioning.MANUAL)
        None,           // last_seen_at
        None,           // revoked_at
        None,           // created_at (auto_now_add)
        None,           // updated_at (auto_now)
    ];
    /// `host_label` width (`:346`). Not unique (`:344-345`).
    pub const HOST_LABEL_MAX_LENGTH: usize = 255;
    /// `label` width (`:347`).
    pub const LABEL_MAX_LENGTH: usize = 128;
    /// `visibility` default (`Visibility.PRIVATE`, `:348-352`).
    pub const DEFAULT_VISIBILITY: i16 = 0;
    /// `visibility` carries `db_index=True` (`:351`).
    pub const VISIBILITY_DB_INDEX: bool = true;
    /// `provisioning` width (`:355-360`).
    pub const PROVISIONING_MAX_LENGTH: usize = 24;
    /// `provisioning` default (`RunnerProvisioning.MANUAL`, `:358`).
    pub const DEFAULT_PROVISIONING: &str = "manual";
    /// `provisioning` carries `db_index=True` (`:359`).
    pub const PROVISIONING_DB_INDEX: bool = true;
    /// Composite index (`:370-373`).
    pub const OWNER_VISIBILITY_INDEX: &str = "dev_machine_owner_vis_idx";
    /// Columns of [`OWNER_VISIBILITY_INDEX`].
    pub const OWNER_VISIBILITY_INDEX_COLUMNS: &[&str] = &["owner_id", "visibility"];
    /// Composite index (`:374`).
    pub const OWNER_HOST_INDEX: &str = "dev_machine_owner_host_idx";
    /// Columns of [`OWNER_HOST_INDEX`].
    pub const OWNER_HOST_INDEX_COLUMNS: &[&str] = &["owner_id", "host_label"];
    /// Single-column index (`:375`).
    pub const HOST_INDEX: &str = "dev_machine_host_idx";
    /// Columns of [`HOST_INDEX`].
    pub const HOST_INDEX_COLUMNS: &[&str] = &["host_label"];

    /// `DevMachine.__str__` (`:378-379`): `label or host_label or
    /// str(id)`. Python `or` on strings falls through on empty, so the
    /// first non-empty hint wins; the id renders hyphenated lowercase,
    /// matching `Uuid`'s `Display`.
    pub fn display(label: &str, host_label: &str, id: &super::Uuid) -> String {
        if !label.is_empty() {
            label.to_owned()
        } else if !host_label.is_empty() {
            host_label.to_owned()
        } else {
            id.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// `runner/models.py:382-540` (columns + small methods; `revoke`
/// `:542-685` belongs to services-C), table `runner`.
pub mod runner {
    use super::Uuid;

    /// Physical table (`Meta.db_table`, `:479`).
    pub const TABLE: &str = "runner";
    /// Default ordering (`Meta.ordering`, `:480`).
    pub const ORDERING: &[&str] = &["-last_heartbeat_at", "-created_at"];
    /// Columns in Django field-definition order (`:394-476`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "owner_id",
        "workspace_id",
        "dev_machine_id",
        "pod_id",
        "name",
        "host_label",
        "provisioning",
        "visibility",
        "refresh_token_hash",
        "refresh_token_fingerprint",
        "refresh_token_generation",
        "previous_refresh_token_hash",
        "access_token_signing_key_version",
        "enrollment_token_hash",
        "enrollment_token_fingerprint",
        "enrolled_at",
        "capabilities",
        "status",
        "os",
        "arch",
        "runner_version",
        "dev_metadata",
        "protocol_version",
        "last_heartbeat_at",
        "free_worktrees",
        "created_at",
        "updated_at",
        "revoked_at",
        "revoked_reason",
    ];
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,            // id (uuid4 pk, client-side; db_index=True is a no-op on a pk)
        None,            // owner_id (required FK)
        None,            // workspace_id (required FK)
        None,            // dev_machine_id (SET_NULL)
        None,            // pod_id (required PROTECT FK; save() may auto-resolve)
        None,            // name (required, no default)
        Some(""),        // host_label
        Some("manual"),  // provisioning (RunnerProvisioning.MANUAL)
        Some("0"),       // visibility (Visibility.PRIVATE)
        Some(""),        // refresh_token_hash
        Some(""),        // refresh_token_fingerprint
        Some("0"),       // refresh_token_generation
        Some(""),        // previous_refresh_token_hash
        Some("1"),       // access_token_signing_key_version (reserved/unused)
        Some(""),        // enrollment_token_hash
        Some(""),        // enrollment_token_fingerprint
        None,            // enrolled_at
        Some("[]"),      // capabilities (JSONField default=list)
        Some("offline"), // status (RunnerStatus.OFFLINE)
        Some(""),        // os
        Some(""),        // arch
        Some(""),        // runner_version
        Some("{}"),      // dev_metadata (JSONField default=dict)
        Some("1"),       // protocol_version
        None,            // last_heartbeat_at
        None,            // free_worktrees (NULL = predates feature)
        None,            // created_at (auto_now_add)
        None,            // updated_at (auto_now)
        None,            // revoked_at
        Some(""),        // revoked_reason
    ];
    /// Enrollment cap (`MAX_PER_USER`, `:392`). Enforced outside the
    /// model by `matcher.can_register_another` (`count_active < 5`;
    /// `desktop_bundled` runners excluded from the count,
    /// `matcher.py:363-372`); the D-13 create endpoint does not check it.
    pub const MAX_PER_USER: i32 = 5;
    /// `name` width (`:419`).
    pub const NAME_MAX_LENGTH: usize = 128;
    /// `host_label` width (`:421`).
    pub const HOST_LABEL_MAX_LENGTH: usize = 255;
    /// `provisioning` width (`:425-430`).
    pub const PROVISIONING_MAX_LENGTH: usize = 24;
    /// `provisioning` default (`RunnerProvisioning.MANUAL`, `:428`).
    pub const DEFAULT_PROVISIONING: &str = "manual";
    /// `provisioning` carries `db_index=True` (`:429`).
    pub const PROVISIONING_DB_INDEX: bool = true;
    /// `visibility` default (`Visibility.PRIVATE`, `:431-435`).
    pub const DEFAULT_VISIBILITY: i16 = 0;
    /// `visibility` carries `db_index=True` (`:434`).
    pub const VISIBILITY_DB_INDEX: bool = true;
    /// `refresh_token_hash` width (`:439`).
    pub const REFRESH_TOKEN_HASH_MAX_LENGTH: usize = 128;
    /// `refresh_token_hash` carries `db_index=True` (`:439`).
    pub const REFRESH_TOKEN_HASH_DB_INDEX: bool = true;
    /// `refresh_token_fingerprint` width (`:440`).
    pub const REFRESH_TOKEN_FINGERPRINT_MAX_LENGTH: usize = 16;
    /// `refresh_token_generation` default (`:441`).
    pub const DEFAULT_REFRESH_TOKEN_GENERATION: i32 = 0;
    /// `previous_refresh_token_hash` width (`:444`).
    pub const PREVIOUS_REFRESH_TOKEN_HASH_MAX_LENGTH: usize = 128;
    /// `access_token_signing_key_version` default (`:446`).
    pub const DEFAULT_ACCESS_TOKEN_SIGNING_KEY_VERSION: i32 = 1;
    /// `enrollment_token_hash` width (`:449`).
    pub const ENROLLMENT_TOKEN_HASH_MAX_LENGTH: usize = 128;
    /// `enrollment_token_fingerprint` width (`:450`).
    pub const ENROLLMENT_TOKEN_FINGERPRINT_MAX_LENGTH: usize = 16;
    /// `status` width (`:453-458`).
    pub const STATUS_MAX_LENGTH: usize = 16;
    /// `status` default (`RunnerStatus.OFFLINE`, `:456`).
    pub const DEFAULT_STATUS: &str = "offline";
    /// `status` carries `db_index=True` (`:457`).
    pub const STATUS_DB_INDEX: bool = true;
    /// `os` width (`:459`).
    pub const OS_MAX_LENGTH: usize = 32;
    /// `arch` width (`:460`).
    pub const ARCH_MAX_LENGTH: usize = 32;
    /// `runner_version` width (`:461`).
    pub const RUNNER_VERSION_MAX_LENGTH: usize = 32;
    /// `protocol_version` default (`:465`).
    pub const DEFAULT_PROTOCOL_VERSION: i32 = 1;
    /// `revoked_reason` width (`:476`).
    pub const REVOKED_REASON_MAX_LENGTH: usize = 32;

    /// Runner names unique per pod (`:484-489`). No partial condition:
    /// runners are hard-revoked, never soft-deleted.
    pub const UNIQUE_NAME_PER_POD: &str = "runner_unique_name_per_pod";
    /// Columns of [`UNIQUE_NAME_PER_POD`].
    pub const UNIQUE_NAME_PER_POD_COLUMNS: &[&str] = &["pod_id", "name"];
    /// Composite index (`:491`). No explicit name — Django
    /// auto-generates one (fixture records `(auto)`).
    pub const OWNER_STATUS_INDEX_AUTO: bool = true;
    /// Columns of the `owner`/`status` index.
    pub const OWNER_STATUS_INDEX_COLUMNS: &[&str] = &["owner_id", "status"];
    /// Composite index (`:492`). No explicit name — Django
    /// auto-generates one (fixture records `(auto)`).
    pub const WORKSPACE_STATUS_INDEX_AUTO: bool = true;
    /// Columns of the `workspace`/`status` index.
    pub const WORKSPACE_STATUS_INDEX_COLUMNS: &[&str] = &["workspace_id", "status"];
    /// Composite index (`:493`).
    pub const POD_STATUS_INDEX: &str = "runner_pod_status_idx";
    /// Columns of [`POD_STATUS_INDEX`].
    pub const POD_STATUS_INDEX_COLUMNS: &[&str] = &["pod_id", "status"];
    /// Composite index (`:494-497`).
    pub const DEV_MACHINE_STATUS_INDEX: &str = "runner_dev_machine_status_idx";
    /// Columns of [`DEV_MACHINE_STATUS_INDEX`].
    pub const DEV_MACHINE_STATUS_INDEX_COLUMNS: &[&str] = &["dev_machine_id", "status"];

    /// `Runner.save` single-project probe (`:514`):
    /// `Project.objects.filter(workspace_id).values_list("id")[:2]`.
    /// `Project.objects` is the inherited `SoftDeletionManager`
    /// (`db/mixins.py:56-67` via `AuditModel`/`BaseModel`), hence the
    /// `deleted_at` scope; `ORDER BY` is `Project.Meta.ordering`
    /// (`-created_at`, `db/models/project.py:253`) applied by the slice.
    /// The ordering is irrelevant to the `len == 1` decision — only the
    /// count matters. `$1` is the workspace id.
    pub const SINGLE_PROJECT_PROBE_SQL: &str = "SELECT \"projects\".\"id\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"workspace_id\" = $1) ORDER BY \"projects\".\"created_at\" DESC LIMIT 2";
    /// The `[:2]` slice limit (`:514`).
    pub const SINGLE_PROJECT_PROBE_LIMIT: i64 = 2;

    /// `Runner.save` single-project auto pod-resolve (`:503-519`).
    ///
    /// `project_probe_ids` is the [`SINGLE_PROJECT_PROBE_SQL`] result
    /// (at most [`SINGLE_PROJECT_PROBE_LIMIT`] ids); `default_pod_id`
    /// loads `Pod.default_for_project_id` for the single project, and
    /// runs only when the probe found exactly one — mirroring Python,
    /// where the lookup sits inside the `len == 1` branch. Returns the
    /// `pod_id` to persist:
    /// - a preset `pod_id` passes through (the probe never runs);
    /// - with no pod and no workspace, `None` (the insert then fails on
    ///   the `NOT NULL` pod FK);
    /// - with no pod but a workspace, the single project's default pod
    ///   when the probe found exactly one project — which may itself be
    ///   `None` (transient window / soft-deleted default), in which case
    ///   `pod_id` stays `None` and the insert fails on the `NOT NULL`
    ///   pod FK, exactly as in Python.
    pub fn auto_resolve_pod_id(
        pod_id: Option<Uuid>,
        workspace_id: Option<Uuid>,
        project_probe_ids: &[Uuid],
        default_pod_id: impl FnOnce() -> Option<Uuid>,
    ) -> Option<Uuid> {
        if pod_id.is_some() || workspace_id.is_none() {
            return pod_id;
        }
        if project_probe_ids.len() == 1 {
            default_pod_id()
        } else {
            None
        }
    }

    /// `Runner.project` (`:521-531`): the loaded pod's project — not a
    /// column. The loader runs only when `pod_id` is `Some` (a podless
    /// in-memory runner short-circuits to `None`; persisted rows always
    /// have a pod).
    pub fn project_of<T>(pod_id: Option<Uuid>, pod_project: impl FnOnce() -> T) -> Option<T> {
        if pod_id.is_some() {
            Some(pod_project())
        } else {
            None
        }
    }

    /// `Runner.project_id` (`:532-536`): `None` when `pod_id` is `None`,
    /// else the loaded pod's `project_id`. The loader runs only when
    /// `pod_id` is `Some` (in Python, loading a missing pod raises
    /// `Pod.DoesNotExist`).
    pub fn project_id_of(
        pod_id: Option<Uuid>,
        pod_project_id: impl FnOnce() -> Uuid,
    ) -> Option<Uuid> {
        if pod_id.is_some() {
            Some(pod_project_id())
        } else {
            None
        }
    }

    /// `mark_heartbeat` (`:538-540`) writes exactly this column list:
    /// `save(update_fields=["last_heartbeat_at"])` — `updated_at`
    /// (`auto_now`) is not touched.
    pub const MARK_HEARTBEAT_UPDATE_COLUMNS: &[&str] = &["last_heartbeat_at"];
    /// `mark_heartbeat` SQL (`:538-540`). `$1` is now, `$2` the runner
    /// id. Plain pk `WHERE`: `save()` never applies a manager scope.
    pub const MARK_HEARTBEAT_SQL: &str =
        "UPDATE \"runner\" SET \"last_heartbeat_at\" = $1 WHERE \"runner\".\"id\" = $2";

    /// `Runner.__str__` (`:500-501`). `owner_id` is `None` only on an
    /// unsaved instance, where Python renders `None`.
    pub fn display(name: &str, owner_id: Option<&Uuid>) -> String {
        match owner_id {
            Some(owner_id) => format!("{name} ({owner_id})"),
            None => format!("{name} (None)"),
        }
    }
}

// ---------------------------------------------------------------------------
// MachineToken
// ---------------------------------------------------------------------------

/// `runner/models.py:811-870`, table `machine_token`.
pub mod machine_token {
    /// Physical table (`Meta.db_table`, `:846`).
    pub const TABLE: &str = "machine_token";
    /// Default ordering (`Meta.ordering`, `:847`).
    pub const ORDERING: &[&str] = &["-created_at"];
    /// Columns in Django field-definition order (`:818-843`).
    pub const COLUMNS: &[&str] = &[
        "id",
        "user_id",
        "dev_machine_id",
        "workspace_id",
        "host_label",
        "token_hash",
        "token_fingerprint",
        "label",
        "is_service",
        "created_at",
        "last_used_at",
        "revoked_at",
    ];
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    /// `host_label` and `token_hash` are required with no default.
    pub const DEFAULTS: &[Option<&str>] = &[
        None,         // id (uuid4 pk, client-side)
        None,         // user_id (required FK)
        None,         // dev_machine_id (SET_NULL)
        None,         // workspace_id (required FK)
        None,         // host_label (required, no default)
        None,         // token_hash (required, no default)
        Some(""),     // token_fingerprint
        Some(""),     // label
        Some("true"), // is_service
        None,         // created_at (auto_now_add)
        None,         // last_used_at
        None,         // revoked_at
    ];
    /// `host_label` width (`:836`).
    pub const HOST_LABEL_MAX_LENGTH: usize = 255;
    /// `token_hash` width (`:837`).
    pub const TOKEN_HASH_MAX_LENGTH: usize = 128;
    /// `token_hash` carries `db_index=True` (`:837`).
    pub const TOKEN_HASH_DB_INDEX: bool = true;
    /// `token_fingerprint` width (`:838`).
    pub const TOKEN_FINGERPRINT_MAX_LENGTH: usize = 16;
    /// `label` width (`:839`).
    pub const LABEL_MAX_LENGTH: usize = 128;
    /// `is_service` default (`:840`).
    pub const DEFAULT_IS_SERVICE: bool = true;

    /// Partial unique: one active CLI (machine-less) token per
    /// user/workspace/host (`:849-853`).
    pub const ONE_ACTIVE_PER_USER_WS_HOST: &str = "machine_token_one_active_per_user_ws_host";
    /// Columns of [`ONE_ACTIVE_PER_USER_WS_HOST`].
    pub const ONE_ACTIVE_PER_USER_WS_HOST_COLUMNS: &[&str] =
        &["user_id", "workspace_id", "host_label"];
    /// `WHERE` of [`ONE_ACTIVE_PER_USER_WS_HOST`].
    pub const ONE_ACTIVE_PER_USER_WS_HOST_WHERE: &str =
        "revoked_at IS NULL AND dev_machine_id IS NULL";
    /// Partial unique: one active token per workspace/dev-machine
    /// (`:854-858`).
    pub const ONE_ACTIVE_PER_WS_DEV_MACHINE: &str = "machine_token_one_active_per_ws_dev_machine";
    /// Columns of [`ONE_ACTIVE_PER_WS_DEV_MACHINE`].
    pub const ONE_ACTIVE_PER_WS_DEV_MACHINE_COLUMNS: &[&str] = &["workspace_id", "dev_machine_id"];
    /// `WHERE` of [`ONE_ACTIVE_PER_WS_DEV_MACHINE`].
    pub const ONE_ACTIVE_PER_WS_DEV_MACHINE_WHERE: &str =
        "revoked_at IS NULL AND dev_machine_id IS NOT NULL";
    /// Composite index (`:861`). No explicit name — Django
    /// auto-generates one (fixture records `(auto)`).
    pub const USER_WS_REVOKED_INDEX_AUTO: bool = true;
    /// Columns of the `user`/`workspace`/`revoked_at` index.
    pub const USER_WS_REVOKED_INDEX_COLUMNS: &[&str] = &["user_id", "workspace_id", "revoked_at"];
    /// Composite index (`:862`).
    pub const DEV_MACHINE_REVOKED_INDEX: &str = "machine_token_dev_rev_idx";
    /// Columns of [`DEV_MACHINE_REVOKED_INDEX`].
    pub const DEV_MACHINE_REVOKED_INDEX_COLUMNS: &[&str] = &["dev_machine_id", "revoked_at"];

    /// `revoke` (`:865-869`): no-op when `revoked_at` is already set,
    /// else stamp now. The branch only inspects presence, so the helper
    /// is generic over the timestamp representation.
    pub fn needs_revoke<T>(revoked_at: Option<T>) -> bool {
        revoked_at.is_none()
    }

    /// `revoke` (`:865-869`) writes exactly this column list:
    /// `save(update_fields=["revoked_at"])` — `updated_at` does not
    /// exist on this model, and no other column is touched.
    pub const REVOKE_UPDATE_COLUMNS: &[&str] = &["revoked_at"];
    /// `revoke` SQL (`:865-869`). `$1` is now, `$2` the token id. Plain
    /// pk `WHERE`: `save()` never applies a manager scope.
    pub const REVOKE_SQL: &str =
        "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"id\" = $2";
}

// ---------------------------------------------------------------------------
// RunnerForceRefresh
// ---------------------------------------------------------------------------

/// `runner/models.py:765-786`, table `runner_force_refresh`.
pub mod runner_force_refresh {
    /// Physical table (`Meta.db_table`, `:785`).
    pub const TABLE: &str = "runner_force_refresh";
    /// No `Meta.ordering` (`:784-785`): unordered reads.
    pub const ORDERING: &[&str] = &[];
    /// Columns in Django field-definition order (`:774-782`).
    pub const COLUMNS: &[&str] = &["runner_id", "min_rtg", "reason", "created_at"];
    /// Django field defaults in [`COLUMNS`] order (`None` = no default).
    pub const DEFAULTS: &[Option<&str>] = &[
        None,      // runner_id (OneToOne pk)
        Some("0"), // min_rtg
        Some(""),  // reason
        None,      // created_at (auto_now_add)
    ];
    /// `min_rtg` default (`:780`). While the row exists, the
    /// access-token verifier rejects tokens with `rtg < min_rtg`; the
    /// row is deleted on the next successful refresh (`:768-772`).
    pub const DEFAULT_MIN_RTG: i32 = 0;
    /// `reason` width (`:781`).
    pub const REASON_MAX_LENGTH: usize = 64;
}

// ---------------------------------------------------------------------------
// Enums + revoke reasons
// ---------------------------------------------------------------------------

/// `RunnerStatus` (`:179-183`), `Visibility` (`:186-187`),
/// `RunnerProvisioning` (`:190-204`).
pub mod enums {
    /// `RunnerStatus.ONLINE` (`:180`).
    pub const RUNNER_STATUS_ONLINE: &str = "online";
    /// `RunnerStatus.OFFLINE` (`:181`).
    pub const RUNNER_STATUS_OFFLINE: &str = "offline";
    /// `RunnerStatus.BUSY` (`:182`).
    pub const RUNNER_STATUS_BUSY: &str = "busy";
    /// `RunnerStatus.REVOKED` (`:183`).
    pub const RUNNER_STATUS_REVOKED: &str = "revoked";
    /// `RunnerStatus` values in declaration order.
    pub const RUNNER_STATUS_CHOICES: &[&str] = &["online", "offline", "busy", "revoked"];
    /// `RunnerStatus.choices`: `(value, label)` pairs (`:180-183`).
    pub const RUNNER_STATUS_LABELS: &[(&str, &str)] = &[
        ("online", "Online"),
        ("offline", "Offline"),
        ("busy", "Busy"),
        ("revoked", "Revoked"),
    ];
    /// `Visibility.PRIVATE`, the sole value (`:187`).
    pub const VISIBILITY_PRIVATE: i16 = 0;
    /// `Visibility.choices`: `(value, label)` pairs (`:187`).
    pub const VISIBILITY_LABELS: &[(i16, &str)] = &[(0, "Private")];
    /// `RunnerProvisioning.MANUAL` (`:203`).
    pub const RUNNER_PROVISIONING_MANUAL: &str = "manual";
    /// `RunnerProvisioning.DESKTOP_BUNDLED` (`:204`).
    pub const RUNNER_PROVISIONING_DESKTOP_BUNDLED: &str = "desktop_bundled";
    /// `RunnerProvisioning` values in declaration order.
    pub const RUNNER_PROVISIONING_CHOICES: &[&str] = &["manual", "desktop_bundled"];
    /// `RunnerProvisioning.choices`: `(value, label)` pairs (`:203-204`).
    pub const RUNNER_PROVISIONING_LABELS: &[(&str, &str)] = &[
        ("manual", "Enrolled by the user"),
        ("desktop_bundled", "Provisioned by Pi Dash Desktop"),
    ];
}

/// `KNOWN_REVOKE_REASONS` (`:27-37`) + `_REVOKE_REASON_MAX_LEN` (`:39`).
pub mod revoke_reasons {
    /// Canonical `Runner.revoke` reason set (`:27-37`), in source-literal
    /// order (Python holds a `frozenset`, so order is cosmetic here).
    /// `user_revoke` is deliberately outside the daemon synthesizer's
    /// canonical set — it is the cloud-only delete reason.
    pub const KNOWN_REVOKE_REASONS: &[&str] = &[
        "manual_revoke",
        "membership_revoked",
        "refresh_token_replayed",
        "runner_removed",
        "dev_machine_revoked",
        "self_revoked",
        "user_revoke",
    ];
    /// `_REVOKE_REASON_MAX_LEN` (`:39`): matches `RunnerSession` /
    /// `Runner.revoked_reason` `max_length`.
    pub const REVOKE_REASON_MAX_LEN: usize = 32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    use serde_json::Value;

    /// D13-F1 fixture (`fixtures/runner_enroll/models/columns.json`).
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../fixtures/runner_enroll/models/columns.json"
        ))
        .unwrap()
    }

    /// Column names from a fixture `fields` array of `{"name", ...}`
    /// objects, in fixture order.
    fn column_names(fields: &Value) -> Vec<String> {
        fields
            .as_array()
            .expect("fixture fields must be an array")
            .iter()
            .map(|entry| {
                entry
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("field object must carry a name")
                    .to_owned()
            })
            .collect()
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("must be an array")
            .iter()
            .map(|v| v.as_str().expect("must be a string array").to_owned())
            .collect()
    }

    /// A missing (`null`) array means "none" — the `runner_force_refresh`
    /// fixture records no `ordering` key at all.
    fn strings_or_empty(value: &Value) -> Vec<String> {
        if value.is_null() {
            Vec::new()
        } else {
            strings(value)
        }
    }

    fn const_names(columns: &[&str]) -> Vec<String> {
        columns.iter().map(ToString::to_string).collect()
    }

    /// Render `SELECT "id" FROM <table> WHERE <scope>` in Postgres dialect.
    fn select_where(table: &str, scope: Condition) -> String {
        let mut select = Query::select();
        select
            .column(Alias::new("id"))
            .from(Alias::new(table))
            .cond_where(scope);
        select.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn all_models_match_fixture() {
        let fixture = fixture();
        assert_eq!(fixture["fixture_id"].as_str().unwrap(), "D13-F1");
        let models = &fixture["models"];
        for (key, table, columns, ordering, defaults) in [
            (
                "pod",
                pod::TABLE,
                pod::COLUMNS,
                pod::ORDERING,
                pod::DEFAULTS,
            ),
            (
                "dev_machine",
                dev_machine::TABLE,
                dev_machine::COLUMNS,
                dev_machine::ORDERING,
                dev_machine::DEFAULTS,
            ),
            (
                "runner",
                runner::TABLE,
                runner::COLUMNS,
                runner::ORDERING,
                runner::DEFAULTS,
            ),
            (
                "machine_token",
                machine_token::TABLE,
                machine_token::COLUMNS,
                machine_token::ORDERING,
                machine_token::DEFAULTS,
            ),
            (
                "runner_force_refresh",
                runner_force_refresh::TABLE,
                runner_force_refresh::COLUMNS,
                runner_force_refresh::ORDERING,
                runner_force_refresh::DEFAULTS,
            ),
        ] {
            let model = &models[key];
            assert_eq!(model["db_table"].as_str().unwrap(), table, "{key}");
            assert_eq!(strings_or_empty(&model["ordering"]), ordering, "{key}");
            assert_eq!(
                column_names(&model["fields"]),
                const_names(columns),
                "{key}"
            );
            assert_eq!(
                defaults.len(),
                columns.len(),
                "{key}: one default slot per column"
            );
        }
    }

    #[test]
    fn constraints_match_fixture() {
        let fixture = fixture();
        let constraints = &fixture["constraints"];
        for (key, name, fields, condition) in [
            (
                "pod",
                pod::UNIQUE_NAME_PER_PROJECT,
                pod::UNIQUE_NAME_PER_PROJECT_COLUMNS,
                Some(pod::UNIQUE_NAME_PER_PROJECT_WHERE),
            ),
            (
                "pod",
                pod::ONE_DEFAULT_PER_PROJECT,
                pod::ONE_DEFAULT_PER_PROJECT_COLUMNS,
                Some(pod::ONE_DEFAULT_PER_PROJECT_WHERE),
            ),
            (
                "runner",
                runner::UNIQUE_NAME_PER_POD,
                runner::UNIQUE_NAME_PER_POD_COLUMNS,
                None,
            ),
            (
                "machine_token",
                machine_token::ONE_ACTIVE_PER_USER_WS_HOST,
                machine_token::ONE_ACTIVE_PER_USER_WS_HOST_COLUMNS,
                Some(machine_token::ONE_ACTIVE_PER_USER_WS_HOST_WHERE),
            ),
            (
                "machine_token",
                machine_token::ONE_ACTIVE_PER_WS_DEV_MACHINE,
                machine_token::ONE_ACTIVE_PER_WS_DEV_MACHINE_COLUMNS,
                Some(machine_token::ONE_ACTIVE_PER_WS_DEV_MACHINE_WHERE),
            ),
        ] {
            let found = constraints[key]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"].as_str().unwrap() == name)
                .unwrap_or_else(|| panic!("fixture constraint {name} missing"));
            assert_eq!(
                found["type"].as_str().unwrap(),
                "UniqueConstraint",
                "{name}"
            );
            assert_eq!(strings(&found["fields"]), const_names(fields), "{name}");
            match (condition, found.get("condition")) {
                (Some(expected), Some(actual)) => {
                    assert_eq!(actual.as_str().unwrap(), expected, "{name}")
                }
                (None, None) => {}
                (expected, actual) => {
                    panic!("{name}: condition mismatch {expected:?} vs {actual:?}")
                }
            }
        }
    }

    /// Fixture index entries are `{"fields", "name"?}` objects (name
    /// `"(auto)"` for unnamed Django indexes, or an `implicit` note for
    /// `db_index=True` columns).
    fn fixture_index<'a>(indexes: &'a Value, key: &str, fields: &[&str]) -> &'a Value {
        indexes[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry.is_object() && strings(&entry["fields"]) == const_names(fields))
            .unwrap_or_else(|| panic!("fixture index on {fields:?} missing under {key}"))
    }

    #[test]
    fn indexes_match_fixture() {
        let fixture = fixture();
        let indexes = &fixture["indexes"];
        // Named composite indexes.
        for (key, name, fields) in [
            (
                "pod",
                pod::PROJECT_IS_DEFAULT_INDEX,
                pod::PROJECT_IS_DEFAULT_INDEX_COLUMNS,
            ),
            (
                "pod",
                pod::WORKSPACE_IS_DEFAULT_INDEX,
                pod::WORKSPACE_IS_DEFAULT_INDEX_COLUMNS,
            ),
            (
                "dev_machine",
                dev_machine::OWNER_VISIBILITY_INDEX,
                dev_machine::OWNER_VISIBILITY_INDEX_COLUMNS,
            ),
            (
                "dev_machine",
                dev_machine::OWNER_HOST_INDEX,
                dev_machine::OWNER_HOST_INDEX_COLUMNS,
            ),
            (
                "dev_machine",
                dev_machine::HOST_INDEX,
                dev_machine::HOST_INDEX_COLUMNS,
            ),
            (
                "runner",
                runner::POD_STATUS_INDEX,
                runner::POD_STATUS_INDEX_COLUMNS,
            ),
            (
                "runner",
                runner::DEV_MACHINE_STATUS_INDEX,
                runner::DEV_MACHINE_STATUS_INDEX_COLUMNS,
            ),
            (
                "machine_token",
                machine_token::DEV_MACHINE_REVOKED_INDEX,
                machine_token::DEV_MACHINE_REVOKED_INDEX_COLUMNS,
            ),
        ] {
            let entry = fixture_index(indexes, key, fields);
            assert_eq!(entry["name"].as_str().unwrap(), name, "{name}");
        }
        // Django auto-named indexes.
        for (key, fields) in [
            ("runner", runner::OWNER_STATUS_INDEX_COLUMNS),
            ("runner", runner::WORKSPACE_STATUS_INDEX_COLUMNS),
            (
                "machine_token",
                machine_token::USER_WS_REVOKED_INDEX_COLUMNS,
            ),
        ] {
            let entry = fixture_index(indexes, key, fields);
            assert_eq!(entry["name"].as_str().unwrap(), "(auto)", "{fields:?}");
        }
        const {
            assert!(runner::OWNER_STATUS_INDEX_AUTO);
            assert!(runner::WORKSPACE_STATUS_INDEX_AUTO);
            assert!(machine_token::USER_WS_REVOKED_INDEX_AUTO);
        }
        // `db_index=True` single-column entries.
        for (key, column) in [
            ("pod", "deleted_at"),
            ("dev_machine", "visibility"),
            ("dev_machine", "provisioning"),
            ("runner", "id"),
            ("runner", "provisioning"),
            ("runner", "visibility"),
            ("runner", "refresh_token_hash"),
            ("runner", "status"),
            ("machine_token", "token_hash"),
        ] {
            let entry = fixture_index(indexes, key, &[column]);
            assert!(
                entry.get("implicit").is_some(),
                "{key}.{column} must be a db_index-implicit entry"
            );
        }
        const {
            assert!(pod::DELETED_AT_DB_INDEX);
            assert!(dev_machine::VISIBILITY_DB_INDEX);
            assert!(dev_machine::PROVISIONING_DB_INDEX);
            assert!(runner::PROVISIONING_DB_INDEX);
            assert!(runner::VISIBILITY_DB_INDEX);
            assert!(runner::REFRESH_TOKEN_HASH_DB_INDEX);
            assert!(runner::STATUS_DB_INDEX);
            assert!(machine_token::TOKEN_HASH_DB_INDEX);
        }
        // Force-refresh: no indexes beyond the pk.
        assert_eq!(
            indexes["runner_force_refresh"][0].as_str().unwrap(),
            "(none: single-column PK only)"
        );
    }

    #[test]
    fn enums_and_revoke_reasons_match_fixture() {
        let fixture = fixture();
        let enums = &fixture["enums"];
        assert_eq!(
            enums["RunnerStatus"]["type"].as_str().unwrap(),
            "TextChoices"
        );
        for (member, value) in [
            ("ONLINE", enums::RUNNER_STATUS_ONLINE),
            ("OFFLINE", enums::RUNNER_STATUS_OFFLINE),
            ("BUSY", enums::RUNNER_STATUS_BUSY),
            ("REVOKED", enums::RUNNER_STATUS_REVOKED),
        ] {
            assert_eq!(enums["RunnerStatus"][member].as_str().unwrap(), value);
        }
        assert_eq!(
            fixture["defaults_and_python_behavior"]["RunnerStatus_values"],
            serde_json::json!(enums::RUNNER_STATUS_CHOICES),
        );
        assert_eq!(
            enums["RunnerProvisioning"]["type"].as_str().unwrap(),
            "TextChoices"
        );
        assert_eq!(
            enums["RunnerProvisioning"]["MANUAL"].as_str().unwrap(),
            enums::RUNNER_PROVISIONING_MANUAL,
        );
        assert_eq!(
            enums["RunnerProvisioning"]["DESKTOP_BUNDLED"]
                .as_str()
                .unwrap(),
            enums::RUNNER_PROVISIONING_DESKTOP_BUNDLED,
        );
        assert_eq!(enums["Visibility"]["PRIVATE"].as_i64().unwrap(), 0);
        assert_eq!(enums::VISIBILITY_PRIVATE, 0);
        let reasons = &fixture["revoke_reasons"];
        assert_eq!(
            strings(&reasons["KNOWN_REVOKE_REASONS"]),
            const_names(revoke_reasons::KNOWN_REVOKE_REASONS),
        );
        assert_eq!(
            reasons["REVOKE_REASON_MAX_LEN"].as_u64().unwrap() as usize,
            revoke_reasons::REVOKE_REASON_MAX_LEN,
        );
    }

    #[test]
    fn documented_defaults_match_fixture_prose() {
        let fixture = fixture();
        let behavior = &fixture["defaults_and_python_behavior"];
        // Each const below is pinned by the fixture's prose note; the
        // starts_with ties the const to the recorded Python value.
        assert!(behavior["Pod_MAX_PER_PROJECT"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{} ", pod::MAX_PER_PROJECT)));
        assert!(behavior["Runner_MAX_PER_USER"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{} ", runner::MAX_PER_USER)));
        assert_eq!(pod::MAX_PER_PROJECT, 20);
        assert_eq!(runner::MAX_PER_USER, 5);
        assert!(behavior["Runner_status_default"]
            .as_str()
            .unwrap()
            .contains(runner::DEFAULT_STATUS));
        assert!(behavior["Runner_provisioning_default"]
            .as_str()
            .unwrap()
            .contains(runner::DEFAULT_PROVISIONING));
        assert!(behavior["Runner_revoked_reason_width"]
            .as_str()
            .unwrap()
            .contains(&format!("varchar({})", runner::REVOKED_REASON_MAX_LENGTH)));
        assert_eq!(
            runner::REVOKED_REASON_MAX_LENGTH,
            revoke_reasons::REVOKE_REASON_MAX_LEN
        );
    }

    #[test]
    fn pod_scopes_render() {
        assert_eq!(
            select_where(pod::TABLE, pod::active_scope()),
            "SELECT \"id\" FROM \"pod\" WHERE \"deleted_at\" IS NULL"
        );
        // An empty `Condition::all()` renders as `WHERE TRUE`: no row is
        // filtered out (same precedent as `space::unscoped_scope`).
        assert_eq!(
            select_where(pod::TABLE, pod::all_objects_scope()),
            "SELECT \"id\" FROM \"pod\" WHERE TRUE"
        );
    }

    #[test]
    fn pod_default_lookup_sql_shape() {
        let sql = pod::DEFAULT_FOR_PROJECT_ID_SQL;
        // Select list is exactly the model's columns, quoted.
        let list = sql
            .strip_prefix("SELECT ")
            .and_then(|rest| rest.split_once(" FROM \"pod\""))
            .expect("default lookup must select from pod")
            .0;
        let selected: Vec<String> = list
            .split(", ")
            .map(|col| {
                col.strip_prefix("\"pod\".\"")
                    .and_then(|c| c.strip_suffix('"'))
                    .expect("column must be pod-qualified")
                    .to_owned()
            })
            .collect();
        assert_eq!(selected, const_names(pod::COLUMNS));
        // Manager scope + filter + Meta ordering + first().
        assert!(sql.contains(
            "WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"project_id\" = $1 AND \"pod\".\"is_default\")"
        ));
        assert!(sql
            .ends_with("ORDER BY \"pod\".\"is_default\" DESC, \"pod\".\"created_at\" ASC LIMIT 1"));
    }

    #[test]
    fn pod_resolve_workspace_matrix() {
        let ws = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        // No project: Python skips the block; workspace passes through.
        assert_eq!(pod::resolve_workspace_id(None, None), Ok(None));
        assert_eq!(pod::resolve_workspace_id(Some(ws), None), Ok(Some(ws)));
        // Auto-fill from the project when omitted.
        assert_eq!(pod::resolve_workspace_id(None, Some(ws)), Ok(Some(ws)));
        // Match persists.
        assert_eq!(pod::resolve_workspace_id(Some(ws), Some(ws)), Ok(Some(ws)));
        // Mismatch raises.
        assert_eq!(
            pod::resolve_workspace_id(Some(ws), Some(other)),
            Err(pod::PodWorkspaceMismatch)
        );
        assert_eq!(pod::WORKSPACE_MISMATCH_FIELD, "workspace");
        assert_eq!(
            pod::WORKSPACE_MISMATCH_MESSAGE,
            "pod.workspace must match pod.project.workspace"
        );
        assert_eq!(
            pod::PodWorkspaceMismatch.to_string(),
            "workspace: pod.workspace must match pod.project.workspace"
        );
    }

    #[test]
    fn runner_save_probe_sql_shape() {
        let sql = runner::SINGLE_PROJECT_PROBE_SQL;
        assert!(sql.starts_with("SELECT \"projects\".\"id\" FROM \"projects\" WHERE ("));
        // SoftDeletionManager scope + workspace filter.
        assert!(sql.contains("\"projects\".\"deleted_at\" IS NULL"));
        assert!(sql.contains("\"projects\".\"workspace_id\" = $1"));
        // Project Meta.ordering + [:2] slice.
        assert!(sql.ends_with("ORDER BY \"projects\".\"created_at\" DESC LIMIT 2"));
        assert_eq!(runner::SINGLE_PROJECT_PROBE_LIMIT, 2);
    }

    #[test]
    fn runner_auto_resolve_matrix() {
        let ws = Uuid::from_u128(10);
        let preset = Uuid::from_u128(11);
        let project = Uuid::from_u128(12);
        let default_pod = Uuid::from_u128(13);
        // Preset pod passes through; neither the probe nor the default
        // lookup runs.
        assert_eq!(
            runner::auto_resolve_pod_id(Some(preset), Some(ws), &[], || {
                panic!("default lookup must not run")
            }),
            Some(preset)
        );
        // No workspace: stays None (insert fails on NOT NULL pod FK).
        assert_eq!(
            runner::auto_resolve_pod_id(None, None, &[], || {
                panic!("default lookup must not run")
            }),
            None
        );
        // Exactly one project: its default pod (may itself be None).
        assert_eq!(
            runner::auto_resolve_pod_id(None, Some(ws), &[project], || Some(default_pod)),
            Some(default_pod)
        );
        assert_eq!(
            runner::auto_resolve_pod_id(None, Some(ws), &[project], || None),
            None
        );
        // Zero or many projects: no resolve, no default lookup.
        assert_eq!(
            runner::auto_resolve_pod_id(None, Some(ws), &[], || {
                panic!("default lookup must not run")
            }),
            None
        );
        assert_eq!(
            runner::auto_resolve_pod_id(None, Some(ws), &[project, preset], || {
                panic!("default lookup must not run")
            }),
            None
        );
    }

    #[test]
    fn runner_project_accessors() {
        let pod_id = Uuid::from_u128(20);
        let project_id = Uuid::from_u128(21);
        assert_eq!(
            runner::project_id_of(Some(pod_id), || project_id),
            Some(project_id)
        );
        assert_eq!(
            runner::project_id_of(None, || panic!("loader must not run")),
            None
        );
        assert_eq!(runner::project_of(Some(pod_id), || "proj"), Some("proj"));
        assert_eq!(
            runner::project_of(None, || panic!("loader must not run")),
            None::<&str>
        );
    }

    #[test]
    fn heartbeat_and_revoke_sql() {
        assert_eq!(
            runner::MARK_HEARTBEAT_UPDATE_COLUMNS,
            &["last_heartbeat_at"]
        );
        assert_eq!(
            runner::MARK_HEARTBEAT_SQL,
            "UPDATE \"runner\" SET \"last_heartbeat_at\" = $1 WHERE \"runner\".\"id\" = $2"
        );
        assert_eq!(machine_token::REVOKE_UPDATE_COLUMNS, &["revoked_at"]);
        assert_eq!(
            machine_token::REVOKE_SQL,
            "UPDATE \"machine_token\" SET \"revoked_at\" = $1 WHERE \"machine_token\".\"id\" = $2"
        );
        assert!(machine_token::needs_revoke::<u8>(None));
        assert!(!machine_token::needs_revoke(Some(1u8)));
    }

    #[test]
    fn display_strings() {
        let id = Uuid::from_u128(0x12345678_1234_5678_1234_567812345678);
        // DevMachine: first non-empty hint wins, else the id.
        assert_eq!(dev_machine::display("Main", "host", &id), "Main");
        assert_eq!(dev_machine::display("", "host", &id), "host");
        assert_eq!(
            dev_machine::display("", "", &id),
            "12345678-1234-5678-1234-567812345678"
        );
        // Pod / Runner: None only on unsaved instances.
        assert_eq!(
            pod::display("web", Some(&id)),
            "web (project=12345678-1234-5678-1234-567812345678)"
        );
        assert_eq!(pod::display("web", None), "web (project=None)");
        assert_eq!(
            runner::display("r1", Some(&id)),
            "r1 (12345678-1234-5678-1234-567812345678)"
        );
        assert_eq!(runner::display("r1", None), "r1 (None)");
    }

    #[test]
    fn field_widths_and_scalar_defaults() {
        assert_eq!(pod::NAME_MAX_LENGTH, 128);
        assert_eq!(pod::DESCRIPTION_MAX_LENGTH, 512);
        assert_eq!(dev_machine::HOST_LABEL_MAX_LENGTH, 255);
        assert_eq!(dev_machine::LABEL_MAX_LENGTH, 128);
        assert_eq!(dev_machine::PROVISIONING_MAX_LENGTH, 24);
        assert_eq!(dev_machine::DEFAULT_PROVISIONING, "manual");
        assert_eq!(dev_machine::DEFAULT_VISIBILITY, 0);
        assert_eq!(runner::NAME_MAX_LENGTH, 128);
        assert_eq!(runner::HOST_LABEL_MAX_LENGTH, 255);
        assert_eq!(runner::PROVISIONING_MAX_LENGTH, 24);
        assert_eq!(runner::DEFAULT_PROVISIONING, "manual");
        assert_eq!(runner::DEFAULT_VISIBILITY, 0);
        assert_eq!(runner::REFRESH_TOKEN_HASH_MAX_LENGTH, 128);
        assert_eq!(runner::REFRESH_TOKEN_FINGERPRINT_MAX_LENGTH, 16);
        assert_eq!(runner::DEFAULT_REFRESH_TOKEN_GENERATION, 0);
        assert_eq!(runner::PREVIOUS_REFRESH_TOKEN_HASH_MAX_LENGTH, 128);
        assert_eq!(runner::DEFAULT_ACCESS_TOKEN_SIGNING_KEY_VERSION, 1);
        assert_eq!(runner::ENROLLMENT_TOKEN_HASH_MAX_LENGTH, 128);
        assert_eq!(runner::ENROLLMENT_TOKEN_FINGERPRINT_MAX_LENGTH, 16);
        assert_eq!(runner::STATUS_MAX_LENGTH, 16);
        assert_eq!(runner::DEFAULT_STATUS, "offline");
        assert_eq!(runner::OS_MAX_LENGTH, 32);
        assert_eq!(runner::ARCH_MAX_LENGTH, 32);
        assert_eq!(runner::RUNNER_VERSION_MAX_LENGTH, 32);
        assert_eq!(runner::DEFAULT_PROTOCOL_VERSION, 1);
        assert_eq!(machine_token::HOST_LABEL_MAX_LENGTH, 255);
        assert_eq!(machine_token::TOKEN_HASH_MAX_LENGTH, 128);
        assert_eq!(machine_token::TOKEN_FINGERPRINT_MAX_LENGTH, 16);
        assert_eq!(machine_token::LABEL_MAX_LENGTH, 128);
        const {
            assert!(machine_token::DEFAULT_IS_SERVICE);
        }
        assert_eq!(runner_force_refresh::DEFAULT_MIN_RTG, 0);
        assert_eq!(runner_force_refresh::REASON_MAX_LENGTH, 64);
        // Enum label pairs mirror Django .choices labels.
        assert_eq!(
            enums::RUNNER_STATUS_LABELS,
            &[
                ("online", "Online"),
                ("offline", "Offline"),
                ("busy", "Busy"),
                ("revoked", "Revoked"),
            ]
        );
        assert_eq!(enums::VISIBILITY_LABELS, &[(0, "Private")]);
        assert_eq!(
            enums::RUNNER_PROVISIONING_LABELS,
            &[
                ("manual", "Enrolled by the user"),
                ("desktop_bundled", "Provisioned by Pi Dash Desktop"),
            ]
        );
    }
}
