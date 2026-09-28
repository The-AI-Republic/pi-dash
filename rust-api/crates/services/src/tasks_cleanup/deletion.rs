//! Soft/hard deletion task bodies.
//!
//! Port of `apps/api/pi_dash/bgtasks/deletion_task.py` (all 193 lines,
//! PIDASHCONV-186): `soft_delete_related_objects` (`:18`), the
//! never-registered `restore_related_objects` (`:109`), and `hard_delete`
//! (`:114`).
//!
//! Walk semantics (fixture `soft_delete_walk`):
//!
//! - Entry reads through `all_objects` (unscoped: already-soft-deleted
//!   rows are found); a missing row returns silently (`DoesNotExist` at
//!   `:25-28`); an unknown `(app_label, model_name)` fails loudly (the
//!   `apps.get_model` `LookupError`, which the `except` does not catch).
//! - Relations first, self last. `DO_NOTHING` skips. `SET_NULL` nulls
//!   (one-to-one: single-row `save(update_fields)` with the `auto_now`
//!   bump; to-many: scoped queryset `update`, no `updated_at` touch).
//!   Everything else takes the CASCADE branch: one-to-one stamps with a
//!   full save and recurses depth-first; to-many hits ported bug BUG-DEL-1
//!   (the manager is called as a function, `TypeError`, caught at `:97`,
//!   warning reported, relation skipped — to-many CASCADE children are
//!   never soft-deleted).
//! - `SET_NULL` failures propagate (that branch sits outside the `try`);
//!   CASCADE-branch failures are warnings and the walk continues.
//! - Recursion is an explicit worklist with a visited set instead of
//!   Python's unbounded call stack. The terminal DB state is identical:
//!   Python re-visits are no-ops (stamps guarded by `not deleted_at`,
//!   nulling idempotent), so visiting each node once only drops redundant
//!   re-walks — and guarantees termination on relation cycles, where
//!   Python would spin until `RecursionError`.
//!
//! Write scoping follows the Porting guide: every function takes an
//! explicit [`RequestContext`][pidash_db::RequestContext]. Discovery reads
//! honor `use_read_replica`; every write runs on the primary pool. These
//! maintenance writes set no audit FK columns, mirroring the Python task
//! (which never touches `created_by`/`updated_by` on these paths).

use std::collections::HashSet;

use pidash_db::tasks_cleanup::{
    cascade_to_many_message, entry_table_for, fetch_live_one_to_one, fetch_row, load_schema_info,
    now_stamp, null_bulk, null_one_to_one, parse_pk, reverse_relations, run_named_hard_deletes,
    run_sweep_hard_deletes, stamp_row, sweep_tables, LookupError, RowState,
};
use pidash_db::{Pools, RequestContext};
use serde_json::Value;

/// What `soft_delete_related_objects` may fail with.
#[derive(Debug, thiserror::Error)]
pub enum DeletionError {
    /// `(app_label, model_name)` resolves to no model: the
    /// `apps.get_model` `LookupError` equivalent. Loud, like Python.
    #[error("unknown model: {app_label}.{model_name}")]
    UnknownModel {
        app_label: String,
        model_name: String,
    },
    /// The Celery args/kwargs do not carry `(app_label, model_name,
    /// instance_pk)`: the task fails, as a Python `TypeError` on missing
    /// positional arguments would.
    #[error("bad task payload: {0}")]
    BadPayload(String),
    /// The `instance_pk` is not a UUID (Django `ValidationError`).
    #[error("bad primary key {0:?}: not a UUID")]
    BadPk(String),
    /// Any database failure.
    #[error("database error: {0}")]
    Db(String),
}

impl From<LookupError> for DeletionError {
    fn from(error: LookupError) -> Self {
        match error {
            LookupError::BadPk(raw) => DeletionError::BadPk(raw),
            LookupError::Db(error) => DeletionError::Db(error.to_string()),
        }
    }
}

fn db_error(error: impl std::fmt::Display) -> DeletionError {
    DeletionError::Db(error.to_string())
}

/// A parsed `soft_delete_related_objects` invocation:
/// `(app_label, model_name, instance_pk, using=None)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftDeleteTarget {
    pub app_label: String,
    pub model_name: String,
    /// Raw pk text; parsed to `Uuid` at execution (every fixture table
    /// uses a UUID pk).
    pub instance_pk: String,
    /// The Django DB alias. Accepted for wire parity, ignored: the Rust
    /// worker has a single primary pool.
    pub using: Option<String>,
}

/// Read pool honoring the request context: replica reads only when the
/// context opts in (writes always use [`Pools::primary`]).
macro_rules! read_pool {
    ($pools:expr, $ctx:expr) => {
        if $ctx.use_read_replica() {
            $pools.replica().unwrap_or_else(|| $pools.primary())
        } else {
            $pools.primary()
        }
    };
}

