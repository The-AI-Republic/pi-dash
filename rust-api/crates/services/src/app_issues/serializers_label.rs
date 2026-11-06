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
//! (e.g. `ß` vs `SS`). [`label_name_conflict`] builds it through SeaORM
//! (`name` is a `varchar`, so the `::text` cast changes nothing). The
//! `project_id=None` arm (`context.get` + Django `None → IS NULL`) is
//! the `PUT` path's (DRF's default context carries no `project_id`).
//!
//! Out of scope here (655's handler layer): the `Label.save()`
//! `sort_order = max + 10000` override (`db/models/label.py:46-54`),
//! the `partial_update` exact-match pre-check (`label.py:60-70`, which
//! is case-sensitive where the serializer is `iexact` — asymmetry kept),
//! and the generic field errors (required / max-length / bad-UUID).

use pidash_db::entity::labels;
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect, QueryTrait,
};
use serde::Serialize;
use uuid::Uuid;

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

/// The `validate_name` duplicate probe (`issue.py:564-572`):
/// `Label.objects` (the `SoftDeletionManager` tombstone guard,
/// `db/mixins.py:56-58`) filtered to the context `project_id` (`None`
/// renders `IS NULL`, the `PUT` path's empty context) and the Postgres
/// `iexact` spelling (`UPPER(name) = UPPER(candidate)`), with the update
/// arm's `.exclude(pk)` when `exclude` is set, as an `.exists()`.
pub async fn label_name_conflict(
    db: &impl ConnectionTrait,
    project_id: Option<Uuid>,
    name: &str,
    exclude: Option<Uuid>,
) -> Result<bool, DbErr> {
    let hit: Option<Uuid> = label_name_conflict_query(project_id, name, exclude)
        .into_tuple()
        .one(db)
        .await?;
    Ok(hit.is_some())
}

fn label_name_conflict_query(
    project_id: Option<Uuid>,
    name: &str,
    exclude: Option<Uuid>,
) -> sea_orm::Select<labels::Entity> {
    labels::Entity::find()
        .select_only()
        .column(labels::Column::Id)
        .filter(match project_id {
            Some(project_id) => labels::Column::ProjectId.eq(project_id),
            None => labels::Column::ProjectId.is_null(),
        })
        .filter(
            Expr::expr(Func::upper(Expr::col((
                labels::Entity,
                labels::Column::Name,
            ))))
            .eq(Func::upper(Expr::val(name.to_owned()))),
        )
        .filter(labels::Column::DeletedAt.is_null())
        .apply_if(exclude, |query, pk| query.filter(labels::Column::Id.ne(pk)))
}

/// Mirrors `validate_name` (`issue.py:563-574`): `conflict` is the
/// [`label_name_conflict`] verdict for the candidate name. Returns the detail the
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
    fn conflict_probe_pins_clauses() {
        // TRACE: issue.py:566 (.filter over Label.objects =
        // SoftDeletionManager, db/mixins.py:56-58; Postgres iexact =
        // UPPER(name) = UPPER(candidate)); :568-569 (.exclude(pk)).
        let project = Uuid::nil();
        let create = label_name_conflict_query(Some(project), "Bug", None)
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert_eq!(
            create,
            r#"SELECT "labels"."id" FROM "labels" WHERE "labels"."project_id" = '00000000-0000-0000-0000-000000000000' AND UPPER("labels"."name") = UPPER('Bug') AND "labels"."deleted_at" IS NULL"#
        );
        let put = label_name_conflict_query(None, "Bug", Some(project))
            .build(sea_orm::DbBackend::Postgres)
            .to_string();
        assert!(put.contains(r#""labels"."project_id" IS NULL"#), "{put}");
        assert!(
            put.ends_with(r#"AND "labels"."id" <> '00000000-0000-0000-0000-000000000000'"#),
            "{put}"
        );
    }

    /// The probe against a constraint-free temp table on a one-connection
    /// pool: iexact, tombstones, project scope (incl. `IS NULL`), exclude.
    #[tokio::test]
    async fn live_label_name_conflict() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        sqlx::query("CREATE TEMP TABLE labels AS SELECT * FROM public.labels WITH NO DATA")
            .execute(&pool)
            .await
            .expect("temp table");
        let (project, live, orphan) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        sqlx::query(&format!(
            "INSERT INTO labels (id, project_id, name, deleted_at) VALUES \
             ('{live}', '{project}', 'Bug', NULL), \
             (gen_random_uuid(), '{project}', 'Gone', now()), \
             ('{orphan}', NULL, 'Loose', NULL)"
        ))
        .execute(&pool)
        .await
        .expect("seed");
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let probe = |project_id, name: &'static str, exclude| {
            let db = db.clone();
            async move {
                label_name_conflict(&db, project_id, name, exclude)
                    .await
                    .expect("probe")
            }
        };
        assert!(probe(Some(project), "bUG", None).await);
        assert!(!probe(Some(project), "gone", None).await);
        assert!(!probe(Some(project), "bug", Some(live)).await);
        assert!(!probe(Some(Uuid::new_v4()), "bug", None).await);
        assert!(probe(None, "LOOSE", None).await);
        assert!(!probe(None, "loose", Some(orphan)).await);
        assert!(!probe(None, "bug", None).await);
    }
}
