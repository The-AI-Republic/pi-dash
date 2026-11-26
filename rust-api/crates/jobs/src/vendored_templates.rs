//! Vendored email templates stay byte-identical to the frozen Django
//! originals (NEWPIDASH-376). The builder no longer ships `apps/api`, so
//! the 12 `apps/api/templates/emails/` files were copied to
//! `rust-api/templates/emails/` and the `include_str!` embeds repointed
//! there. This module pins both halves: each vendored file hashes to the
//! frozen sha256 recorded at copy time, and each embedded const equals its
//! vendored file (the one `live` const that is private to its module is
//! pinned by the wiring test inside `tasks_mail::live` instead).

use sha2::{Digest, Sha256};

/// `(vendored path under rust-api/templates/emails/, frozen sha256)`.
/// Hashes recorded from `apps/api/templates/emails/` at copy time.
const TEMPLATES: [(&str, &str); 12] = [
    (
        "auth/magic_signin.html",
        "72d7d2c31e18dc3c361fb2463a1f904c96d4d3f4f2de28ad89b89e6cc5f9a111",
    ),
    (
        "auth/forgot_password.html",
        "9080951eed70ff5f86e27ea9901fd2f39bf1332dd6a34af7e8e209079ed22c3f",
    ),
    (
        "user/email_updated.html",
        "be7cd35ded4e03d5620774d699ff093e39a0bf744a39d7341fbb60b7944bd4ac",
    ),
    (
        "exports/analytics.html",
        "6b64865d9dd6daf24c571ed94999fa06165bd7c84f291e8f085c36913b918d12",
    ),
    (
        "notifications/issue-updates.html",
        "3f9fd5d9f55e0829576a202cc3c9e3a49c80329f4643ccffefb8ebc075922fd4",
    ),
    (
        "user/user_activation.html",
        "77ca5fb85e55ab8864a921807512f4ff7fb61f40bccb8fe7c9200e8763129500",
    ),
    (
        "user/user_deactivation.html",
        "e621bf8c3b6a2b553219a89c9833bf270112e0d5f5178c70138e76b08a718c9b",
    ),
    (
        "notifications/project_addition.html",
        "421a85d2b82c195c1385e08bbc6ca9e1a5716bb66dbbcbdbd4a6ac132653eb4f",
    ),
    (
        "invitations/workspace_invitation.html",
        "69998f85cb9f6d3c23de01eb0d4d404db97611b75ef37b1c0913917dfad4a210",
    ),
    (
        "invitations/project_invitation.html",
        "9c8d0e4c816a6ed7e4afccd5d7d7c7f286c4a6182c1d911918a10d76b19783b8",
    ),
    (
        "notifications/webhook-deactivate.html",
        "daa6ba5538b9f4d3900038fc25756e68cada60d2a7522881005bedbcec2763e3",
    ),
    (
        "test_email.html",
        "a67a110fe1634b154ac35a61c8849d4b796999249bfb2dbbad5a95acc99144e7",
    ),
];

fn vendored(rel: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../templates/emails/{rel}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("vendored template readable ({rel}): {e}"))
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn vendored_templates_match_frozen_originals() {
    for (rel, golden) in TEMPLATES {
        let bytes = vendored(rel);
        assert_eq!(
            hex_digest(&bytes),
            golden,
            "{rel} differs from frozen original"
        );
    }
}

#[test]
fn embedded_consts_match_vendored_files() {
    use crate::tasks_mail::mail_send::{
        ANALYTICS_EXPORT_TEMPLATE, EMAIL_UPDATED_TEMPLATE, FORGOT_PASSWORD_TEMPLATE,
        MAGIC_SIGNIN_TEMPLATE,
    };
    use crate::tasks_mail::membership_mail::{
        PROJECT_ADDITION_TEMPLATE, PROJECT_INVITATION_TEMPLATE, USER_ACTIVATION_TEMPLATE,
        USER_DEACTIVATION_TEMPLATE, WORKSPACE_INVITATION_TEMPLATE,
    };
    use crate::tasks_webhooks::webhook_send::DEACTIVATION_HTML_SOURCE;
    let cases: [(&str, &str); 10] = [
        ("auth/magic_signin.html", MAGIC_SIGNIN_TEMPLATE),
        ("auth/forgot_password.html", FORGOT_PASSWORD_TEMPLATE),
        ("user/email_updated.html", EMAIL_UPDATED_TEMPLATE),
        ("exports/analytics.html", ANALYTICS_EXPORT_TEMPLATE),
        ("user/user_activation.html", USER_ACTIVATION_TEMPLATE),
        ("user/user_deactivation.html", USER_DEACTIVATION_TEMPLATE),
        (
            "notifications/project_addition.html",
            PROJECT_ADDITION_TEMPLATE,
        ),
        (
            "invitations/workspace_invitation.html",
            WORKSPACE_INVITATION_TEMPLATE,
        ),
        (
            "invitations/project_invitation.html",
            PROJECT_INVITATION_TEMPLATE,
        ),
        (
            "notifications/webhook-deactivate.html",
            DEACTIVATION_HTML_SOURCE,
        ),
        // `notifications/issue-updates.html` embeds as the private
        // `tasks_mail::live::ISSUE_UPDATES_SOURCE`; its wiring test lives
        // in that module. `test_email.html` is an inline const in
        // pidash-services, pinned by `template_matches_vendored_copy`.
    ];
    for (rel, embedded) in cases {
        let bytes = vendored(rel);
        assert_eq!(
            embedded.as_bytes(),
            bytes.as_slice(),
            "{rel} embed points at the wrong file"
        );
    }
}
