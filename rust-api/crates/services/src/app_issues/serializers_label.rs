#![forbid(unsafe_code)]

//! App `LabelSerializer` + `validate_name` for app:issues (D-26).
//!
//! Port of `apps/api/pi_dash/app/serializers/issue.py:549-575`
//! (`LabelSerializer`: 7-key `Meta.fields` + `read_only_fields` +
//! `validate_name`). Consumers: `WorkspaceLabelsEndpoint.get`
//! (`app/views/workspace/label.py:22-30`, read-only) and `LabelViewSet`
//! (`app/views/issue/label.py`: list/create/retrieve/update/destroy) plus
//! `BulkCreateIssueLabelsEndpoint` (`label.py:92-117`, read-only).
//!
//! This is the 7-key *app* class, not the `fields = "__all__"` *space*
//! class (`space/serializer/issue.py:50-57`, ported as
//! `space::serializers::taxonomy::LabelView`): different keys, different
//! wire bytes. The app `LabelLiteSerializer` (`issue.py:577-581`) is NOT
//! re-ported here — it is already merged as
//! [`super::serializers_refs::label_lite_to_representation`] (same class,
//! same `[id, name, color]` order, byte-tested); a second copy would be a
//! parallel port.
//!
//! Field mapping (probed on the pinned Django 4.2.30 / DRF 3.15.2 with
//! model-mirror fields; read bytes cross-checked against the D-24
//! live-Django oracle, PIDASHCONV-621):
//!
//! * Explicit `Meta.fields` lists render verbatim:
//!   `parent, name, color, id, project_id, workspace_id, sort_order`.
//! * `parent` is an auto `PrimaryKeyRelatedField` (self-FK, `null=True,
//!   blank=True` → `required=False`); `None` renders `null`.
//! * `name` / `color` are auto `CharField`s (`max_length=255`); `name` is
//!   `required=True`, `color` (`blank=True`) is `required=False` and may
//!   render `""`.
//! * `id` is the declared `PrimaryKeyRelatedField(read_only=True)` from
//!   `BaseSerializer` (`serializers/base.py:8-9`) and renders the UUID
//!   string.
//! * `project_id` / `workspace_id` are FK attnames, not model fields: DRF
//!   resolves them through the `hasattr(model_class, …)` property fallback
//!   (`ModelSerializer.build_field`) as read-only `ReadOnlyField`s. They
//!   render in reads and are silently dropped from writes.
//!   `project` is nullable (`db/models/workspace.py:187`), so `project_id`
//!   may render `null`; `workspace` is not.
//! * `sort_order` is an auto `FloatField` (`required=False` via the
//!   `default=65535`); DRF renders the float (`65535.0`).
//!
//! Write guards: the writable set is `{parent, name, color, sort_order}`;
//! `{id, project_id, workspace_id}` are read-only and silently dropped
//! from input, as are unknown keys and the `workspace` / `project` names
//! (DRF `to_internal_value` only reads `_writable_fields`; probed).
//! Serialize the [`AppLabelView`] struct directly (`to_string`): this
//! crate's `serde_json` has no `preserve_order`, so `to_value` would
//! re-sort keys.
//!
//! Ported quirk (translate, don't redesign): `read_only_fields =
//! ["workspace", "project"]` (`:561`) is dead — neither name is in
//! `Meta.fields`, and DRF only consumes `extra_kwargs` per declared field
//! name (`ModelSerializer.get_fields`), so the entries are silently
//! ignored (probed: the serializer builds and both names stay absent).
//! The real guards are the three read-only fields above.
//!
//! `validate_name` (`:563-574`) runs only after the `name` field itself
//! validates (so the value is a non-empty string): it probes
//! `Label.objects.filter(project_id=…, name__iexact=…)` — soft-deleted
//! rows excluded by `SoftDeletionManager`
//! (`db/mixins.py:56-58`) — excluding the instance's own pk on
//! update, and raises `ValidationError("LABEL_NAME_ALREADY_EXISTS")`
//! when a row exists. On Postgres `iexact` compiles to
//! `UPPER(name::text) = UPPER($n)` with the raw value bound
//! (`postgresql/base.py` operators, `operations.py` `lookup_cast` /
//! `prep_for_iexact_query`); the comparison stays DB-side because
//! Python-casefolding and Postgres-`UPPER` disagree on some Unicode
//! (e.g. `ß` vs `SS`). The `project_id=None` arm (`context.get` +
//! Django `None → IS NULL`) is unreachable from the D-26 views, which
//! always pass the URL `project_id` (`label.py:43,71`); only the
//! `= $1` form is pinned here.
//!
//! Out of scope here (655's handler layer): the `Label.save()`
//! `sort_order = max + 10000` override (`db/models/label.py:46-54`),
//! the `partial_update` exact-match pre-check (`label.py:60-70`, which
//! is case-sensitive where the serializer is `iexact` — asymmetry kept),
//! and the generic field errors (required / max-length / bad-UUID).

use serde::Serialize;

