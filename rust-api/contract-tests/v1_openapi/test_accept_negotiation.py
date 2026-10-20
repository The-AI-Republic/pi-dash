"""`Accept`-header content negotiation on the schema surface (PIDASHCONV-1207).

Pins DRF's `DefaultContentNegotiation.select_renderer` behaviour on the
`/api/schema/` document view and the UI pages: `Accept` picks the renderer
(by DRF precedence), `?format=` filters the candidates first, and an
`Accept` that matches nothing is a 406 rendered by the *first* renderer.
A backend that branches on `?format=` alone (and so renders YAML for every
non-`?format=json` request, never 406s) fails these rows.
"""

import json

import pytest
import yaml

from . import client as sc

JSON_CT = "application/json"
VENDOR_JSON_CT = "application/vnd.oai.openapi+json"
YAML_CT = "application/vnd.oai.openapi; charset=utf-8"
VENDOR_YAML_CT = "application/yaml; charset=utf-8"
HTML_CT = "text/html; charset=utf-8"

#: The `NotAcceptable` 406 body through the document view's first renderer
#: (the OpenAPI YAML renderer): DRF ErrorDetail serialized as YAML.
NOT_ACCEPTABLE_YAML = (
    "detail:\n"
    "  string: Could not satisfy the request Accept header.\n"
    "  code: not_acceptable\n"
)


@pytest.mark.contract
def test_accept_json_returns_json_document():
    r = sc.negotiate(sc.SCHEMA, JSON_CT)
    assert r.status_code == 200
    assert r.headers["content-type"] == JSON_CT
    assert r.headers["content-disposition"] == (
        'inline; filename="The Pi Dash REST API.json"'
    )
    assert r.text.startswith('{\n    "openapi": "3.0.3",')
    json.loads(r.text)


@pytest.mark.contract
def test_accept_vendor_json_returns_json_document():
    r = sc.negotiate(sc.SCHEMA, VENDOR_JSON_CT)
    assert r.status_code == 200
    assert r.headers["content-type"] == VENDOR_JSON_CT
    assert r.headers["content-disposition"] == (
        'inline; filename="The Pi Dash REST API.json"'
    )
    json.loads(r.text)


@pytest.mark.contract
def test_accept_yaml_uses_second_yaml_renderer():
    r = sc.negotiate(sc.SCHEMA, "application/yaml")
    assert r.status_code == 200
    assert r.headers["content-type"] == VENDOR_YAML_CT
    assert r.headers["content-disposition"] == (
        'inline; filename="The Pi Dash REST API.yaml"'
    )
    assert yaml.safe_load(r.text)["openapi"].startswith("3.0.")


@pytest.mark.contract
def test_default_accept_still_yaml():
    """Regression pin: `*/*` still selects the first YAML renderer."""
    r = sc.negotiate(sc.SCHEMA, "*/*")
    assert r.status_code == 200
    assert r.headers["content-type"] == YAML_CT


@pytest.mark.contract
def test_accept_html_is_406_in_yaml():
    r = sc.negotiate(sc.SCHEMA, "text/html")
    assert r.status_code == 406
    assert r.headers["content-type"] == YAML_CT
    assert r.text == NOT_ACCEPTABLE_YAML


@pytest.mark.contract
def test_format_filter_then_unmatched_accept_is_406():
    """`?format=yaml` leaves only the YAML renderers; `Accept: application/json`
    then matches none -> 406 (still rendered by the first renderer)."""
    r = sc.negotiate(sc.SCHEMA, JSON_CT, params={"format": "yaml"})
    assert r.status_code == 406
    assert r.headers["content-type"] == YAML_CT
    assert r.text == NOT_ACCEPTABLE_YAML


@pytest.mark.contract
@pytest.mark.parametrize("path", [sc.SWAGGER_UI, sc.REDOC])
def test_ui_pages_unmatched_accept_is_406_html(path):
    r = sc.negotiate(path, JSON_CT)
    assert r.status_code == 406
    assert r.headers["content-type"] == HTML_CT
    assert r.text == "406 Not Acceptable"


@pytest.mark.contract
def test_unsafe_method_body_follows_accept():
    """Negotiation runs in `initial`, before dispatch, so the accepted
    renderer serializes the 405 as well."""
    r = sc.negotiate(sc.SCHEMA, JSON_CT, method="POST")
    assert r.status_code == 405
    assert r.headers["content-type"] == JSON_CT
    assert json.loads(r.text) == {"detail": 'Method "POST" not allowed.'}

    r = sc.negotiate(sc.SCHEMA, "application/yaml", method="POST")
    assert r.status_code == 405
    assert r.headers["content-type"] == VENDOR_YAML_CT
    assert r.text == (
        'detail:\n  string: Method "POST" not allowed.\n  code: method_not_allowed\n'
    )


@pytest.mark.contract
@pytest.mark.parametrize(
    ("accept", "content_type"),
    [
        ("application/json; q=0.5", JSON_CT),
        ("application/yaml; q=0.9", VENDOR_YAML_CT),
        ("*/*; q=0.5", YAML_CT),
    ],
)
def test_accept_params_do_not_leak_into_content_type(accept, content_type):
    """DRF sets Content-Type from the chosen renderer's media type, not from
    the matched Accept token: `q` (and any other Accept parameter) is dropped.
    """
    r = sc.negotiate(sc.SCHEMA, accept)
    assert r.status_code == 200
    assert r.headers["content-type"] == content_type
