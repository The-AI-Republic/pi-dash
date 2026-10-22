//! Soft-delete entry coverage (PIDASHCONV-1293).
//!
//! Every literal model name the API's `soft_delete_related_objects`
//! enqueue helpers publish must resolve through the db entry map: an
//! unmapped name fails the job as `UnknownModel` and runs no cascade,
//! where Django's `apps.get_model` resolves every installed model.
//! When a handler starts enqueuing a new model, add its name here (and
//! the `_meta` row in `ENTRY_TABLES` if it is missing).

use pidash_db::tasks_cleanup::entry_table_for;

/// `(app_label, model_name)` pairs published by the API's enqueue
/// helpers (fixed-model helpers and variable call sites alike), audited
/// from `crates/api/src`, `services/src/app_issues/pr_links.rs` and
/// `jobs/src/oss_worker/model_runtime/tools_exec.rs`.
const ENQUEUED: &[(&str, &str)] = &[
    ("db", "analyticview"),
    ("db", "apitoken"),
    ("db", "commentreaction"),
    ("db", "cycle"),
    ("db", "cycleissue"),
    ("db", "deployboard"),
    ("db", "draftissue"),
    ("db", "estimate"),
    ("db", "estimatepoint"),
    ("db", "gitcodereviewlink"),
    ("db", "githubpullrequestlink"),
    ("db", "githubrepositorysync"),
    ("db", "gitrepositorybinding"),
    ("db", "intake"),
    ("db", "intakeissue"),
    ("db", "issue"),
    ("db", "issuecomment"),
    ("db", "issuelink"),
    ("db", "issuereaction"),
    ("db", "issuerelation"),
    ("db", "issuesubscriber"),
    ("db", "issueview"),
    ("db", "issuevote"),
    ("db", "label"),
    ("db", "loopjob"),
    ("db", "module"),
    ("db", "moduleissue"),
    ("db", "modulelink"),
    ("db", "notification"),
    ("db", "page"),
    ("db", "project"),
    ("db", "projectmemberinvite"),
    ("db", "scheduler"),
    ("db", "schedulerbinding"),
    ("db", "state"),
    ("db", "sticky"),
    ("db", "webhook"),
    ("db", "workspace"),
    ("db", "workspacememberinvite"),
    ("db", "workspacetheme"),
    ("db", "workspaceuserlink"),
];

#[test]
fn every_enqueued_model_name_resolves() {
    assert_eq!(ENQUEUED.len(), 41);
    for (app_label, model_name) in ENQUEUED {
        assert!(
            entry_table_for(app_label, model_name).is_some(),
            "enqueued model has no entry: {app_label}.{model_name}"
        );
    }
}
