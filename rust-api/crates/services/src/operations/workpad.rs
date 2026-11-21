//! Workpad operation: partial update under the row lock.
//!
//! The serializer step of the v1 workpad endpoint, moved from
//! `pidash_api::v1_work_items::handlers_relations` (OVL-14,
//! PIDASHCONV-1649): `IssueWorkpadSerializer(partial=True)` under the
//! row lock (`views/issue.py:3280-3320`).

use std::sync::Arc;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::domain::activity::TaskEnqueueHook;
use crate::domain::issues as domain_issues;
use crate::domain::transition::IssueTransitionHook;
use crate::domain::{self, WriteCtx};
use crate::v1_work_items::shape_labels::{validate_workpad_write, WorkpadWriteInput};

use super::common::{activate_timezone, db_error, now_utc};
use super::deps::Deps;
use super::error::{Error, Outcome};

/// Workpad PATCH without the wire field (`views/issue.py:3287-3291`).
pub const WORKPAD_MISSING_BODY: &str =
    r#"{"error":"PATCH requires a `body` field in the request payload."}"#;

/// The workpad size cap, in characters (`.ai_design/unified_workpad/
/// design.md` U7, §8.5). The workpad is inlined into every issue-run
/// prompt, so a runaway note costs every later run. Existing longer
/// workpads are not truncated; the next agent write must fit. Applies to
/// agent writes only ([`WorkpadWrite::from_run`]).
pub const WORKPAD_MAX_CHARS: usize = 16_000;

/// The [`WORKPAD_MAX_CHARS`] refusal body.
pub const WORKPAD_TOO_LONG: &str = r#"{"error":"workpad is longer than 16000 characters: shorten it (drop Keep in mind bullets that are no longer true and keep the last ten Log lines) and retry"}"#;

/// Refuse a workpad body over [`WORKPAD_MAX_CHARS`] characters.
pub fn check_workpad_size(workpad: &str) -> Result<(), Error> {
    if workpad.chars().count() > WORKPAD_MAX_CHARS {
        return Err(Error::FieldErrors(WORKPAD_TOO_LONG.to_owned()));
    }
    Ok(())
}

/// `"body" in request.data` (`:3287`): key membership on objects,
/// element membership on arrays, substring on strings — and `TypeError`
/// (500) on numbers/bools/null.
pub fn workpad_has_body(value: &Value) -> Result<bool, Error> {
    match value {
        Value::Object(map) => Ok(map.contains_key("body")),
        Value::Array(items) => Ok(items.iter().any(|item| item.as_str() == Some("body"))),
        Value::String(text) => Ok(text.contains("body")),
        // `"body" in 5` / `in True` / `in None` raises `TypeError` →
        // generic 500.
        _ => Err(Error::ServerError),
    }
}

