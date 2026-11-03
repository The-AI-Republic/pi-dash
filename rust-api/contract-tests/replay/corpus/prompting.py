# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: prompting write flows (PIDASHCONV-823).

Built from the ``prompting`` contract suite: test_section_write.py
(workspace/user overrides, version bumps, locked sections, jinja and
variable validation, delete-to-default) and test_preview.py (prompt
rendering per kind, relationships, drafts, scope gates). The seed
reuses the domain's ``Seed`` factories (project, states, issues,
relations, scheduler binding, one admin-only override).
"""

from ..session import Request, Session
from ._csrf import csrf as _csrf
from ._csrf import prime as _prime

_BASE = "/api/workspaces/{ws}/prompt-sections"
_MISSING = "00000000-0000-0000-0000-000000000000"

# The preview mints a fresh PIDASH_RUN_ID per render and weaves it
# through the prompt (3 spots in coding-task). Context-anchored so the
# stable issue/project uuids in the same text still compare exactly.
_RUN_ID_PATTERNS = [
    r'agent-run-id "[0-9a-f-]{36}"',
    r"agent run \(`[0-9a-f-]{36}`\)",
]
# The scheduler prompt carries the --agent-run-id spot plus a
# "Run id:" line instead of the backtick context.
_RUN_ID_CLI_PATTERN = [
    r'agent-run-id "[0-9a-f-]{36}"',
    r"Run id: [0-9a-f-]{36}",
]


def _preview(kind: str) -> str:
    return "/api/workspaces/{ws}/prompts/" + kind + "/preview"


def _seed_extras(conn) -> None:
    from _harness.seed import Seed

    with conn.cursor() as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-other'")
        other_ws = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()[0])
    seed = Seed(conn)
    project = seed.project(ws_id)
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE projects SET description = %s WHERE id = %s",
            ("Reject stubs; run two independent review passes.", project),
        )
    todo = seed.state(project, ws_id)
    seed.issue(ws_id, project, name="Prompting plain", state_id=todo)
    handler = seed.issue(ws_id, project, name="Handler", sequence_id=11, state_id=todo)
    blocker = seed.issue(ws_id, project, name="Model layer", sequence_id=12, state_id=todo)
    dependent = seed.issue(ws_id, project, name="Follow-up", sequence_id=13, state_id=todo)
    seed.issue_relation(ws_id, project, handler, blocker)
    seed.issue_relation(ws_id, project, dependent, handler)
    intest = seed.state(project, ws_id, name="In Test", group="test")
    seed.issue(ws_id, project, name="Port the parser", state_id=intest)
    other_project = seed.project(other_ws)
    other_state = seed.state(other_project, other_ws)
    seed.issue(other_ws, other_project, name="Foreign", state_id=other_state)
    sched = seed.scheduler(ws_id)
    seed.scheduler_binding(ws_id, project, sched, admin_id)
    seed.override(ws_id, "implementation", "ADMIN-ONLY-MARKER", user_id=admin_id)
    conn.commit()


def _setup(world, config) -> dict:
    import psycopg

    with psycopg.connect(config.django_db) as conn, conn.cursor() as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()[0])
        out = {}
        for name, key in (
            ("Prompting plain", "issue"),
            ("Handler", "handler"),
            ("Port the parser", "parser"),
        ):
            cur.execute(
                "SELECT i.id FROM issues i JOIN projects p ON p.id = i.project_id"
                " WHERE p.workspace_id = %s AND i.name = %s",
                (ws_id, name),
            )
            out[key] = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-other'")
        other_ws = str(cur.fetchone()[0])
        cur.execute(
            "SELECT i.id FROM issues i JOIN projects p ON p.id = i.project_id"
            " WHERE p.workspace_id = %s AND i.name = 'Foreign'",
            (other_ws,),
        )
        out["foreign_issue"] = str(cur.fetchone()[0])
        cur.execute(
            "SELECT b.id FROM scheduler_bindings b WHERE b.workspace_id = %s",
            (ws_id,),
        )
        out["binding"] = str(cur.fetchone()[0])
    return out


SESSIONS = (
    Session(
        name="prompting_lifecycle",
        domain="prompting",
        source="prompting/test_section_write.py, test_preview.py",
        auth_users=("admin", "member", "outsider"),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=("issue", "handler", "parser", "foreign_issue", "binding"),
        requests=(
            # One CSRF prime for the whole session: no login/logout
            # rotates the secret in this flow, and 30+ anonymous
            # primes would trip the 30/min anonymous throttle.
            _prime("p0"),
            # -- section writes: workspace override lifecycle --
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "Custom workspace guidance."},
                auth="admin", capture={"ov_ws": "id"}, expect_status=200,
                **_csrf("p0"),
            ),
            Request(
                "GET", _BASE + "?kind=coding-task&scope=workspace",
                route="/api/workspaces/{}/prompt-sections",
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "v1"},
                auth="admin", capture={"ov_v1": "id"}, expect_status=200,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "v2"},
                auth="admin", capture={"ov_v2": "id"}, expect_status=200,
                **_csrf("p0"),
            ),
            # -- section writes: roles, locked sections, validation --
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "nope"},
                auth="member", expect_status=403,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "user", "body": "My personal guidance."},
                auth="member", capture={"ov_user": "id"}, expect_status=200,
                **_csrf("p0"),
            ),
            # The member's list resolves their own user override while
            # the seeded admin-only marker stays invisible to them.
            Request(
                "GET", _BASE + "?kind=coding-task&scope=user",
                route="/api/workspaces/{}/prompt-sections",
                auth="member", expect_status=200,
            ),
            Request(
                "PUT", _BASE + "/pidash-cli",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "hack the cli docs"},
                auth="admin", expect_status=403,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/pidash-cli",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "user", "body": "hack the cli docs"},
                auth="admin", expect_status=403,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "{{% if x %}}unclosed"},
                auth="admin", expect_status=400,
                # Non-divergence (no issue; cf. the 204 content-type
                # rule): both sides echo their own template engine's
                # diagnostic (Jinja2 vs mini-jinja), which cannot match
                # byte for byte. Status + error code still compare;
                # the suites pin the status only.
                body_blank_paths=["detail"],
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                # Braces doubled: .format() collapses them back to one pair.
                json={"scope": "workspace", "body": "{{{{ totally_unknown }}}}"},
                auth="admin", expect_status=400,
                # Non-divergence, same engine-diagnostic class as above:
                # Jinja2 vs mini-jinja undefined-variable text.
                body_blank_paths=["detail"],
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace"},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/no-such-section",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "x"},
                auth="admin", expect_status=404,
                **_csrf("p0"),
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "x"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "PUT", _BASE + "/implementation",
                route="/api/workspaces/{}/prompt-sections/{}",
                json={"scope": "workspace", "body": "cross-tenant write"},
                auth="outsider", expect_status=403,
                **_csrf("p0"),
            ),
            # -- section delete: revert to default, then 404 --
            Request(
                "DELETE", _BASE + "/implementation",
                params={"scope": "workspace"},
                route="/api/workspaces/{}/prompt-sections/{}",
                auth="admin", expect_status=204,
                **_csrf("p0"),
            ),
            Request(
                "GET", _BASE + "?kind=coding-task&scope=workspace",
                route="/api/workspaces/{}/prompt-sections",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _BASE + "/implementation",
                params={"scope": "workspace"},
                route="/api/workspaces/{}/prompt-sections/{}",
                auth="admin", expect_status=404,
                **_csrf("p0"),
            ),
            Request(
                "DELETE", _BASE + "/implementation",
                params={"scope": "workspace"},
                route="/api/workspaces/{}/prompt-sections/{}",
                auth="outsider", expect_status=403,
                **_csrf("p0"),
            ),
            # -- preview renders per kind (full prompt text compares) --
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{handler}"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("review"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("test"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{parser}"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("scheduler"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"binding_id": "{binding}"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_CLI_PATTERN,
                **_csrf("p0"),
            ),
            # -- preview validation --
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("scheduler"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": _MISSING},
                auth="admin", expect_status=404,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{foreign_issue}"},
                auth="admin", expect_status=404,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("scheduler"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"binding_id": _MISSING},
                auth="admin", expect_status=404,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("bogus"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
            # -- preview gates --
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="member", expect_status=403,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}", "scope": "user"},
                auth="member", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}"},
                auth="outsider", expect_status=403,
                **_csrf("p0"),
            ),
            # -- preview drafts --
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}", "section_key": "implementation",
                      "body": "DRAFT-PREVIEW-MARKER"},
                auth="admin", expect_status=200,
                body_regex_blanks=_RUN_ID_PATTERNS,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}", "section_key": "pidash-cli",
                      "body": "SHOULD-BE-BLOCKED"},
                auth="admin", expect_status=403,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}", "section_key": "implementation"},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
            Request(
                "POST", _preview("coding-task"),
                route="/api/workspaces/{}/prompts/{}/preview",
                json={"issue_id": "{issue}", "section_key": "scheduler-task",
                      "body": "not part of this kind"},
                auth="admin", expect_status=400,
                **_csrf("p0"),
            ),
        ),
    ),
)
