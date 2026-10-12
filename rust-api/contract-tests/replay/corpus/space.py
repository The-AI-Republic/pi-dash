# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: public-board (space) write flows (PIDASHCONV-823).

Built from the ``space`` contract suite: test_intake.py (intake-issue
create/patch/delete + inbox alias), test_issue.py (comments, issue and
comment reactions, votes) and test_asset.py (issue-asset mint, patch,
delete/restore, comment-asset bulk link).

The seed reuses the ``Seeder`` factories through a thin raw-conn
adapter (the runner hands ``seed`` a connection, not a ``Database``);
``setup`` reads the generated ids back from the Django copy (identical
to the Rust clone) while the three board anchors are fixed so the
comment-disabled and intake-less boards are addressable statically.
"""

from ..session import Request, Session

_SOFT_DELETE_TASK = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"


ANCHOR = "replay-anchor"
ANCHOR_NC = "replay-anchor-nc"
ANCHOR_NI = "replay-anchor-ni"

MEMBER_EMAIL = "replay-member@example.com"


class _ConnDb:
    """Adapt a raw psycopg connection to the Seeder db surface."""

    def __init__(self, conn):
        self._conn = conn

    def execute(self, sql, params=None):
        with self._conn.cursor() as cur:
            cur.execute(sql, params or ())
            return cur

    def fetchone(self, sql, params=None):
        with self._conn.cursor() as cur:
            cur.execute(sql, params or ())
            return cur.fetchone()

    def fetchval(self, sql, params=None):
        row = self.fetchone(sql, params)
        if not row:
            return None
        return row[0] if not isinstance(row, dict) else next(iter(row.values()))


def _seed_extras(conn) -> None:
    """Full public-board world (mirrors the space suite's world fixture)."""
    from psycopg.rows import dict_row

    from _harness.seed import Seeder, SeedTracker

    adapter = _ConnDb(conn)
    seeder = Seeder(adapter, SeedTracker(adapter), tag="RP823")
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM projects WHERE workspace_id = %s AND identifier = 'RPJ'",
            (ws_id,),
        )
        project_id = str(cur.fetchone()["id"])
        cur.execute(
            "SELECT id FROM states WHERE workspace_id = %s AND project_id = %s"
            " ORDER BY created_at LIMIT 1",
            (ws_id, project_id),
        )
        state_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = %s", (MEMBER_EMAIL,))
        member_id = str(cur.fetchone()["id"])

    intake = seeder.create_intake(ws_id, project_id)
    seeder.create_board(ws_id, project_id, anchor=ANCHOR, intake_id=intake["id"])
    issue = seeder.create_issue(ws_id, project_id, state_id)
    seeder.create_comment(ws_id, project_id, issue["id"], member_id)
    bridge_issue = seeder.create_issue(ws_id, project_id, state_id)
    bridge = seeder.create_intake_issue(ws_id, project_id, intake["id"], bridge_issue["id"])
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (member_id, bridge_issue["id"]),
        )
        cur.execute(
            "UPDATE intake_issues SET created_by_id = %s WHERE id = %s",
            (member_id, bridge["id"]),
        )
    # Patch asset with seeded metadata (skips the Celery metadata probe,
    # like the suite) plus delete/restore and bulk-link assets.
    patch_asset = seeder.create_asset(ws_id, project_id, member_id, uploaded=False)
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE file_assets SET storage_metadata = '{\"seeded\": true}' WHERE id = %s",
            (patch_asset["id"],),
        )
    seeder.create_asset(ws_id, project_id, member_id)
    seeder.create_asset(
        ws_id, project_id, member_id, entity_type="COMMENT_DESCRIPTION"
    )
    # Comments-disabled board on a fresh project (the 400 branch; one
    # live board per project).
    nc_project = seeder.create_project(ws_id, name="Replay NoComments")
    nc_state = seeder.create_state(ws_id, nc_project["id"])
    seeder.create_board(ws_id, nc_project["id"], anchor=ANCHOR_NC, comments=False)
    nc_issue = seeder.create_issue(ws_id, nc_project["id"], nc_state["id"])
    # Intake-less board on another fresh project (the 400 branch).
    ni_project = seeder.create_project(ws_id, name="Replay NoIntake")
    seeder.create_board(ws_id, ni_project["id"], anchor=ANCHOR_NI)
    conn.commit()