/// `PATCH .../workpad/` (`IssueWorkpadSerializer(partial=True)` under
/// the row lock, `views/issue.py:3280-3320`).
///
/// The caller already ran auth, the gate and body negotiation; `write`
/// carries what it resolved.
pub async fn update(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &WorkpadWrite<'_>,
) -> Result<Outcome, Error> {
    let slug = write.slug;
    let project_id = write.project_id;
    let issue_id = write.issue_id;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let body = write.body;
    // The wire-field gate (`:3287-3291`): `"body" not in request.data`.
    // On a JSON string it is a SUBSTRING test; on numbers/bools/null it
    // raises `TypeError` → 500.
    if !workpad_has_body(body)? {
        return Err(Error::FieldErrors(WORKPAD_MISSING_BODY.to_owned()));
    }
    // The existence probe (`:3307-3311`): the SAME text the domain
    // lock-read runs (one builder), on the pool — a miss 404s BEFORE validation runs,
    // as today. (`FOR UPDATE` in autocommit releases at statement end.)
    let probe = sea_orm::ConnectionTrait::query_one(
        &pidash_db::pool::sea_orm_connection(&deps.pool),
        sea_orm::DbBackend::Postgres
            .build(&domain_issues::workpad_select(issue_id, project_id, slug)),
    )
    .await
    .map_err(|error| db_error(pidash_db::orm_tx::into_sqlx(error), "workpad-lock"))?;
    if probe.is_none() {
        return Err(Error::NotFound);
    }
    let validated = validate_workpad_write(&WorkpadWriteInput {
        body,
        partial: true,
    })
    .map_err(|error| Error::FieldErrors(error.body().to_owned()))?;
    let Some(workpad) = validated.workpad else {
        // Unreachable: the wire-field gate above guarantees a `body` key,
        // and validation only omits it when absent.
        return Err(Error::ServerError);
    };
    if write.from_run {
        check_workpad_size(&workpad)?;
    }
    // The locked read + sparse `save()` (workpad, audit, hook recomputes)
    // run in the domain write; the completion pair when the state moved.
    // No orchestration fire — the port has none.
    let updated_at = now_utc();
    let transition: Arc<dyn IssueTransitionHook> = deps.transition.clone();
    let enqueue: Arc<dyn TaskEnqueueHook> = deps.enqueue.clone();
    let if_body_sha256 = write.if_body_sha256.clone();
    let new_sha = crate::orchestration::creation::workpad_sha256(&workpad);
    let outcome = domain::run(ctx, &deps.orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = deps.pool.clone();
        let ctx = ctx.clone();
        let slug = slug.to_owned();
        let if_body_sha256 = if_body_sha256.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::set_workpad(
                &ctx,
                txn,
                &pool,
                domain_issues::SetWorkpad {
                    issue_id,
                    project_id,
                    slug,
                    workpad,
                    updated_at,
                    if_body_sha256,
                },
            )
            .await
        })
    })
    .await
    .map_err(|error| {
        if let Some(current) = domain_issues::stale_workpad_sha(&error) {
            // 412 with the current hash echo (NEWPIDASH-320 §6.1): the
            // caller re-reads, re-applies its edit, and retries.
            let mut out = Map::with_capacity(2);
            out.insert(
                "error".to_owned(),
                Value::String("workpad changed since you read it".to_owned()),
            );
            out.insert("body_sha256".to_owned(), Value::String(current.to_owned()));
            return Error::PreconditionFailed(
                serde_json::to_string(&out).expect("412 body serializes"),
            );
        }
        if error.is_not_found() {
            Error::NotFound
        } else {
            Error::ServerError
        }
    })?;
    if outcome.stopped().is_some() {
        return Err(Error::ServerError);
    }
    // The timestamp plus the new body's hash — the body itself just came
    // from the caller (`:3315-3320`), so it is not repeated.
    let mut out = Map::with_capacity(2);
    out.insert(
        "updated_at".to_owned(),
        Value::String(pidash_compat::datetime::render_datetime_in(
            &updated_at,
            &tz,
        )),
    );
    out.insert("body_sha256".to_owned(), Value::String(new_sha));
    Ok(Outcome::ok(
        serde_json::to_string(&out).map_err(|_| Error::ServerError)?,
    ))
}

/// The pre-resolved workpad-update scope: auth, the gate and body
/// negotiation already ran; `body` is the negotiated payload. Forms
/// collapse repeated keys last-wins at parse (the scalar agent spec),
/// so no `from_form` flag travels: nothing downstream branches on it.
pub struct WorkpadWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The workpad's issue id.
    pub issue_id: Uuid,
    /// The negotiated payload.
    pub body: &'a Value,
    /// Stale-write guard (NEWPIDASH-320 §6.1): the caller-read body hash
    /// the locked row must still match. `None` (old CLI) writes
    /// unconditionally, as today.
    pub if_body_sha256: Option<String>,
    /// The write came from an agent run (the request carries the run
    /// header). Only these are held to [`WORKPAD_MAX_CHARS`]: the cap keeps
    /// agent notes short, and a person's edit is never refused for length
    /// (unified workpad §8.5).
    pub from_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workpad_size_cap_counts_characters() {
        assert_eq!(check_workpad_size(&"x".repeat(WORKPAD_MAX_CHARS)), Ok(()));
        assert_eq!(
            check_workpad_size(&"x".repeat(WORKPAD_MAX_CHARS + 1)),
            Err(Error::FieldErrors(WORKPAD_TOO_LONG.to_owned()))
        );
        // Characters, not bytes: a multi-byte note at the cap still fits.
        assert_eq!(check_workpad_size(&"é".repeat(WORKPAD_MAX_CHARS)), Ok(()));
    }

    #[test]
    fn workpad_body_membership() {
        assert!(workpad_has_body(&serde_json::json!({"body": "x"})).expect("bool"));
        assert!(!workpad_has_body(&serde_json::json!({"workpad": "x"})).expect("bool"));
        // `"body" in [...]` is ELEMENT membership, not always-false.
        assert!(workpad_has_body(&serde_json::json!(["body"])).expect("bool"));
        assert!(!workpad_has_body(&serde_json::json!(["other"])).expect("bool"));
        assert!(!workpad_has_body(&serde_json::json!([5])).expect("bool"));
        assert!(workpad_has_body(&Value::String("has body inside".to_owned())).expect("bool"));
        assert!(!workpad_has_body(&Value::String("nope".to_owned())).expect("bool"));
        // `TypeError` wart → 500.
        for body in [Value::Number(5.into()), Value::Bool(true), Value::Null] {
            assert_eq!(workpad_has_body(&body), Err(Error::ServerError));
        }
    }
}
