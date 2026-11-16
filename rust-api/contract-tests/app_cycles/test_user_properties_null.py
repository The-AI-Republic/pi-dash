"""NEWPIDASH-214: PATCH cycle user-properties with JSON null must 400.

Django assigns None to the NOT NULL JSONField columns
(db/models/cycle.py:137-140) and save() raises IntegrityError, which the
generic handle_exception maps to 400 {"error": "The payload is not valid"}
(app/views/base.py:220-224), leaving the row unchanged. The Rust port
previously bound "null"::jsonb and persisted a JSON null scalar with 201.

This case is tier 2 (status code and stored value for an input a JSON
client can send when an unset preference is serialised as null).
"""

import pytest

from .conftest import cycle_urls

pytestmark = pytest.mark.contract

PAYLOAD_NOT_VALID = {"error": "The payload is not valid"}


def _provision(admin):
    urls = cycle_urls(admin)
    pk = admin["cycle"]["id"]
    r = admin["client"].get(urls["user_properties"](pk))
    assert r.status_code == 200, r.text[:500]
    return pk, urls


def test_patch_null_rejected_and_row_unchanged(admin):
    pk, urls = _provision(admin)
    before = admin["client"].get(urls["user_properties"](pk)).json()

    for key in (
        "filters",
        "rich_filters",
        "display_filters",
        "display_properties",
    ):
        r = admin["client"].patch(
            urls["user_properties"](pk), json={key: None}
        )
        assert r.status_code == 400, (key, r.status_code, r.text[:500])
        assert r.json() == PAYLOAD_NOT_VALID, (key, r.json())

    after = admin["client"].get(urls["user_properties"](pk)).json()
    # The row is byte-identical for the persisted JSON columns; updated_at must
    # not advance on the rejected writes (Django leaves row unchanged, no save).
    for col in ("filters", "rich_filters", "display_filters", "display_properties"):
        assert after[col] == before[col], (col, after[col], before[col])
    assert after["updated_at"] == before["updated_at"]


def test_patch_null_in_object_rejected(admin):
    """One null among other valid keys still 400s and leaves the row."""
    pk, urls = _provision(admin)
    before = admin["client"].get(urls["user_properties"](pk)).json()
    r = admin["client"].patch(
        urls["user_properties"](pk),
        json={"filters": {"priority": "high"}, "rich_filters": None},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == PAYLOAD_NOT_VALID
    after = admin["client"].get(urls["user_properties"](pk)).json()
    assert after["filters"] == before["filters"]
    assert after["rich_filters"] == before["rich_filters"]
    assert after["updated_at"] == before["updated_at"]
