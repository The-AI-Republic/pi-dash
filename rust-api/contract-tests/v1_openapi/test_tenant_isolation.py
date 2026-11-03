"""Tenant isolation: the public document carries no tenant data.

Two fully seeded tenants plus anonymous callers must all receive
byte-identical documents and UI pages, and none of the tenants' secrets
(keys, slugs, emails, ids) may appear anywhere in the served text.

Note the views ignore API keys (session auth only), so keyed requests count
toward the same anonymous throttle budget — every fetch here waits out
``429``s via :func:`client.get_patient`.
"""

import pytest

from . import client as sc


def _doc_bytes(api_key=None) -> bytes:
    r = sc.get_patient(sc.SCHEMA, params={"format": "json"}, api_key=api_key)
    assert r.status_code == 200
    return r.content


@pytest.mark.contract
def test_doc_identical_across_tenants(schema_bytes, tenants):
    a, b = tenants
    assert _doc_bytes(api_key=a.api_key) == schema_bytes
    assert _doc_bytes(api_key=b.api_key) == schema_bytes


@pytest.mark.contract
def test_ui_pages_identical_across_tenants(tenants):
    from .test_ui_pages import normalize

    a, _ = tenants
    for path in (sc.SWAGGER_UI, sc.REDOC):
        keyed = sc.get_patient(path, api_key=a.api_key)
        assert keyed.status_code == 200
        anon = sc.get_patient(path)
        assert anon.status_code == 200
        assert normalize(keyed.text) == normalize(anon.text)


@pytest.mark.contract
def test_doc_leaks_no_tenant_secrets(tenants, schema_bytes):
    a, b = tenants
    swagger = sc.get_patient(sc.SWAGGER_UI)
    assert swagger.status_code == 200
    redoc = sc.get_patient(sc.REDOC)
    assert redoc.status_code == 200
    haystacks = [schema_bytes.decode(), swagger.text, redoc.text]
    secrets = [
        a.api_key,
        b.api_key,
        a.workspace_slug,
        b.workspace_slug,
        a.workspace_id,
        b.workspace_id,
        a.project_id,
        b.project_id,
        a.project_identifier,
        b.project_identifier,
    ]
    for text in haystacks:
        for secret in secrets:
            assert secret and secret not in text, secret[:12]
