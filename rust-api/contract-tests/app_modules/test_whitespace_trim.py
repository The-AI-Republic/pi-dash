"""Whitespace trimming for module write fields (NEWPIDASH-213).

Same DRF ModelSerializer trim_whitespace=True pattern as cycles:
"   " -> 400 blank for name, padded names stored trimmed, length after strip.
Status choice field also trims (CharField choices).
"""

import pytest

from .conftest import module_url, modules_url

pytestmark = pytest.mark.contract


def test_post_whitespace_only_name_rejected(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "   "},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"name": ["This field may not be blank."]}


def test_post_padded_name_is_trimmed(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "  Alpha  "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "Alpha", r.json()["name"]


def test_post_name_length_checked_after_strip(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "x" * 253 + "   "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["name"] == "x" * 253

    r2 = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "x" * 256},
    )
    assert r2.status_code == 400, r2.text[:500]
    assert r2.json() == {"name": ["Ensure this field has no more than 255 characters."]}


def test_patch_whitespace_only_name_rejected(api, seed, tenant_a, project_a, auth_a, module_a):
    r = api.patch(
        module_url(tenant_a, project_a, module_a),
        headers=auth_a,
        json={"name": "   "},
    )
    assert r.status_code == 400, r.text[:500]
    assert r.json() == {"name": ["This field may not be blank."]}


def test_patch_padded_name_is_trimmed(api, tenant_a, project_a, auth_a, module_a):
    r = api.patch(
        module_url(tenant_a, project_a, module_a),
        headers=auth_a,
        json={"name": "  Patched  "},
    )
    assert r.status_code == 200, r.text[:500]
    assert r.json()["name"] == "Patched"


def test_post_description_is_trimmed(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "DescTrim", "description": "  hello  "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["description"] == "hello"


def test_post_external_fields_trimmed(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={
            "name": "ExtTrim",
            "external_source": "  jira  ",
            "external_id": "  EXT-1  ",
        },
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["external_source"] == "jira"
    assert r.json()["external_id"] == "EXT-1"


def test_post_status_trimmed(api, tenant_a, project_a, auth_a):
    r = api.post(
        modules_url(tenant_a, project_a),
        headers=auth_a,
        json={"name": "StatusTrim", "status": "  planned  "},
    )
    assert r.status_code == 201, r.text[:500]
    assert r.json()["status"] == "planned"
