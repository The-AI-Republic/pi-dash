#![forbid(unsafe_code)]

//! D-23 golden route table: the 121 served `/api/v1/` paths (stage 5).
//!
//! Static data sourced from `FX-OPENAPI-06` (byte copy of
//! `rust-api/contract-tests/v1_openapi/routes_golden.json` from PIDASHCONV-81,
//! re-recorded by PIDASHCONV-525). This table is post-hook data: every entry
//! already satisfies [`super::hooks::endpoint_kept`] (v1-only, no `PUT`, no
//! `server` paths); the doc builder still filters through the hook so the
//! rule stays semantic, and the tests below pin this table to the fixture.
//!
//! Provenance + regen: when Django legitimately gains or loses a v1 route,
//! refresh the golden per `test_routes_golden.py` (`REGEN_GOLDEN=1 pytest`
//! `v1_openapi/test_routes_golden.py` + reviewed diff, never blanket-update),
//! re-record `FX-OPENAPI-06`, then regenerate this table from it (paths
//! sorted, methods sorted lowercase, one `(path, &[methods])` row per path)
//! and review this diff too.
//!
//! Cross-domain: none — static data, no calls into endpoint-domain code.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

/// The golden route map as static data: `(path, methods)` pairs, paths
/// sorted, methods sorted lowercase. 121 entries, 189 operations.
pub const ROUTES: &[(&str, &[&str])] = &[
    ("/api/v1/assets/user-assets/", &["post"]),
    ("/api/v1/assets/user-assets/{asset_id}/", &["delete", "patch"]),
    ("/api/v1/auth/device/approve/", &["post"]),
    ("/api/v1/auth/device/start/", &["post"]),
    ("/api/v1/auth/device/token/", &["post"]),
    ("/api/v1/auth/machine-token/", &["post"]),
    ("/api/v1/auth/revoke/", &["post"]),
    ("/api/v1/auth/workspaces/", &["get"]),
    ("/api/v1/runner/chat/sessions/{session_id}/approvals/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/closed/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/events/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/failed/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/complete/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/started/", &["post"]),
    ("/api/v1/runner/chat/sessions/{session_id}/started/", &["post"]),
    ("/api/v1/runner/dev-machines/desktop-enroll/", &["delete", "post"]),
    ("/api/v1/runner/dev-machines/{dev_machine_id}/commands/{request_id}/result/", &["post"]),
    ("/api/v1/runner/dev-machines/{dev_machine_id}/sessions/", &["post"]),
    ("/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/", &["delete"]),
    ("/api/v1/runner/health/", &["get"]),
    ("/api/v1/runner/machine-tokens/", &["post"]),
    ("/api/v1/runner/metrics/", &["get"]),
    ("/api/v1/runner/projects/", &["get"]),
    ("/api/v1/runner/runners/", &["post"]),
    ("/api/v1/runner/runners/enroll/", &["post"]),
    ("/api/v1/runner/runners/{runner_id}/", &["delete"]),
    ("/api/v1/runner/runners/{runner_id}/refresh/", &["post"]),
    ("/api/v1/runner/runners/{runner_id}/sessions/", &["post"]),
    ("/api/v1/runner/runners/{runner_id}/sessions/{sid}/", &["delete"]),
    ("/api/v1/runner/runs/{run_id}/accept/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/approvals/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/awaiting-reauth/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/cancelled/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/complete/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/events/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/fail/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/pause/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/queued/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/resumed/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/started/", &["post"]),
    ("/api/v1/runner/runs/{run_id}/stream/upgrade/", &["post"]),
    ("/api/v1/runners/{runner_id}/", &["delete"]),
    ("/api/v1/users/me/", &["get"]),
    ("/api/v1/workspaces/{slug}/agent-runs/{run_id}/yield/", &["post"]),
    ("/api/v1/workspaces/{slug}/assets/", &["post"]),
    ("/api/v1/workspaces/{slug}/assets/{asset_id}/", &["get", "patch"]),
    ("/api/v1/workspaces/{slug}/invitations/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/invitations/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/issues/search/", &["get"]),
    ("/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/", &["get"]),
    ("/api/v1/workspaces/{slug}/members/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/archive/", &["delete", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/{cycle_id}/unarchive/", &["delete"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/unarchive/", &["delete"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/", &["delete", "get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/labels/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/members/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/modules/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/{issue_id}/", &["delete"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/pages/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/", &["get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/", &["delete", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/project-members/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/states/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/summary/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/{pk}/", &["delete"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/{pk}/", &["delete"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/grouped/", &["get"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/relate/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/unrelate/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/workpad/", &["get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/", &["post"]),
    ("/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/", &["post"]),
    ("/api/v1/workspaces/{slug}/stickies/", &["get", "post"]),
    ("/api/v1/workspaces/{slug}/stickies/{pk}/", &["delete", "get", "patch"]),
    ("/api/v1/workspaces/{slug}/work-items/search/", &["get"]),
    ("/api/v1/workspaces/{slug}/work-items/search/advanced/", &["get"]),
    ("/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/", &["get"]),
];

/// Number of paths in [`ROUTES`]. Pinned to the golden file by the tests.
pub const ROUTE_COUNT: usize = 121;

/// `{path: sorted methods}` in the `test_routes_golden.route_map` shape,
/// for golden comparison.
pub fn route_map() -> std::collections::BTreeMap<&'static str, Vec<&'static str>> {
    ROUTES
        .iter()
        .map(|(path, methods)| (*path, methods.to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-06.routes_golden.json");

    fn golden() -> serde_json::Value {
        serde_json::from_str(FIXTURE).expect("fixture parses")
    }

    #[test]
    fn route_count_is_121() {
        assert_eq!(ROUTES.len(), ROUTE_COUNT);
        assert_eq!(golden().as_object().unwrap().len(), ROUTE_COUNT);
    }

    #[test]
    fn route_map_matches_golden() {
        let golden = golden();
        let golden = golden.as_object().unwrap();
        let got = route_map();
        assert_eq!(got.len(), golden.len());
        for (path, methods) in &got {
            let want: Vec<&str> = golden[*path]
                .as_array()
                .unwrap()
                .iter()
                .map(|method| method.as_str().unwrap())
                .collect();
            assert_eq!(methods, &want, "{path}");
        }
    }

    #[test]
    fn hook_invariants_hold_on_every_entry() {
        for (path, methods) in ROUTES {
            assert!(path.starts_with("/api/v1/"), "{path}");
            assert!(!path.to_ascii_lowercase().contains("server"), "{path}");
            for method in *methods {
                assert!(
                    crate::v1_openapi::hooks::endpoint_kept(path, method),
                    "{path} {method}"
                );
                assert!(
                    matches!(*method, "get" | "post" | "patch" | "delete"),
                    "{path} {method}"
                );
            }
        }
        let ops: usize = ROUTES.iter().map(|(_, methods)| methods.len()).sum();
        assert_eq!(ops, 189);
    }
}
