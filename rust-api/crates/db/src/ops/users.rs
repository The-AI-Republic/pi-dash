//! Users + membership ops SQL (D-37).
//!
//! JSON model-default documents shared by the ops commands and the
//! dummy-data task: `ProjectMember` / `ProjectUserProperty` field
//! defaults. Each builder returns a fresh document per call, like
//! the model callables. (PIDASHCONV-807 extends this module with the
//! ops SQL layer on rebase; the builders below are the shared base.)

use serde_json::Value;

// ---------------------------------------------------------------------------
// JSON model defaults (fresh document per call, like the callables)
// ---------------------------------------------------------------------------

/// `view_props`/`default_props` for `ProjectMember`
/// (`db/models/project.py:43-65`, `get_default_props`): filters +
/// display filters only — no `display_properties` key.
pub fn project_member_props_json() -> Value {
    serde_json::json!({
        "filters": {
            "priority": null, "state": null, "state_group": null,
            "assignees": null, "created_by": null, "labels": null,
            "start_date": null, "target_date": null, "subscriber": null
        },
        "display_filters": {
            "group_by": null, "order_by": "-created_at", "type": null,
            "sub_issue": true, "show_empty_groups": true,
            "layout": "list", "calendar_date_range": ""
        }
    })
}

/// `preferences` for `ProjectMember` / `ProjectUserProperty`
/// (`project.py:get_default_preferences`).
pub fn project_preferences_json() -> Value {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}
    })
}

/// `filters` for `ProjectUserProperty`
/// (`db/models/issue.py:50-61`, `get_default_filters`).
pub fn property_filters_json() -> Value {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null
    })
}

/// `display_filters` for `ProjectUserProperty` (`issue.py:64-73`).
pub fn property_display_filters_json() -> Value {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true,
        "layout": "list", "calendar_date_range": ""
    })
}

/// `display_properties` for `ProjectUserProperty` (`issue.py:76-91`):
/// `sub_issue_count`, exactly as the model default spells it.
pub fn property_display_properties_json() -> Value {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_json_defaults_match_model_callables() {
        let props = project_member_props_json();
        assert!(props.get("filters").is_some());
        assert!(props.get("display_filters").is_some());
        assert!(props.get("display_properties").is_none());
        assert_eq!(
            props["display_filters"]["order_by"],
            Value::String("-created_at".to_owned())
        );
        assert_eq!(props["filters"]["subscriber"], Value::Null);
        let prefs = project_preferences_json();
        assert_eq!(prefs["pages"]["block_display"], Value::Bool(true));
        assert_eq!(
            prefs["navigation"]["default_tab"],
            Value::String("work_items".to_owned())
        );
    }

    #[test]
    fn property_json_defaults_match_issue_callables() {
        assert_eq!(property_filters_json()["subscriber"], Value::Null);
        assert_eq!(
            property_display_filters_json()["sub_issue"],
            Value::Bool(true)
        );
        let display = property_display_properties_json();
        assert_eq!(display["sub_issue_count"], Value::Bool(true));
        assert!(display.get("sub_issue").is_none());
    }

    #[test]
    fn json_defaults_are_fresh_per_call() {
        let mut first = project_member_props_json();
        first["filters"]["priority"] = Value::String("mutated".to_owned());
        assert_eq!(
            project_member_props_json()["filters"]["priority"],
            Value::Null
        );
    }
}
