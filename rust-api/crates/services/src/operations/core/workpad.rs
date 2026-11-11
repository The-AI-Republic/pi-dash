//! `core::workpad::set` (NEWPIDASH-190 §4.3): the shared workpad
//! write over a caller-supplied [`WriteTxn`](crate::domain::WriteTxn).
//!
//! - [`WorkpadMode::Sparse`] — the v1 (`workpad::update`) path: the
//!   `set_workpad` domain save (stamps `updated_by`, hook
//!   recomputes, completion chain).
//! - [`WorkpadMode::Bare`] — the agent (`op_update_current_issue_workpad`)
//!   path: the `update_workpad` domain write (bare `workpad` +
//!   `updated_at`, no hooks, no chain).
//!
//! The mode exists only because the two wrappers genuinely differ on the
//! wire; it is on the phase 5 convergence list.
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::issues as domain_issues;
use crate::domain::{WriteCtx, WriteTxn};

use super::CoreError;

/// Which wrapper's shape the write takes (NEWPIDASH-190 §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpadMode {
    /// Operations (v1 workpad PATCH): stamps `updated_by`, runs hook
    /// recomputes and the completion chain.
    Sparse,
    /// Agent tool: bare two columns, no hooks, no chain.
    Bare,
}

/// A workpad write on a real issue.
#[derive(Debug, Clone)]
pub struct WorkpadSet {
    /// The issue whose workpad to set.
    pub issue_id: Uuid,
    /// The owning project (the write is scoped; the rows match both).
    pub project_id: Uuid,
    /// The workspace slug for the locked sparse read (unused on Bare).
    pub slug: String,
    /// The validated workpad text.
    pub body: String,
    /// Which wrapper's shape to run.
    pub mode: WorkpadMode,
}

/// A landed workpad write: the stamp it wrote and whether the
/// completion chain stopped (Sparse only — the operation answers
/// `ServerError` when set, as it did off the domain outcome).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkpadSaved {
    /// The `updated_at` the write stamped.
    pub updated_at: DateTime<Utc>,
    /// Whether the completion chain stopped (`ChainOutcome::stopped`
    /// was `Some`); always `false` on Bare.
    pub stopped: bool,
}

/// `core::workpad::set(txn, ctx, pool, WorkpadSet { issue_id,
/// project_id, body, mode }) -> WorkpadSaved { updated_at }` (§4.3).
/// `updated_at` stamps now at the call site (the operations response
/// renders the stamp, so the core owns the clock).
pub async fn set(
    txn: &mut WriteTxn,
    ctx: &WriteCtx,
    pool: &PgPool,
    input: WorkpadSet,
) -> Result<WorkpadSaved, CoreError> {
    match input.mode {
        WorkpadMode::Sparse => {
            let updated_at = now_utc();
            let outcome = domain_issues::set_workpad(
                ctx,
                txn,
                pool,
                domain_issues::SetWorkpad {
                    issue_id: input.issue_id,
                    project_id: input.project_id,
                    slug: input.slug,
                    workpad: input.body,
                    updated_at,
                },
            )
            .await?;
            Ok(WorkpadSaved {
                updated_at,
                stopped: outcome.stopped().is_some(),
            })
        }
        WorkpadMode::Bare => {
            let updated_at = now_utc();
            domain_issues::update_workpad(
                &txn.connection(),
                input.issue_id,
                input.project_id,
                &input.body,
                updated_at,
            )
            .await?;
            Ok(WorkpadSaved {
                updated_at,
                stopped: false,
            })
        }
    }
}

fn now_utc() -> DateTime<Utc> {
    crate::operations::common::now_utc()
}

/// Map a core failure back onto the domain error type (for wrappers
/// that drive the core inside `domain::run`, which reports
/// `DomainError`): `Db` passes through; the bare update's native sqlx
/// error rides the SeaORM exec wrapper.
pub fn core_to_domain_error(error: super::CoreError) -> crate::domain::DomainError {
    match error {
        super::CoreError::Db(inner) => inner,
        super::CoreError::Sqlx(inner) => crate::domain::DomainError::Db(sea_orm::DbErr::Exec(
            sea_orm::RuntimeErr::SqlxError(inner),
        )),
    }
}
