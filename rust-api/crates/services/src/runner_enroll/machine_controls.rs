//! Owner machine controls (#2903, `machine_identity.py` minus `label`).
//!
//! The web PATCH service: locked read, owner/revoked gates, save only
//! changed fields + `updated_at`, then the pause side effects. The
//! machine-identity issue (NEWPIDASH-52) extends this with `label`.

use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use pidash_db::entity::dev_machine;

use super::dispatch_rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MachineControls {
    pub engine_paused_by_owner: Option<bool>,
    pub engine_team_runs: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedMachine {
    pub model: dev_machine::Model,
    pub was_paused: bool,
    pub effects: dispatch_rules::PauseEffects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineControlFailure {
    NotFound,
    Forbidden,
    Revoked,
}

/// Service error: a gate failure or a DB failure (the handler maps the
/// former to its status, the latter to the JSON 500).
#[derive(Debug)]
pub enum UpdateError {
    Failure(MachineControlFailure),
    Db(DbErr),
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpdateError::Failure(_) => None,
            UpdateError::Db(error) => Some(error),
        }
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::Failure(failure) => write!(f, "{failure:?}"),
            UpdateError::Db(error) => write!(f, "{error}"),
        }
    }
}

/// `update_machine_controls` (#2903, `runners.py` patch body): the caller
/// holds the `BEGIN`; this locks the row (`lock_exclusive`), gates, saves
/// changed fields, and runs the pause effects in-transaction.
pub async fn update_machine_controls(
    db: &impl ConnectionTrait,
    machine_id: Uuid,
    user_id: Uuid,
    values: &MachineControls,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<UpdatedMachine, UpdateError> {
    let Some(machine) = dev_machine::Entity::find_by_id(machine_id)
        .lock_exclusive()
        .one(db)
        .await
        .map_err(UpdateError::Db)?
    else {
        return Err(UpdateError::Failure(MachineControlFailure::NotFound));
    };
    if machine.owner_id != user_id {
        return Err(UpdateError::Failure(MachineControlFailure::Forbidden));
    }
    if machine.revoked_at.is_some() {
        return Err(UpdateError::Failure(MachineControlFailure::Revoked));
    }
    let was_paused = dispatch_rules::engine_paused(
        machine.engine_paused_by_owner,
        &machine.engine_auto_pause_reason,
    );
    let mut changed_paused = machine.engine_paused_by_owner;
    let mut changed_team = machine.engine_team_runs;
    let mut changed = false;
    if let Some(value) = values.engine_paused_by_owner {
        if value != machine.engine_paused_by_owner {
            changed_paused = value;
            changed = true;
        }
    }
    if let Some(value) = values.engine_team_runs {
        if value != machine.engine_team_runs {
            changed_team = value;
            changed = true;
        }
    }
    if changed {
        use sea_orm::sea_query::Expr;
        dev_machine::Entity::update_many()
            .col_expr(
                dev_machine::Column::EnginePausedByOwner,
                Expr::value(changed_paused),
            )
            .col_expr(
                dev_machine::Column::EngineTeamRuns,
                Expr::value(changed_team),
            )
            .col_expr(dev_machine::Column::UpdatedAt, Expr::value(now))
            .filter(dev_machine::Column::Id.eq(machine_id))
            .exec(db)
            .await
            .map_err(UpdateError::Db)?;
    }
    let effects = dispatch_rules::on_engine_pause_changed(
        db,
        machine_id,
        changed_paused,
        &machine.engine_auto_pause_reason,
        was_paused,
    )
    .await
    .map_err(UpdateError::Db)?;
    let mut model = machine;
    model.engine_paused_by_owner = changed_paused;
    model.engine_team_runs = changed_team;
    if changed {
        model.updated_at = now.into();
    }
    Ok(UpdatedMachine {
        model,
        was_paused,
        effects,
    })
}