def _setup(world: dict, config) -> dict:
    """Read the seeded addresses back (identical on both backends)."""
    import psycopg
    from psycopg.rows import dict_row

    ws_id = world["workspaces"]["ws"]["id"]
    project_id = world["projects"]["main"]["id"]
    with psycopg.connect(config.django_db, autocommit=True) as conn:
        with conn.cursor(row_factory=dict_row) as cur:
            cur.execute(
                "SELECT id FROM intakes WHERE project_id = %s", (project_id,)
            )
            intake_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM issues WHERE project_id = %s"
                " AND id NOT IN (SELECT issue_id FROM intake_issues"
                " WHERE intake_id = %s)",
                (project_id, intake_id),
            )
            issue_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM issue_comments WHERE issue_id = %s", (issue_id,)
            )
            comment_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM intake_issues WHERE intake_id = %s", (intake_id,)
            )
            bridge_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM file_assets WHERE project_id = %s"
                " AND storage_metadata IS NOT NULL",
                (project_id,),
            )
            patch_asset_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM file_assets WHERE project_id = %s"
                " AND entity_type = 'COMMENT_DESCRIPTION'",
                (project_id,),
            )
            comment_asset_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT id FROM file_assets WHERE project_id = %s"
                " AND storage_metadata IS NULL"
                " AND entity_type <> 'COMMENT_DESCRIPTION'",
                (project_id,),
            )
            delete_asset_id = str(cur.fetchone()["id"])
            cur.execute(
                "SELECT i.id FROM issues i JOIN projects p ON p.id = i.project_id"
                " WHERE p.workspace_id = %s AND p.name = 'Replay NoComments'",
                (ws_id,),
            )
            nc_issue_id = str(cur.fetchone()["id"])
    return {
        "intake": intake_id,
        "issue": issue_id,
        "seed_comment": comment_id,
        "bridge": bridge_id,
        "patch_asset": patch_asset_id,
        "comment_asset": comment_asset_id,
        "delete_asset": delete_asset_id,
        "nc_issue": nc_issue_id,
        "anchor": ANCHOR,
        "anchor_nc": ANCHOR_NC,
        "anchor_ni": ANCHOR_NI,
    }


