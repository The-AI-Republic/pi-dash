"""Contract: page ``?search=`` term splitting (DRF ``SearchFilter``).

``PageViewSet`` (``app/views/page/base.py:79,88``) sets
``search_fields = ["name"]`` and runs the queryset through
``filter_backends = (DjangoFilterBackend, SearchFilter)``. DRF 3.15.2
``SearchFilter.get_search_terms`` -> ``search_smart_split`` groups quoted
phrases: ``?search="foo bar"`` is ONE ANDed ``name__icontains`` term
``foo bar`` (quotes removed), ``?search="a\\"b"`` unescapes to ``a"b``,
and a comma-separated token splits on commas. The terms feed both the list
(``list_pages``) and the retrieve re-read of ``get_queryset``.

These pin the Django behaviour so the Rust port keeps it (before the fix
``search_terms`` did ``replace(',', ' ')`` + ``split_whitespace``, turning
``"foo bar"`` into two terms that keep the literal quote characters).
"""

import pytest

from .conftest import page_url

pytestmark = pytest.mark.contract

QUOTED_PHRASE = '"foo bar"'
ESCAPED_QUOTE = '"a\\"b"'
COMMA_MIXED = 'foo, "bar baz", qux'


def _seed_pages(seeder, world):
    """Phrase/quote pages linked to the world project."""
    pages = {}
    names = {
        "phrase": "foo bar",
        "phrase_ext": "foo bar baz qux",
        "quote": 'a"b',
        "bar_baz": "bar baz",
    }
    for key, name in names.items():
        page = seeder.create_page(world["workspace"]["id"], world["owner"]["id"], name=name)
        seeder.link_page_project(
            world["workspace"]["id"], world["project"]["id"], page["id"], by_id=world["owner"]["id"]
        )
        pages[key] = page
    return pages


def test_list_search_quoted_phrase_is_one_term(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    response = user_client.get(page_url(world), params={"search": QUOTED_PHRASE})
    assert response.status_code == 200, response.text
    # One term "foo bar": A and B contain the phrase; the world's
    # "Contract page" does not.
    assert {row["id"] for row in response.json()} == {
        pages["phrase"]["id"],
        pages["phrase_ext"]["id"],
    }


def test_retrieve_search_quoted_phrase_matches(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    response = user_client.get(
        page_url(world, pages["phrase"]["id"]), params={"search": QUOTED_PHRASE}
    )
    assert response.status_code == 200, response.text
    assert response.json()["id"] == pages["phrase"]["id"]


def test_list_search_escaped_quote_unescapes(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    # ?search="a\"b" -> unescape_string_literal -> term  a"b
    response = user_client.get(page_url(world), params={"search": ESCAPED_QUOTE})
    assert response.status_code == 200, response.text
    assert {row["id"] for row in response.json()} == {pages["quote"]["id"]}


def test_retrieve_search_escaped_quote_matches(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    response = user_client.get(
        page_url(world, pages["quote"]["id"]), params={"search": ESCAPED_QUOTE}
    )
    assert response.status_code == 200, response.text
    assert response.json()["id"] == pages["quote"]["id"]


def test_list_search_comma_mixed_quoted_and_bare(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    # tokens: foo, "bar baz", qux -> terms ["foo", "bar baz", "qux"]
    response = user_client.get(page_url(world), params={"search": COMMA_MIXED})
    assert response.status_code == 200, response.text
    assert {row["id"] for row in response.json()} == {pages["phrase_ext"]["id"]}


def test_retrieve_search_comma_mixed_matches(user_client, world, seeder):
    pages = _seed_pages(seeder, world)
    response = user_client.get(
        page_url(world, pages["phrase_ext"]["id"]), params={"search": COMMA_MIXED}
    )
    assert response.status_code == 200, response.text
    assert response.json()["id"] == pages["phrase_ext"]["id"]


def test_list_search_bare_comma_is_two_terms(user_client, world, seeder):
    # Regression guard: unquoted comma-separated input still splits into
    # two ANDed terms ("foo", "bar") and does not become one literal token.
    pages = _seed_pages(seeder, world)
    response = user_client.get(page_url(world), params={"search": "foo,bar"})
    assert response.status_code == 200, response.text
    assert {row["id"] for row in response.json()} == {
        pages["phrase"]["id"],
        pages["phrase_ext"]["id"],
    }
