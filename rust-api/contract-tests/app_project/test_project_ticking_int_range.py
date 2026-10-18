"""Contract tests: agent-ticking int fields at the int4 bound (PIDASHCONV-1135).

The issue that filed this file claimed Django has no numeric fence on the
four agent-ticking int fields and only Rust does. That premise is false:
Django's ``IntegerField.validators`` is a ``cached_property`` that appends
the backend's column range — ``MinValueValidator(-2147483648)`` /
``MaxValueValidator(2147483647)`` from
``connection.ops.integer_field_range("IntegerField")`` — and DRF 3.15.2
``ModelSerializer`` copies those into the serializer field via
``rest_framework/utils/field_mapping.py:get_field_kwargs``. The model
source has no *declared* validators, but at runtime they are there, so
both backends reject an out-of-i32 value with DRF's bound message before
``validate_agent_*`` ever runs.

These tests pin the shared behavior (identical status and bytes on both
backends), so a Rust-only fence change — or its removal — cannot land
silently:

* at the bound (``±2^31``) the value is accepted, or, for a negative
  cadence / non-positive ``max_ticks``, the serializer field validator
  answers ``AGENT_CADENCE_TOO_SHORT`` / ``AGENT_MAX_TICKS_*``;
* past the bound the DRF ``Min``/``MaxValueValidator`` message wins,
  400, no row written, neither side 500s.
"""

import uuid

import pytest

I32_MAX = 2**31 - 1
I32_MIN = -(2**31)
PAST_POSITIVE = [I32_MAX + 1, 9_999_999_999]
PAST_NEGATIVE = [I32_MIN - 1, -9_999_999_999]

TICKING_INTERVAL_FIELDS = [
    "agent_default_interval_seconds",
    "agent_review_default_interval_seconds",
    "agent_test_default_interval_seconds",
]
TICKING_FIELDS = TICKING_INTERVAL_FIELDS + ["agent_default_max_ticks"]

ABOVE_MESSAGE = "Ensure this value is less than or equal to 2147483647."
BELOW_MESSAGE = "Ensure this value is greater than or equal to -2147483648."


def _fence_body(field, value):
    return {field: [ABOVE_MESSAGE if value > 0 else BELOW_MESSAGE]}


def _negative_in_range_body(field):
    if field == "agent_default_max_ticks":
        return {field: ["AGENT_MAX_TICKS_MUST_BE_POSITIVE_OR_INFINITE"]}
    return {field: ["AGENT_CADENCE_TOO_SHORT"]}


def _project_field(world, project_id, field):
    with world.conn.cursor() as cur:
        cur.execute("SELECT %s FROM projects WHERE id = %%s" % field,
                    (str(project_id),))
        return cur.fetchone()[0]


def _project_ids(world, identifier):
    with world.conn.cursor() as cur:
        cur.execute("SELECT id FROM projects WHERE identifier = %s",
                    (identifier,))
        return [str(row[0]) for row in cur.fetchall()]


@pytest.mark.parametrize("field", TICKING_FIELDS)
@pytest.mark.parametrize("value", PAST_POSITIVE + PAST_NEGATIVE)
def test_partial_update_past_i32_fenced_400_unchanged(world, field, value):
    client, _, ws, project = world.full_stack()
    before = _project_field(world, project["id"], field)
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={field: value})
    assert resp.status_code == 400
    assert resp.json() == _fence_body(field, value)
    assert _project_field(world, project["id"], field) == before


@pytest.mark.parametrize("field", TICKING_FIELDS)
@pytest.mark.parametrize("value", PAST_POSITIVE + PAST_NEGATIVE)
def test_create_past_i32_fenced_400_no_row(world, field, value):
    user = world.user("tickpast")
    ws = world.workspace(user, "tickpast")
    world.ws_member(ws, user, 20)
    client = world.client(user)
    identifier = "PAST%s" % uuid.uuid4().hex[:6].upper()
    resp = client.post(f"/api/workspaces/{ws['slug']}/projects/",
                       json={"name": "Past i32", "identifier": identifier,
                             field: value})
    assert resp.status_code == 400
    assert resp.json() == _fence_body(field, value)
    assert _project_ids(world, identifier) == []


@pytest.mark.parametrize("field", TICKING_FIELDS)
def test_partial_update_at_i32_max_accepted(world, field):
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={field: I32_MAX})
    assert resp.status_code == 200
    assert resp.json()[field] == I32_MAX
    assert _project_field(world, project["id"], field) == I32_MAX


@pytest.mark.parametrize("field", TICKING_FIELDS)
def test_partial_update_at_i32_min_reaches_field_validator(world, field):
    # In range, so the bound validators pass and `validate_agent_*` runs:
    # this is the case where the AGENT* message *is* reachable, and the
    # fence must not pre-empt it.
    client, _, ws, project = world.full_stack()
    resp = client.patch(f"/api/workspaces/{ws['slug']}/projects/{project['id']}/",
                        json={field: I32_MIN})
    assert resp.status_code == 400
    assert resp.json() == _negative_in_range_body(field)