fn json_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Parse Celery v2 `(args, kwargs)` into a [`SoftDeleteTarget`].
/// `using` arrives positionally (`delay(a, m, pk, using=u)` puts it in
/// `args[3]`) or as a kwarg (`kwargs["using"]`); both are accepted.
pub fn parse_soft_delete_call(
    args: &Value,
    kwargs: &Value,
) -> Result<SoftDeleteTarget, DeletionError> {
    let positional: &[Value] = args.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let get = |index: usize, key: &str| {
        positional
            .get(index)
            .and_then(json_string)
            .or_else(|| kwargs.get(key).and_then(json_string))
    };
    let app_label = get(0, "app_label")
        .ok_or_else(|| DeletionError::BadPayload("missing app_label".to_owned()))?;
    let model_name = get(1, "model_name")
        .ok_or_else(|| DeletionError::BadPayload("missing model_name".to_owned()))?;
    let instance_pk = get(2, "instance_pk")
        .ok_or_else(|| DeletionError::BadPayload("missing instance_pk".to_owned()))?;
    if instance_pk.is_empty() {
        return Err(DeletionError::BadPayload("empty instance_pk".to_owned()));
    }
    Ok(SoftDeleteTarget {
        app_label,
        model_name,
        instance_pk,
        using: get(3, "using"),
    })
}

/// What one `soft_delete_related_objects` run did. `warnings` carries the
/// per-relation error lines Python `print`s (`Error handling relation
/// ...`); the jobs handler logs them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoftDeleteOutcome {
    /// Distinct `(table, pk)` nodes walked.
    pub visited: usize,
    /// Rows stamped `deleted_at` (relations + self).
    pub stamped: usize,
    /// Rows touched by FK nulling.
    pub nulled_rows: usize,
    /// Skipped-relation lines (the to-many CASCADE bug, CASCADE-branch
    /// row errors). The walk continued past every one.
    pub warnings: Vec<String>,
}

/// `soft_delete_related_objects(app_label, model_name, instance_pk,
/// using=None)` (`deletion_task.py:18-105`).
pub async fn soft_delete_related_objects(
    pools: &Pools,
    ctx: &RequestContext,
    target: SoftDeleteTarget,
) -> Result<SoftDeleteOutcome, DeletionError> {
    let table = entry_table_for(&target.app_label, &target.model_name).ok_or_else(|| {
        DeletionError::UnknownModel {
            app_label: target.app_label.clone(),
            model_name: target.model_name.clone(),
        }
    })?;
    let pk = parse_pk(&target.instance_pk)?;

    let reads = read_pool!(pools, ctx);
    let writes = pools.primary();
    let schema = load_schema_info(reads).await.map_err(db_error)?;

    let root = fetch_row(reads, &schema, table, &pk)
        .await
        .map_err(db_error)?;
    let root = match root {
        None => return Ok(SoftDeleteOutcome::default()),
        Some(row) => row,
    };

    let mut outcome = SoftDeleteOutcome::default();
    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut stack = vec![(table.to_owned(), root)];

    while let Some((table, state)) = stack.pop() {
        if !visited.insert((table.clone(), state.pk.to_string())) {
            continue;
        }
        outcome.visited += 1;

        let relations = reverse_relations(reads, &table).await.map_err(db_error)?;
        for relation in &relations {
            if relation.is_do_nothing() {
                continue;
            }
            if relation.is_set_null() {
                // Outside the Python `try`: failures propagate.
                if relation.one_to_one {
                    if let Some(related) =
                        fetch_live_one_to_one(reads, &schema, relation, &state.pk)
                            .await
                            .map_err(db_error)?
                    {
                        outcome.nulled_rows +=
                            null_one_to_one(writes, &schema, relation, &related.pk, &now_stamp())
                                .await
                                .map_err(db_error)? as usize;
                    }
                } else {
                    outcome.nulled_rows += null_bulk(writes, &schema, relation, &state.pk)
                        .await
                        .map_err(db_error)? as usize;
                }
                continue;
            }
            // CASCADE branch (and RESTRICT / SET DEFAULT / anything else,
            // exactly like the Python `else`): inside the `try`, so a row
            // failure becomes a warning and the walk continues.
            let step: Result<Option<(String, RowState)>, DeletionError> = async {
                if relation.one_to_one {
                    let related = fetch_live_one_to_one(reads, &schema, relation, &state.pk)
                        .await
                        .map_err(db_error)?;
                    let related = match related {
                        None => return Ok(None),
                        Some(related) => related,
                    };
                    if !schema.has_deleted_at(&relation.child_table) || related.deleted_at.is_some()
                    {
                        return Ok(None);
                    }
                    stamp_row(
                        writes,
                        &schema,
                        &relation.child_table,
                        &related.pk,
                        &now_stamp(),
                    )
                    .await
                    .map_err(db_error)?;
                    Ok(Some((relation.child_table.clone(), related)))
                } else {
                    // BUG-DEL-1, ported as-is: the related manager is
                    // not callable, so this branch never touches rows.
                    Ok(None)
                }
            }
            .await;
            match step {
                Ok(None) if !relation.one_to_one => {
                    outcome.warnings.push(cascade_to_many_message(&accessor(
                        &relation.child_table,
                        &relation.fk_column,
                    )));
                }
                Ok(None) => {}
                Ok(Some((child_table, child_state))) => {
                    outcome.stamped += 1;
                    stack.push((child_table, child_state));
                }
                Err(error) => {
                    outcome.warnings.push(format!(
                        "Error handling relation {}: {error}",
                        accessor(&relation.child_table, &relation.fk_column)
                    ));
                }
            }
        }

        // Finally, the instance itself (`:102-105`), outside the `try`:
        // failures propagate.
        if schema.has_deleted_at(&table) && state.deleted_at.is_none() {
            stamp_row(writes, &schema, &table, &state.pk, &now_stamp())
                .await
                .map_err(db_error)?;
            outcome.stamped += 1;
        }
    }

    Ok(outcome)
}