SESSIONS = (
    Session(
        name="space_lifecycle",
        domain="space",
        source="space/test_intake.py, test_issue.py, test_asset.py",
        auth_users=("member", "outsider"),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=(
            "intake", "issue", "seed_comment", "bridge", "patch_asset",
            "comment_asset", "delete_asset", "nc_issue",
            "anchor", "anchor_nc", "anchor_ni",
        ),
        # issue_activity carries int(timezone.now()) as `epoch`: the two
        # backends' requests can straddle a second boundary (app_issues).
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # The vote POST response carries no id, so the vote DELETE's
        # soft_delete pk (args[2]) is backend-local with no capture to
        # name it: resolve it as an existence-checked row ref instead.
        # Other DELETEs' pks are capture-substituted (or live in other
        # tables), so this ref only ever blanks the vote row's pk.
        payload_row_refs={
            _SOFT_DELETE_TASK: [("args/2", "issue_votes")],
            # issue_vote.activity.deleted carries the vote row pk as
            # current_instance.identifier (also backend-local, also
            # never in a response).
            "pi_dash.bgtasks.issue_activities_task.issue_activity": [
                ("kwargs/current_instance/identifier", "issue_votes"),
            ],
        },
        requests=(
            # -- intake-issues: create via both aliases --
            Request(
                "POST", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/",
                json={"issue": {"name": "Intake idea", "priority": "low",
                                "description_html": "<p>why</p>"}},
                auth="member",
                # The view get-or-creates the Triage state per backend
                # (per-side id); the bridge id is per-side too.
                capture={
                    "intake_issue": "id",
                    "bridge0": "issue_intake/0/id",
                    "triage": "state",
                },
                expect_status=200,
            ),
            Request(
                "POST", "/api/public/anchor/{anchor}/intakes/{intake}/inbox-issues/",
                route="/api/public/anchor/{}/intakes/{}/inbox-issues/",
                json={"issue": {"name": "Inbox idea"}},
                auth="member",
                capture={"inbox_issue": "id", "bridge1": "issue_intake/0/id"},
                expect_status=200,
            ),
            Request(
                "POST", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/",
                json={"issue": {"priority": "low"}},
                auth="member", expect_status=400,
            ),
            Request(
                "POST", "/api/public/anchor/{anchor_ni}/intakes/{intake}/intake-issues/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/",
                json={"issue": {"name": "no intake"}},
                auth="member", expect_status=400,
            ),
            # -- intake-issues: outsider is rejected, creator edits --
            Request(
                "PATCH", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/{bridge}/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/{}/",
                json={"issue": {"name": "hijack"}},
                auth="outsider", expect_status=400,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/{bridge}/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/{}/",
                auth="outsider", expect_status=400,
            ),
            Request(
                "PATCH", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/{bridge}/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/{}/",
                json={"issue": {"name": "Renamed idea", "description_html": "<p>new</p>"}},
                auth="member", expect_status=200,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/intakes/{intake}/intake-issues/{bridge}/",
                route="/api/public/anchor/{}/intakes/{}/intake-issues/{}/",
                auth="member", expect_status=204,
            ),
            # -- issue comments: create, outsider 404, creator edit, delete --
            Request(
                "POST", "/api/public/anchor/{anchor}/issues/{issue}/comments/",
                route="/api/public/anchor/{}/issues/{}/comments/",
                json={"comment_html": "<p>public note</p>"},
                auth="member",
                capture={"comment": "id", "comment_desc": "description"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/public/anchor/{anchor_nc}/issues/{nc_issue}/comments/",
                route="/api/public/anchor/{}/issues/{}/comments/",
                json={"comment_html": "<p>x</p>"},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", "/api/public/anchor/{anchor}/issues/{issue}/comments/{comment}/",
                route="/api/public/anchor/{}/issues/{}/comments/{}/",
                json={"comment_html": "<p>hijack</p>"},
                auth="outsider", expect_status=404,
            ),
            Request(
                "PATCH", "/api/public/anchor/{anchor}/issues/{issue}/comments/{comment}/",
                route="/api/public/anchor/{}/issues/{}/comments/{}/",
                json={"comment_html": "<p>edited</p>"},
                auth="member", expect_status=200,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/issues/{issue}/comments/{comment}/",
                route="/api/public/anchor/{}/issues/{}/comments/{}/",
                auth="member", expect_status=204,
            ),
            # -- issue reactions: create + delete --
            Request(
                "POST", "/api/public/anchor/{anchor}/issues/{issue}/reactions/",
                route="/api/public/anchor/{}/issues/{}/reactions/",
                json={"reaction": "rocket"},
                auth="member", capture={"ireaction": "id"},
                expect_status=201,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/issues/{issue}/reactions/rocket/",
                route="/api/public/anchor/{}/issues/{}/reactions/{}/",
                auth="member", expect_status=204,
            ),
            # -- issue votes: upsert + delete + delete-again 404 --
            Request(
                "POST", "/api/public/anchor/{anchor}/issues/{issue}/votes/",
                route="/api/public/anchor/{}/issues/{}/votes/",
                json={"vote": 1},
                auth="member", expect_status=201,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/issues/{issue}/votes/",
                route="/api/public/anchor/{}/issues/{}/votes/",
                auth="member", expect_status=204,
            ),
            # -- comment reactions: create + delete (on the live seeded
            # comment; the created comment was edited + deleted above) --
            Request(
                "POST", "/api/public/anchor/{anchor}/comments/{seed_comment}/reactions/",
                route="/api/public/anchor/{}/comments/{}/reactions/",
                json={"reaction": "eyes"},
                auth="member", capture={"creaction": "id"},
                expect_status=201,
            ),
            Request(
                "DELETE", "/api/public/anchor/{anchor}/comments/{seed_comment}/reactions/eyes/",
                route="/api/public/anchor/{}/comments/{}/reactions/{}/",
                auth="member", expect_status=204,
            ),
            # -- assets: mint, patch, delete/restore, bulk link --
            Request(
                "POST", "/api/public/assets/v2/anchor/{anchor}/",
                route="/api/public/assets/v2/anchor/{}/",
                form={"name": "shot.png", "type": "image/png", "size": "10",
                      "entity_type": "ISSUE_DESCRIPTION"},
                auth="member",
                # The stored file_assets.asset key is the same value the
                # response returns (Django mints one asset_key and both
                # stores and presigns it), so capturing the response key
                # also substitutes the DB cell on each side — and fails
                # loudly if a backend ever stores a different key than
                # it returns.
                capture={"asset_id": "asset_id",
                         "asset_key": "upload_data/fields/key"},
                # The mint response embeds per-backend randomness in six
                # leaves: asset_url's trailing id, the presigned key
                # (an independent random uuid, dashless, plus filename),
                # the policy + signature derived from that key, and the
                # SigV4 mint instant (x-amz-date is per-second — mints
                # straddling a second boundary flaked the v1_assets
                # session's exact compare — while x-amz-credential
                # embeds the date, a UTC-midnight flake).
                # Presence-asserted, values blanked; every other leaf
                # (Content-Type, algorithm, url, the asset_id capture
                # itself) still compares exactly.
                body_blank_paths=[
                    "asset_url",
                    "upload_data/fields/key",
                    "upload_data/fields/policy",
                    "upload_data/fields/x-amz-signature",
                    "upload_data/fields/x-amz-date",
                    "upload_data/fields/x-amz-credential",
                ],
                expect_status=200,
            ),
            Request(
                "POST", "/api/public/assets/v2/anchor/{anchor}/",
                route="/api/public/assets/v2/anchor/{}/",
                form={"name": "shot.png", "type": "image/png", "size": "10",
                      "entity_type": "NOPE"},
                auth="member", expect_status=400,
            ),
            Request(
                "PATCH", "/api/public/assets/v2/anchor/{anchor}/{patch_asset}/",
                route="/api/public/assets/v2/anchor/{}/{}/",
                json={"attributes": {"name": "renamed.png"}},
                auth="member", expect_status=204,
            ),
            Request(
                "DELETE", "/api/public/assets/v2/anchor/{anchor}/{delete_asset}/",
                route="/api/public/assets/v2/anchor/{}/{}/",
                auth="member", expect_status=204,
            ),
            Request(
                "POST", "/api/public/assets/v2/anchor/{anchor}/restore/{delete_asset}/",
                route="/api/public/assets/v2/anchor/{}/restore/{}/",
                auth="member", expect_status=204,
            ),
            Request(
                "POST", "/api/public/assets/v2/anchor/{anchor}/{seed_comment}/bulk/",
                route="/api/public/assets/v2/anchor/{}/{}/bulk/",
                json={"asset_ids": ["{comment_asset}"]},
                auth="member", expect_status=204,
            ),
            Request(
                "POST", "/api/public/assets/v2/anchor/{anchor}/{seed_comment}/bulk/",
                route="/api/public/assets/v2/anchor/{}/{}/bulk/",
                json={"asset_ids": []},
                auth="member", expect_status=400,
            ),
        ),
    ),
)