/// App `LabelSerializer` wire keys (`issue.py:549-562`), in
/// `Meta.fields` order.
pub const APP_LABEL_FIELDS: [&str; 7] = [
    "parent",
    "name",
    "color",
    "id",
    "project_id",
    "workspace_id",
    "sort_order",
];

/// A `Label` row for app rendering (`db/models/label.py:11-25`,
/// `workspace.py:185-187`): `parent` (self-FK, `null=True`) and
/// `project_id` (`project`, `null=True`) are nullable; `workspace_id` is
/// not. `color` (`blank=True`) may be `""`.
#[derive(Debug, Clone, PartialEq)]
pub struct AppLabelRow<'a> {
    pub parent: Option<&'a str>,
    pub name: &'a str,
    pub color: &'a str,
    pub id: &'a str,
    pub project_id: Option<&'a str>,
    pub workspace_id: &'a str,
    pub sort_order: f64,
}

/// App `LabelSerializer.to_representation` output (`issue.py:549-562`),
/// in `Meta.fields` order. `sort_order` goes through `serde` `f64`,
/// which matches DRF for every realistic label sort order (integral
/// magnitudes: `65535.0` renders `"sort_order":65535.0` on both); same
/// caveat as the merged space port.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppLabelView<'a> {
    pub parent: Option<&'a str>,
    pub name: &'a str,
    pub color: &'a str,
    pub id: &'a str,
    pub project_id: Option<&'a str>,
    pub workspace_id: &'a str,
    pub sort_order: f64,
}

/// Port of the app `LabelSerializer` read shape (`issue.py:549-562`).
/// Field-for-field copy.
pub fn app_label_to_representation<'a>(row: &'a AppLabelRow<'a>) -> AppLabelView<'a> {
    AppLabelView {
        parent: row.parent,
        name: row.name,
        color: row.color,
        id: row.id,
        project_id: row.project_id,
        workspace_id: row.workspace_id,
        sort_order: row.sort_order,
    }
}

/// Input names the serializer accepts on write (`issue.py:549-562`):
/// the four `_writable_fields` (probed). Everything else in the input —
/// the three [`APP_LABEL_READONLY_INPUT_FIELDS`], the `workspace` /
/// `project` relation names, and unknown keys — is silently dropped.
pub const APP_LABEL_WRITABLE_FIELDS: [&str; 4] = ["parent", "name", "color", "sort_order"];

/// Input names the serializer silently drops on write: `id` (declared
/// read-only, `base.py:8-9`) plus `project_id` / `workspace_id`
/// (property-fallback `ReadOnlyField`s). Together with
/// [`APP_LABEL_WRITABLE_FIELDS`] these partition [`APP_LABEL_FIELDS`].
pub const APP_LABEL_READONLY_INPUT_FIELDS: [&str; 3] = ["id", "project_id", "workspace_id"];

/// `validate_name` failure detail (`issue.py:572`).
pub const LABEL_NAME_ALREADY_EXISTS: &str = "LABEL_NAME_ALREADY_EXISTS";

/// `validate_name` 400 body (`issue.py:563-574` via
/// `Response(serializer.errors)`): a field-level `ValidationError`
/// renders the detail string in a one-element list under the field
/// (probed byte-for-byte).
pub const LABEL_NAME_CONFLICT_BODY: &str = "{\"name\":[\"LABEL_NAME_ALREADY_EXISTS\"]}";

/// Django `.exists()` probe for the create arm of `validate_name`
/// (`issue.py:564-572`): `labels` (`db_table`, `label.py:43`) with the
/// `project_id` scope, the Postgres `iexact` spelling, and the
/// `SoftDeletionManager` tombstone guard (`db/mixins.py:56-58`). `$1` is
/// the context `project_id`, `$2` the raw candidate name.
pub const LABEL_NAME_CONFLICT_PROBE_SQL: &str = "SELECT 1 FROM labels WHERE project_id = $1 AND UPPER(name::text) = UPPER($2) AND deleted_at IS NULL LIMIT 1";

/// Django `.exists()` probe for the update arm of `validate_name`
/// (`issue.py:568-572`): the create probe plus the `.exclude(pk)`
/// clause, which Django emits as `NOT (id = …)` (`id` is the PK, never
/// null, so the `NOT` form and `<>` agree). `$3` is the instance pk.
pub const LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL: &str = "SELECT 1 FROM labels WHERE project_id = $1 AND UPPER(name::text) = UPPER($2) AND deleted_at IS NULL AND NOT (id = $3) LIMIT 1";