/// Fixture-style accessor label for warning lines
/// (`<child_table>.<fk_column>`; the exact Django accessor name is a
/// `_meta` detail with no catalog equivalent).
fn accessor(child_table: &str, fk_column: &str) -> String {
    format!("{child_table}.{fk_column}")
}

/// What one `hard_delete` run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardDeleteOutcome {
    /// Rows removed by the 18 named deletes.
    pub named_deleted: u64,
    /// Rows removed by the catalog sweep (re-inclusive of the named
    /// tables, like the Python loop).
    pub sweep_deleted: u64,
    /// Tables the sweep visited.
    pub sweep_tables: usize,
}

/// `hard_delete()` (`deletion_task.py:114-193`): the 18 named models in
/// source order, then every `deleted_at` base table. `cutoff` is the
/// preformatted `now - HARD_DELETE_AFTER_DAYS` literal.
pub async fn hard_delete(
    pools: &Pools,
    ctx: &RequestContext,
    cutoff: &str,
) -> Result<HardDeleteOutcome, DeletionError> {
    let writes = pools.primary();
    let named_deleted = run_named_hard_deletes(writes, cutoff)
        .await
        .map_err(db_error)?;
    let schema = load_schema_info(read_pool!(pools, ctx))
        .await
        .map_err(db_error)?;
    let tables = sweep_tables(&schema);
    let sweep_tables_count = tables.len();
    let sweep_deleted = run_sweep_hard_deletes(writes, &schema, cutoff)
        .await
        .map_err(db_error)?;
    Ok(HardDeleteOutcome {
        named_deleted,
        sweep_deleted,
        sweep_tables: sweep_tables_count,
    })
}

/// `restore_related_objects` (`deletion_task.py:108-110`), ported as-is:
///
/// ```python
/// # @shared_task
/// def restore_related_objects(app_label, model_name, instance_pk, using=None):
///     pass
/// ```
///
/// The decorator ships commented out, so this is a plain function that
/// returns `None` unconditionally — it is NOT a registered task.
/// Ported bug BUG-DEL-2: the restore path is dead; the jobs layer must
/// not register any task under its would-be name, and the registry test
/// pins that absence.
pub fn restore_related_objects(
    _app_label: &str,
    _model_name: &str,
    _instance_pk: &str,
    _using: Option<&str>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_parses_positional_and_using_kwarg() {
        let target = parse_soft_delete_call(
            &json!(["db", "issue", "12345678-1234-1234-1234-1234567890ab"]),
            &json!({"using": "other"}),
        )
        .expect("valid");
        assert_eq!(
            target,
            SoftDeleteTarget {
                app_label: "db".to_owned(),
                model_name: "issue".to_owned(),
                instance_pk: "12345678-1234-1234-1234-1234567890ab".to_owned(),
                using: Some("other".to_owned()),
            }
        );
        let positional_using = parse_soft_delete_call(
            &json!([
                "db",
                "issue",
                "12345678-1234-1234-1234-1234567890ab",
                "other"
            ]),
            &json!({}),
        )
        .expect("valid");
        assert_eq!(positional_using.using, Some("other".to_owned()));
        let no_using = parse_soft_delete_call(
            &json!(["db", "issue", "12345678-1234-1234-1234-1234567890ab"]),
            &json!({}),
        )
        .expect("valid");
        assert_eq!(no_using.using, None);
    }

    #[test]
    fn payload_rejects_missing_or_empty_fields() {
        assert!(parse_soft_delete_call(&json!(["db", "issue"]), &json!({})).is_err());
        assert!(parse_soft_delete_call(&json!([]), &json!({})).is_err());
        assert!(parse_soft_delete_call(&json!([1, 2, 3]), &json!({})).is_err());
        assert!(parse_soft_delete_call(&json!(["db", "issue", ""]), &json!({})).is_err());
    }

    #[test]
    fn restore_is_a_plain_no_op() {
        restore_related_objects("db", "issue", "pk", None);
    }

    #[test]
    fn unknown_model_error_names_both_parts() {
        let error = DeletionError::UnknownModel {
            app_label: "db".to_owned(),
            model_name: "nope".to_owned(),
        };
        assert_eq!(error.to_string(), "unknown model: db.nope");
    }
}
