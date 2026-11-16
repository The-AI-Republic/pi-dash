"""Contract test: transfer progress_snapshot burndown uses the UTC day as today.

PIDASHCONV-1417. Django's ``burndown_plot`` nulls chart days after
``timezone.now().date()``, which is always the UTC date (``USE_TZ=True``;
``TimezoneMixin`` activation has no effect on it). The Rust transfer caller
used the actor-zone day instead, so a transfer in the west-of-UTC split
window stored ``null`` for the last chart day.

The test only runs inside the split window: actor zone UTC-11 while the UTC
hour is below 11, so the actor's local date is still yesterday while the
source cycle's end already falls on UTC today.
"""

import datetime as dt
import zoneinfo

import pytest

from _harness import api, db

SLUG = db.WS_A_SLUG
PROJ = db.PROJ_A_ID
ACTOR_TZ = "Pacific/Pago_Pago"


class TestTransferSnapshotTodayUtc:
    def test_snapshot_today_is_utc_day(self, admin_client, db_conn):
        now_utc = dt.datetime.now(dt.timezone.utc)
        if now_utc.hour >= 11:
            pytest.skip("needs UTC hour < 11 for the Pago_Pago split window")
        actor_today = now_utc.astimezone(zoneinfo.ZoneInfo(ACTOR_TZ)).date()
        utc_today = now_utc.date()
        assert actor_today < utc_today

        with db_conn.cursor() as cur:
            cur.execute(
                "UPDATE users SET user_timezone = %s WHERE id = %s",
                [ACTOR_TZ, db.ADMIN_ID],
            )
            cur.execute(
                "UPDATE cycles SET start_date = now() - interval '3 days',"
                " end_date = now() - interval '1 minute'"
                " WHERE id = %s RETURNING end_date",
                [db.CYCLE_COMPLETED_ID],
            )
            end_date = cur.fetchone()[0]
            cur.execute(
                "INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id,"
                " issue_id, project_id, workspace_id) VALUES (gen_random_uuid(),"
                " now(), now(), %s, %s, %s, %s)",
                [db.CYCLE_COMPLETED_ID, db.ISSUE_BACKLOG_ID, PROJ, db.WS_A_ID],
            )
            db_conn.commit()
        # Midnight race: end_date must still fall on UTC today.
        if end_date.astimezone(dt.timezone.utc).date() != utc_today:
            pytest.skip("source cycle end crossed UTC midnight; rerun")

        r = admin_client.post(
            api.cycle_transfer_url(SLUG, PROJ, db.CYCLE_COMPLETED_ID),
            json={"new_cycle_id": db.CYCLE_DRAFT_ID},
        )
        assert r.status_code == 200

        with db_conn.cursor() as cur:
            cur.execute(
                "SELECT progress_snapshot FROM cycles WHERE id = %s",
                [db.CYCLE_COMPLETED_ID],
            )
            snapshot = cur.fetchone()[0]
        chart = snapshot["distribution"]["completion_chart"]
        assert chart[str(utc_today)] == 1
