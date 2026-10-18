"""Contract: huge JSON integers in page write fields (PIDASHCONV-1175).

DRF ``CharField.to_internal_value`` stringifies a number with Python
``str()`` (``fields.py:759-766``), and Python's ``json`` parses a digit-only
literal of any size as an unbounded ``int``, so ``str(2**100)`` is its 31
exact decimal digits. ``ChoiceField.to_internal_value`` (``access``) renders
its miss the same way: ``'"%s" is not a valid choice.' % str(data)``.

The app_pages kernel rendered such a token through ``f64`` (``py_scalar``
falling back to ``as_f64`` -> ``py_float_str``), so ``2**100`` became
``1.2676506002282294e+30``: a different stored value, a different body, and a
different ``access`` message. These bodies are sent as raw JSON text so no
client-side float conversion can hide the bug.
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

# 2**100, past i64 and u64. Served exactly by Python's unbounded int.
HUGE_DIGITS = "1267650600228229401496703205376"
HUGE_TEXT = HUGE_DIGITS


def _raw(payload: str):
    """A raw JSON body (no client-side number coercion)."""
    return {"content": payload.encode(), "headers": {"Content-Type": "application/json"}}


def test_create_page_huge_int_name(user_client, world, db):
    response = user_client.post(page_url(world), **_raw('{"name": ' + HUGE_DIGITS + "}"))
    assert response.status_code == 201, response.text
    body = response.json()
    assert body["name"] == HUGE_TEXT
    assert db.fetchval("SELECT name FROM pages WHERE id=%s", (body["id"],)) == HUGE_TEXT


def test_partial_update_page_huge_int_name(user_client, world, db):
    response = user_client.patch(
        page_url(world, world["page"]["id"]), **_raw('{"name": ' + HUGE_DIGITS + "}")
    )
    assert response.status_code == 200, response.text
    assert response.json()["name"] == HUGE_TEXT
    assert db.fetchval("SELECT name FROM pages WHERE id=%s", (world["page"]["id"],)) == HUGE_TEXT


def test_partial_update_page_huge_int_description_html(user_client, world, db):
    response = user_client.patch(
        page_url(world, world["page"]["id"]), **_raw('{"description_html": ' + HUGE_DIGITS + "}")
    )
    assert response.status_code == 200, response.text
    assert response.json()["description_html"] == HUGE_TEXT
    assert (
        db.fetchval("SELECT description_html FROM pages WHERE id=%s", (world["page"]["id"],))
        == HUGE_TEXT
    )


def test_partial_update_page_huge_int_access_message(user_client, world):
    response = user_client.patch(
        page_url(world, world["page"]["id"]), **_raw('{"access": ' + HUGE_DIGITS + "}")
    )
    assert response.status_code == 400, response.text
    assert response.json() == {"access": [f'"{HUGE_TEXT}" is not a valid choice.']}
