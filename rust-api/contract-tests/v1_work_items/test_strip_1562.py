"""Contract: v1 work-item writes strip with MLStripper (PIDASHCONV-1562).

``Issue.save`` computes ``description_stripped`` with
``pi_dash.utils.html_processor.strip_tags`` (``db/models/issue.py:21``) —
``MLStripper`` (``HTMLParser`` with ``convert_charrefs=True``), so entities
decode. The v1 Rust port computed the column with
``api::space::sanitize::strip_tags`` (a port of
``django.utils.html.strip_tags``), which passes entities through verbatim —
``<p>R&amp;D</p>`` stored ``R&amp;D`` where Django stores ``R&D``. Fixed by
migrating the v1 writers onto ``domain::issues`` (PIDASHCONV-1521 PR2); these
cases pin the decoded value on create and PATCH.

The v1 wire shape carries no ``description_stripped`` key, so the column is
observed straight from Postgres like the app_intake twin
(``test_strip_1439.py``).
"""

import pytest

from .conftest import issue_url

pytestmark = pytest.mark.contract

# Byte evidence (issue): Django/ml yield "R&D" (3 bytes, hex 522644) for the
# markdown-shaped witness; the old v1 kernel kept "R&amp;D" (8 bytes).
HTML = "<p>R&amp;D</p>"
DECODED = "R&D"


def _stripped(db, issue_id):
    row = db.fetchone(
        "SELECT description_stripped FROM issues WHERE id = %s", (issue_id,)
    )
    return row["description_stripped"]


def test_work_item_create_stripped_decodes_entities(api, world, db):
    # PIDASHCONV-1562: the create arm (Issue.save, db/models/issue.py:329-334)
    # strips with MLStripper, so the issues row carries DECODED.
    response = api.post(
        issue_url(world),
        json={"name": "entity probe", "description_html": HTML},
    )
    assert response.status_code == 201, response.text
    issue_id = response.json()["id"]
    assert _stripped(db, issue_id) == DECODED


def test_work_item_patch_stripped_decodes_entities(api, world, db):
    # PIDASHCONV-1562: the update arm (db/models/issue.py:344-349) recomputes
    # with the same stripper on PATCH.
    response = api.patch(
        issue_url(world, world["issue"]["id"]),
        json={"description_html": HTML},
    )
    assert response.status_code == 200, response.text
    assert _stripped(db, world["issue"]["id"]) == DECODED