/// Mirrors `validate_name` (`issue.py:563-574`): `conflict` is the
/// `*_PROBE_SQL` verdict for the candidate name. Returns the detail the
/// handler renders as [`LABEL_NAME_CONFLICT_BODY`] with 400.
pub fn validate_label_name(conflict: bool) -> Result<(), &'static str> {
    if conflict {
        return Err(LABEL_NAME_ALREADY_EXISTS);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|name| name.to_string()).collect()
    }

    const L_BUG: &str = "0e1cb691-7d20-4f47-804c-4c17711db39d";
    const L_SUB: &str = "81e4506d-fe09-4bcd-8746-94ea4e17643d";
    const PROJ: &str = "22e00942-19b1-45b1-b388-438f37942205";
    const WS: &str = "994cc638-306d-4305-ad86-c4e87f829cb4";

    #[test]
    fn app_label_fields_match_wire_order() {
        let row = AppLabelRow {
            parent: None,
            name: "Bug",
            color: "#ff0000",
            id: L_BUG,
            project_id: Some(PROJ),
            workspace_id: WS,
            sort_order: 65535.0,
        };
        assert_eq!(
            serialized_keys(&app_label_to_representation(&row)),
            const_keys(&APP_LABEL_FIELDS)
        );
    }

    #[test]
    fn app_label_replays_probe_bytes() {
        // TRACE: issue.py:549-562 read shape; bytes pinned by the
        // pinned-DRF probe (null parent, set project, integral float).
        let row = AppLabelRow {
            parent: None,
            name: "Bug",
            color: "#ff0000",
            id: L_BUG,
            project_id: Some(PROJ),
            workspace_id: WS,
            sort_order: 65535.0,
        };
        assert_eq!(
            serde_json::to_string(&app_label_to_representation(&row)).expect("serializes"),
            "{\"parent\":null,\"name\":\"Bug\",\"color\":\"#ff0000\",\
             \"id\":\"0e1cb691-7d20-4f47-804c-4c17711db39d\",\
             \"project_id\":\"22e00942-19b1-45b1-b388-438f37942205\",\
             \"workspace_id\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"sort_order\":65535.0}",
        );
    }

    #[test]
    fn app_label_renders_parent_and_null_project() {
        // TRACE: issue.py:549-562; label.py:12-18 (nullable self-FK),
        // workspace.py:187 (nullable project). Empty color renders "".
        let row = AppLabelRow {
            parent: Some(L_BUG),
            name: "sub",
            color: "",
            id: L_SUB,
            project_id: None,
            workspace_id: WS,
            sort_order: 75535.0,
        };
        assert_eq!(
            serde_json::to_string(&app_label_to_representation(&row)).expect("serializes"),
            "{\"parent\":\"0e1cb691-7d20-4f47-804c-4c17711db39d\",\
             \"name\":\"sub\",\"color\":\"\",\
             \"id\":\"81e4506d-fe09-4bcd-8746-94ea4e17643d\",\
             \"project_id\":null,\
             \"workspace_id\":\"994cc638-306d-4305-ad86-c4e87f829cb4\",\
             \"sort_order\":75535.0}",
        );
    }

    #[test]
    fn write_guards_partition_the_seven_keys() {
        // TRACE: issue.py:549-562 field mapping; base.py:8-9 (read-only
        // id); DRF property fallback for the attnames; read_only_fields
        // (:561) dead per get_fields.
        let mut guarded: Vec<&str> = Vec::new();
        guarded.extend(APP_LABEL_WRITABLE_FIELDS);
        guarded.extend(APP_LABEL_READONLY_INPUT_FIELDS);
        guarded.sort_unstable();
        let mut all = APP_LABEL_FIELDS.to_vec();
        all.sort_unstable();
        assert_eq!(guarded, all);
        assert_eq!(
            APP_LABEL_WRITABLE_FIELDS.len() + APP_LABEL_READONLY_INPUT_FIELDS.len(),
            7
        );
    }

    #[test]
    fn validate_label_name_maps_conflict_to_detail() {
        // TRACE: issue.py:563-574.
        assert_eq!(validate_label_name(true), Err(LABEL_NAME_ALREADY_EXISTS));
        assert_eq!(validate_label_name(false), Ok(()));
    }

    #[test]
    fn conflict_body_is_exact_drf_bytes() {
        // TRACE: issue.py:572 via Response(serializer.errors),
        // label.py:48/80; probed byte-for-byte on pinned DRF.
        assert_eq!(
            LABEL_NAME_CONFLICT_BODY,
            "{\"name\":[\"LABEL_NAME_ALREADY_EXISTS\"]}"
        );
    }

    #[test]
    fn conflict_probe_sql_pins_clauses() {
        // TRACE: issue.py:566 (.filter over Label.objects =
        // SoftDeletionManager, db/mixins.py:56-58; Postgres iexact =
        // UPPER(name::text) = UPPER($n)); :568-569 (.exclude(pk) =
        // NOT (id = …)).
        for sql in [
            LABEL_NAME_CONFLICT_PROBE_SQL,
            LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL,
        ] {
            assert!(sql.starts_with("SELECT 1 FROM labels WHERE "), "{sql}");
            assert!(sql.contains("project_id = $1"), "{sql}");
            assert!(sql.contains("UPPER(name::text) = UPPER($2)"), "{sql}");
            assert!(sql.contains("deleted_at IS NULL"), "{sql}");
            assert!(sql.ends_with("LIMIT 1"), "{sql}");
        }
        assert!(
            !LABEL_NAME_CONFLICT_PROBE_SQL.contains("id = $3"),
            "create arm has no exclude"
        );
        assert!(
            LABEL_NAME_CONFLICT_EXCLUDING_SELF_PROBE_SQL.contains("NOT (id = $3)"),
            "update arm excludes self"
        );
    }
}
