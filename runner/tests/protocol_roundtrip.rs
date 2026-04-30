//! Integration tests that exercise the cloud protocol serde and approval
//! router state machine at their public API.

use pidash::approval::{
    policy::{Decision, Policy},
    router::{ApprovalRecord, ApprovalRouter, ApprovalStatus, DecisionSource},
};
use pidash::cloud::protocol::{
    ApprovalDecision, ApprovalKind, ClientMsg, Envelope, RunnerStatus, ServerMsg, WIRE_VERSION,
};
use pidash::config::schema::ApprovalPolicySection;
use std::path::Path;
use uuid::Uuid;

#[test]
fn envelope_roundtrips_all_client_variants() {
    let variants: Vec<ClientMsg> = vec![
        ClientMsg::Hello {
            runner_id: Uuid::new_v4(),
            version: "0.1.0".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            status: RunnerStatus::Idle,
            in_flight_run: Some(Uuid::new_v4()),
            protocol_version: WIRE_VERSION,
            project_slug: None,
        },
        ClientMsg::Heartbeat {
            ts: chrono::Utc::now(),
            status: RunnerStatus::Busy,
            in_flight_run: None,
        },
        ClientMsg::RunCompleted {
            run_id: Uuid::new_v4(),
            done_payload: serde_json::json!({"conclusion": "success"}),
            ended_at: chrono::Utc::now(),
        },
    ];
    for v in variants {
        let env = Envelope::new(v);
        let s = serde_json::to_string(&env).unwrap();
        let _: Envelope<ClientMsg> = serde_json::from_str(&s).unwrap();
    }
}

#[test]
fn envelope_roundtrips_all_server_variants() {
    let variants: Vec<ServerMsg> = vec![
        ServerMsg::Welcome {
            server_time: chrono::Utc::now(),
            heartbeat_interval_secs: 25,
            protocol_version: WIRE_VERSION,
        },
        ServerMsg::Assign {
            run_id: Uuid::new_v4(),
            work_item_id: None,
            prompt: "x".into(),
            repo_url: None,
            repo_ref: None,
            git_work_branch: None,
            expected_codex_model: None,
            approval_policy_overrides: None,
            deadline: None,
            resume_thread_id: None,
        },
        ServerMsg::Assign {
            run_id: Uuid::new_v4(),
            work_item_id: None,
            prompt: "y".into(),
            repo_url: Some("https://example.invalid/x.git".into()),
            repo_ref: Some("develop".into()),
            git_work_branch: Some("feat/pinned".into()),
            expected_codex_model: None,
            approval_policy_overrides: None,
            deadline: None,
            resume_thread_id: Some("sess_resume".into()),
        },
        ServerMsg::Cancel {
            run_id: Uuid::new_v4(),
            reason: Some("user".into()),
        },
        ServerMsg::Decide {
            run_id: Uuid::new_v4(),
            approval_id: Uuid::new_v4(),
            decision: ApprovalDecision::Accept,
            decided_by: Some("a@b".into()),
        },
        ServerMsg::Revoke {
            reason: "revoked".into(),
        },
    ];
    for v in variants {
        let env = Envelope::new(v);
        let s = serde_json::to_string(&env).unwrap();
        let _: Envelope<ServerMsg> = serde_json::from_str(&s).unwrap();
    }
}

#[tokio::test]
async fn router_resolution_is_idempotent_after_first_writer() {
    let router = ApprovalRouter::new();
    let rec = ApprovalRecord {
        approval_id: "a1".into(),
        run_id: Uuid::new_v4(),
        kind: ApprovalKind::CommandExecution,
        payload: serde_json::json!({"command": "ls"}),
        reason: None,
        requested_at: chrono::Utc::now(),
        expires_at: None,
        status: ApprovalStatus::Pending,
    };
    router.open(rec).await;
    let first = router
        .decide("a1", ApprovalDecision::Accept, DecisionSource::Local)
        .await;
    let second = router
        .decide("a1", ApprovalDecision::Decline, DecisionSource::Cloud)
        .await;
    assert!(first.is_some());
    assert!(second.is_none());
}

#[test]
fn policy_evaluation_is_deterministic_across_samples() {
    let cfg = ApprovalPolicySection::default();
    let policy = Policy::new(&cfg, Path::new("/"));
    let ls = policy.evaluate(
        ApprovalKind::CommandExecution,
        &serde_json::json!({"command": "ls"}),
    );
    let rm = policy.evaluate(
        ApprovalKind::CommandExecution,
        &serde_json::json!({"command": "rm -rf /"}),
    );
    assert_eq!(ls, Decision::AutoAccept);
    assert_eq!(rm, Decision::AutoDecline);
}
