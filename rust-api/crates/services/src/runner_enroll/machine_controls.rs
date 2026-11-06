#![forbid(unsafe_code)]

//! Owner machine controls (`machine_identity.py` `parse/update_machine_controls`).
//!
//! The web PATCH allows `engine_paused_by_owner` / `engine_team_runs` /
//! `label`; the daemon PATCH allows `engine_paused_by_owner` / `label`.
//! Parse validates unknown keys, label string/collapse/128, bool types,
//! with the `{"error": <message>}` bodies. Update locks the row, gates
//! owner/revoked, saves changed fields + `updated_at`, then runs the
//! pause side effects. An empty `label` resets to `host_label[:128]`.

use sea_orm::{ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use pidash_db::entity::dev_machine;

use super::dispatch_rules;

/// `MACHINE_LABEL_MAX` (`machine_identity.py:56`).
pub const MACHINE_LABEL_MAX: usize = 128;

/// Web PATCH allowed fields (`runners.py` `DevMachineDeleteEndpoint.FIELDS`).
pub const WEB_FIELDS: [&str; 3] = ["engine_paused_by_owner", "engine_team_runs", "label"];
/// Daemon PATCH allowed fields (`machines.py` `DaemonDevMachineEndpoint.FIELDS`).
pub const DAEMON_FIELDS: [&str; 2] = ["engine_paused_by_owner", "label"];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MachineControls {
    pub engine_paused_by_owner: Option<bool>,
    pub engine_team_runs: Option<bool>,
    pub label: Option<String>,
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

/// `parse_machine_controls` (`machine_identity.py`): unknown keys (other
/// than `workspace`) → 400 with the allowed list; `label` must be a
/// string, whitespace collapsed, ≤128 chars; bool fields must be bool.
/// Returns `Err(code)` where `code` is the wire error string.
pub fn parse_machine_controls(
    data: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<MachineControls, String> {
    let mut unknown: Vec<&String> = data
        .keys()
        .filter(|k| !allowed.contains(&k.as_str()) && k.as_str() != "workspace")
        .collect();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(format!(
            "only these fields can be changed here: {}",
            allowed.join(", ")
        ));
    }
    let mut values = MachineControls::default();
    for field in allowed {
        let Some(value) = data.get(*field) else {
            continue;
        };
        if *field == "label" {
            let Some(text) = value.as_str() else {
                return Err("label must be a string".to_string());
            };
            let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed.len() > MACHINE_LABEL_MAX {
                return Err(format!(
                    "label must be at most {MACHINE_LABEL_MAX} characters"
                ));
            }
            values.label = Some(collapsed);
        } else {
            let Some(flag) = value.as_bool() else {
                return Err(format!("{field} must be a boolean"));
            };
            if *field == "engine_paused_by_owner" {
                values.engine_paused_by_owner = Some(flag);
            } else if *field == "engine_team_runs" {
                values.engine_team_runs = Some(flag);
            }
        }
    }
    Ok(values)
}

/// `update_machine_controls`: the caller holds the `BEGIN`; this locks
/// the row (`lock_exclusive`), gates, saves changed fields, and runs the
/// pause effects in-transaction.
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
    let mut label = machine.label.clone();
    if let Some(wanted) = values.label.clone() {
        if wanted.is_empty() {
            label = machine.host_label.chars().take(MACHINE_LABEL_MAX).collect();
        } else {
            label = wanted;
        }
    }
    let mut changed_paused = machine.engine_paused_by_owner;
    let mut changed_team = machine.engine_team_runs;
    if let Some(value) = values.engine_paused_by_owner {
        changed_paused = value;
    }
    if let Some(value) = values.engine_team_runs {
        changed_team = value;
    }
    let was_paused = dispatch_rules::engine_paused(
        machine.engine_paused_by_owner,
        &machine.engine_auto_pause_reason,
    );
    let changed = label != machine.label
        || changed_paused != machine.engine_paused_by_owner
        || changed_team != machine.engine_team_runs;
    if changed {
        use sea_orm::sea_query::Expr;
        dev_machine::Entity::update_many()
            .col_expr(dev_machine::Column::Label, Expr::value(label.clone()))
            .col_expr(
                dev_machine::Column::EnginePausedByOwner,
                Expr::value(changed_paused),
            )
            .col_expr(dev_machine::Column::EngineTeamRuns, Expr::value(changed_team))
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
    model.label = label;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn map(v: Value) -> serde_json::Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn parse_web_label_collapses() {
        let values =
            parse_machine_controls(&map(json!({"label": "  Studio   Mac "})), &WEB_FIELDS).unwrap();
        assert_eq!(values.label.as_deref(), Some("Studio Mac"));
    }

    #[test]
    fn parse_unknown_key_message() {
        let err =
            parse_machine_controls(&map(json!({"host_label": "x"})), &WEB_FIELDS).unwrap_err();
        assert_eq!(
            err,
            "only these fields can be changed here: engine_paused_by_owner, engine_team_runs, label"
        );
    }

    #[test]
    fn parse_daemon_rejects_team_runs() {
        let err =
            parse_machine_controls(&map(json!({"engine_team_runs": true})), &DAEMON_FIELDS).unwrap_err();
        assert_eq!(
            err,
            "only these fields can be changed here: engine_paused_by_owner, label"
        );
    }

    #[test]
    fn parse_label_must_be_string() {
        let err = parse_machine_controls(&map(json!({"label": 5})), &WEB_FIELDS).unwrap_err();
        assert_eq!(err, "label must be a string");
    }

    #[test]
    fn parse_label_too_long() {
        let long = "a".repeat(129);
        let err = parse_machine_controls(&map(json!({"label": long})), &WEB_FIELDS).unwrap_err();
        assert_eq!(err, "label must be at most 128 characters");
    }

    #[test]
    fn parse_bool_must_be_bool() {
        let err = parse_machine_controls(
            &map(json!({"engine_paused_by_owner": "yes"})),
            &WEB_FIELDS,
        )
        .unwrap_err();
        assert_eq!(err, "engine_paused_by_owner must be a boolean");
    }

    #[test]
    fn parse_ignores_workspace() {
        let values = parse_machine_controls(
            &map(json!({"workspace": "ws", "label": "x"})),
            &WEB_FIELDS,
        )
        .unwrap();
        assert_eq!(values.label.as_deref(), Some("x"));
    }
}
