# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: instance-console (license) write flows (PIDASHCONV-823).

Built from the ``license`` contract suite: test_instance.py (PATCH
name/read-only/overlong), test_configurations.py (PATCH plain, padded,
unknown-key, encrypted-at-rest, disable-email, email-check),
test_admins.py (create/unknown/missing-email, soft-delete, idempotent
delete, sign-up-screen-visited), test_auth_forms.py (token-less form
POSTs hit the CSRF_FAILURE_VIEW: HTTP 200 + csrf_failure template),
test_workspaces.py (create + validation) and test_permissions.py (anon
401 / non-admin 403 shapes).

The world seed already carries the license instance + admin; the seed
extras add the configuration rows, one newcomer user for the admin
invite, and nothing else. Console requests authenticate as
``admin:<user>`` (the forged sessions row presented as the
admin-session-id cookie the /api/instances/* console reads).
"""

from ..session import Request, Session

NEWCOMER_EMAIL = "replay-newcomer@example.com"

_EMAIL_SEED = (
    ("EMAIL_HOST", "smtp.example.com"),
    ("EMAIL_HOST_USER", "mailer"),
    ("EMAIL_HOST_PASSWORD", "s3cret"),
    ("ENABLE_SMTP", "1"),
    ("EMAIL_PORT", "587"),
    ("EMAIL_FROM", "noreply@example.com"),
)


def _seed_extras(conn) -> None:
    """Configuration rows + newcomer user (cloned into the Rust copy)."""
    from _harness import db as db_helpers

    db_helpers.create_user(conn, "newcomer", email=NEWCOMER_EMAIL)
    with conn.cursor() as cur:
        rows = [
            ("CONTRACT_PLAIN", "old", "general", False),
            ("CONTRACT_PADDED", "old", "general", False),
            # Plaintext seed under a true flag, exactly like the
            # contract test: the PATCH overwrites it with real
            # ciphertext; the comparer decrypts only valid Fernet.
            ("CONTRACT_SECRET", "old", "general", True),
        ]
        rows += [(key, value, "email", False) for key, value in _EMAIL_SEED]
        for key, value, category, encrypted in rows:
            cur.execute(
                "INSERT INTO instance_configurations "
                "(created_at, updated_at, id, key, value, category, is_encrypted) "
                "VALUES (now(), now(), gen_random_uuid(), %s, %s, %s, %s)",
                (key, value, category, encrypted),
            )
    conn.commit()


SESSIONS = (
    Session(
        name="license_lifecycle",
        domain="license",
        source="license/test_instance.py, test_configurations.py, "
        "test_admins.py, test_auth_forms.py, test_workspaces.py, "
        "test_permissions.py",
        auth_users=("admin:admin", "admin:member"),
        seed=_seed_extras,
        # Fernet-randomised ciphertext (license/utils/encryption.py):
        # rows flagged is_encrypted compare by decrypted plaintext.
        encrypted_columns={"instance_configurations": ["value"]},
        # Workspace.background_color default=get_random_color
        # (db/models/workspace.py:139): minted per backend at create.
        generated_columns={"workspaces": ["background_color"]},
        # Known divergence (PIDASHCONV-886): the workspace post_save
        # receiver seeds two builtin scheduler rows; the Rust create
        # path never ports the signal. Exact normalised rows,
        # django-only; the gap tracker fails loudly if they stop
        # appearing.
        known_extra_rows=[
            ("schedulers", "PIDASHCONV-886", {'created_at': '<replay-volatile>', 'updated_at': '<replay-volatile>', 'deleted_at': None, 'id': '<replay-volatile>', 'slug': 'fable-security-audit', 'name': 'Claude Mythos/Fable System Security Audit', 'description': 'Comprehensive Fable-powered audit: scans the project for security vulnerabilities and correctness bugs and files Pi Dash issues for new findings.', 'prompt': 'You are performing a scheduled, read-only security and correctness audit of this\nrepository. Run autonomously to completion — no human is available to answer\nquestions, so make reasonable assumptions and note them rather than stopping.\n\nScope: the entire repository at the current working directory — application code,\nconfiguration, infrastructure-as-code, dependency manifests, and CI/CD\ndefinitions. This is authorized defensive review of the project owner\'s own code.\n\nFind:\n1. Security vulnerabilities — injection (SQL, command, XSS, SSRF, path traversal,\n   template, deserialization); broken authn/authz (missing checks, IDOR,\n   privilege escalation, weak sessions); secrets committed or logged;\n   cryptographic misuse; insecure configuration (permissive CORS, verbose prod\n   errors, open cloud resources, unauthenticated endpoints); vulnerable or\n   outdated dependencies with known CVEs (read manifests + lockfiles; if a\n   scanner such as npm audit / pip-audit / osv-scanner / trivy is available, run\n   it and fold in the results); unsafe file handling and redirects.\n2. Correctness bugs — logic errors that could cause wrong behavior, data loss,\n   crashes, race conditions, or resource leaks, traced through real data flow\n   (not lint-level nits).\n\nMethod: work at high thoroughness. Trace data from untrusted inputs to sensitive\nsinks and reason about WHY each finding is exploitable or wrong, not just whether\na pattern matches. Read the actual code paths. Where deterministic tooling exists\nin the repo (SAST, dependency scanners, type checkers, the test suite), run it\nand incorporate the output. Do NOT modify code, commit, or open PRs — read-only.\n\nReport EVERY issue you find, including uncertain or low-severity ones; do not\nfilter while finding. For each finding capture: a short specific title, the file\npath and line range, category (security|correctness) and a specific subtype,\nseverity (high|medium|low), confidence (high|medium|low), a 1-3 sentence\nexplanation of the exploit path or failure mode, and a concrete fix.\n\nFor each NEW finding, create a Pi Dash issue with the `pi-dash` CLI. Use the\ntitle prefix "[fable-security]" for security findings and "[fable-bug]" for\ncorrectness findings (a dedicated namespace so this audit does not collide with\nthe basic "[security]" Security Audit scheduler):\n    pi-dash issue create \\\n      --title "[fable-security] <short summary>" \\\n      --description "<file path + line range, category/subtype, severity,\n                     confidence, explanation, and suggested fix>"\n\nBefore creating an issue, list existing open issues with the "[fable-security]"\nor "[fable-bug]" title prefix and skip any finding that already has a\ncorresponding open issue (de-dupe by file + subtype, not by exact title). Never\nrefile duplicates.\n\nEnd with a one-line summary: issues filed, duplicates skipped, breakdown by\nseverity. If there are no new findings, file nothing and report "No new findings".\n', 'source': 'builtin', 'is_enabled': True, 'created_by_id': 'f3c361a8-d936-4bc7-9967-db4acc1fcbfa', 'updated_by_id': None, 'workspace_id': '{ws_new}', 'color': '#3b82f6'}),
            ("schedulers", "PIDASHCONV-886", {'created_at': '<replay-volatile>', 'updated_at': '<replay-volatile>', 'deleted_at': None, 'id': '<replay-volatile>', 'slug': 'security-audit', 'name': 'Security Audit', 'description': 'Scans the project for common security vulnerabilities and files Pi Dash issues for any new findings.', 'prompt': 'Scan this project\'s source code for potential security vulnerabilities\n(injection, auth bypass, secret leakage, unsafe deserialization, SSRF,\ninsecure defaults).\n\nFor each finding, create a Pi Dash issue using the `pi-dash` CLI:\n    pi-dash issue create \\\n      --title "[security] <short summary>" \\\n      --description "<file path, line range, vulnerable snippet,\n                     severity (high|medium|low), and suggested fix>"\n\nBefore creating an issue, list existing open issues with the\n"[security]" title prefix and skip any finding that already has a\ncorresponding open issue (de-dupe by file + rule, not by exact title).\n', 'source': 'builtin', 'is_enabled': True, 'created_by_id': 'f3c361a8-d936-4bc7-9967-db4acc1fcbfa', 'updated_by_id': None, 'workspace_id': '{ws_new}', 'color': '#3b82f6'}),
        ],
        requests=(
            # -- instance: rename, read-only fields ignored, overlong --
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "Renamed"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "Kept", "is_setup_done": False,
                      "email": "x@y.zz"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "n" * 300},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "x"},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "x"},
                auth="anonymous", expect_status=401,
            ),
            # -- configurations: plain, padded, unknown, encrypted --
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PLAIN": "new"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PADDED": "  padded  "},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"NO_SUCH_KEY": "x"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_SECRET": "fresh"},
                auth="admin:admin", expect_status=200,
            ),
            # GET re-reads the stored rows through the decrypting
            # serializer: proves both backends' ciphertext decrypts to
            # the same plaintext (the DB compare already asserts it via
            # encrypted_columns; this asserts the read path too).
            Request(
                "GET", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "DELETE", "/api/instances/configurations/disable-email-feature/",
                route="/api/instances/configurations/disable-email-feature/",
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/email-credentials-check/",
                route="/api/instances/email-credentials-check/",
                json={},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PLAIN": "z"},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "DELETE", "/api/instances/configurations/disable-email-feature/",
                route="/api/instances/configurations/disable-email-feature/",
                auth="anonymous", expect_status=401,
            ),
            # -- admins: invite, unknown, missing, delete, idempotent --
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": NEWCOMER_EMAIL, "role": 20},
                auth="admin:admin", capture={"newadmin": "id"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": "ghost@example.com"},
                auth="admin:admin", expect_status=404,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": NEWCOMER_EMAIL},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "DELETE", "/api/instances/admins/{newadmin}/",
                route="/api/instances/admins/{}/",
                auth="admin:admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/instances/admins/00000000-0000-0000-0000-000000000000/",
                route="/api/instances/admins/{}/",
                auth="admin:admin", expect_status=204,
            ),
            # -- signup-screen-visited: admin then anonymous (AllowAny,
            # idempotent re-mark) --
            Request(
                "POST", "/api/instances/admins/sign-up-screen-visited/",
                route="/api/instances/admins/sign-up-screen-visited/",
                auth="admin:admin", expect_status=204,
            ),
            Request(
                "POST", "/api/instances/admins/sign-up-screen-visited/",
                route="/api/instances/admins/sign-up-screen-visited/",
                auth="anonymous", expect_status=204,
            ),
            # -- token-less form POSTs: the custom CSRF_FAILURE_VIEW
            # (HTTP 200 + csrf_failure template) is the contract --
            Request(
                "POST", "/api/instances/admins/sign-in/",
                route="/api/instances/admins/sign-in/",
                form={"email": "replay-admin@example.com",
                      "password": "ContractPass123!"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/admins/sign-up/",
                route="/api/instances/admins/sign-up/",
                form={"email": "fresh@example.com",
                      "password": "SomeStrongPass123!",
                      "first_name": "Fresh"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/admins/sign-out/",
                route="/api/instances/admins/sign-out/",
                auth="admin:admin", expect_status=200,
            ),
            # -- workspaces: create + validation --
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "New Space", "slug": "new-space"},
                auth="admin:admin", capture={"ws_new": "id"},
                # background_color is minted random per backend (see
                # generated_columns); presence-asserted here.
                body_blank_paths=["background_color"],
                expect_status=201,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Only Name"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "n" * 81, "slug": "s" * 49},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Api", "slug": "api"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Acme 2", "slug": "REPLAY-WS"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Nope", "slug": "nope"},
                auth="admin:member", expect_status=403,
            ),
        ),
    ),
)
