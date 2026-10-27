//! Renderers: typed rows in, API shapes out.
//!
//! Verbatim moves of the `pidash-api` handler helpers (OVL-05,
//! PIDASHCONV-1543); the handlers call these.

use serde_json::Value;
use uuid::Uuid;

use crate::app_project::ser_workflow as ser;

/// One state row for the read shape (the 9 `STATE_READ_KEYS` columns).
/// Moved from `api::app_project::handlers_workflow`.
#[derive(Debug, Clone)]
pub struct StateRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub color: String,
    pub group: String,
    pub default: bool,
    pub description: String,
    pub sequence: f64,
}

/// Render one state through the L3 read shape.
pub fn render_state(row: &StateRow) -> Value {
    let id = row.id.to_string();
    let project_id = row.project_id.to_string();
    let workspace_id = row.workspace_id.to_string();
    let row_view = ser::StateRow {
        id: &id,
        project_id: &project_id,
        workspace_id: &workspace_id,
        name: &row.name,
        color: &row.color,
        group: &row.group,
        default: row.default,
        description: &row.description,
        sequence: row.sequence,
    };
    let view = ser::state_to_representation(&row_view);
    serde_json::to_value(&view).expect("state view serializes")
}

/// The 9-column state tuple, in [`StateRow`] field order.
pub type StateRowTuple = (Uuid, Uuid, Uuid, String, String, String, bool, String, f64);

/// Build a [`StateRow`] from its tuple form.
pub fn state_row_from(tuple: StateRowTuple) -> StateRow {
    StateRow {
        id: tuple.0,
        project_id: tuple.1,
        workspace_id: tuple.2,
        name: tuple.3,
        color: tuple.4,
        group: tuple.5,
        default: tuple.6,
        description: tuple.7,
        sequence: tuple.8,
    }
}
