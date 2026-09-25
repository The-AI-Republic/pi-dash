"""Shape of the OpenAPI document itself: ``GET /api/schema/``.

Pins ``SPECTACULAR_SETTINGS`` (title, version, servers, tags, auth scheme)
and the two processing hooks: the preprocess filter (v1 paths only, no
``PUT``, no ``server`` paths) and the project-id dual-form postprocessor.
"""

import pytest
import yaml

from . import client as sc

EXPECTED_INFO = {
    "title": "The Pi Dash REST API",
    "version": "0.0.1",
    "description": (
        "The Pi Dash REST API\n\n"
        "Visit our quick start guide and full API documentation at "
        "[github.com/The-AI-Republic/pi-dash](https://github.com/The-AI-Republic/pi-dash#readme)."
    ),
    "contact": {
        "name": "Pi Dash",
        "url": "https://airepublic.com",
        "email": "support@airepublic.com",
    },
    "license": {
        "name": "GNU AGPLv3",
        "url": "https://github.com/The-AI-Republic/pi-dash/blob/preview/LICENSE.txt",
    },
}

EXPECTED_SERVERS = [
    {"url": "http://localhost:8000", "description": "Local"},
    {"url": "https://airepublic.com/api", "description": "Production"},
]

EXPECTED_TAGS = [
    "Assets",
    "Cycles",
    "Intake",
    "Labels",
    "Members",
    "Modules",
    "Projects",
    "States",
    "Users",
    "Work Item Activity",
    "Work Item Attachments",
    "Work Item Comments",
    "Work Item Links",
    "Work Items",
]

ALLOWED_METHODS = {"get", "post", "patch", "delete"}


@pytest.mark.contract
def test_doc_default_is_yaml():
    r = sc.get_patient(sc.SCHEMA)
    assert r.status_code == 200
    assert r.headers["content-type"].startswith("application/vnd.oai.openapi")
    assert "+json" not in r.headers["content-type"]
    doc = yaml.safe_load(r.text)
    assert doc["openapi"].startswith("3.0.")


@pytest.mark.contract
def test_doc_json_format_matches_yaml():
    """``?format=json`` parses to the same document as the default YAML."""
    yml = yaml.safe_load(sc.get_patient(sc.SCHEMA).text)
    r = sc.get_patient(sc.SCHEMA, params={"format": "json"})
    assert r.status_code == 200
    assert "application/vnd.oai.openapi+json" in r.headers["content-type"]
    assert r.json() == yml


@pytest.mark.contract
def test_openapi_version(doc):
    assert doc["openapi"] == "3.0.3"


@pytest.mark.contract
def test_info_block(doc):
    assert doc["info"] == EXPECTED_INFO


@pytest.mark.contract
def test_servers(doc):
    assert doc["servers"] == EXPECTED_SERVERS


@pytest.mark.contract
def test_tags(doc):
    assert [t["name"] for t in doc["tags"]] == EXPECTED_TAGS


@pytest.mark.contract
def test_api_key_security_scheme(doc):
    """``auth.py``'s ``APIKeyAuthenticationExtension`` is the documented scheme."""
    scheme = doc["components"]["securitySchemes"]["ApiKeyAuthentication"]
    assert scheme["type"] == "apiKey"
    assert scheme["in"] == "header"
    assert scheme["name"] == "X-API-Key"


@pytest.mark.contract
def test_paths_cover_only_api_v1(doc):
    """Preprocess hook: every documented path lives under ``/api/v1/``."""
    paths = doc["paths"]
    assert len(paths) >= 100
    assert [p for p in paths if not p.startswith("/api/v1/")] == []


@pytest.mark.contract
def test_no_put_operations(doc):
    """Preprocess hook drops ``PUT`` even though the viewsets serve it."""
    puts = [(p, m) for p, ops in doc["paths"].items() for m in ops if m.lower() == "put"]
    assert puts == []


@pytest.mark.contract
def test_no_server_paths(doc):
    """Preprocess hook drops installer paths containing ``server``."""
    assert [p for p in doc["paths"] if "server" in p.lower()] == []


@pytest.mark.contract
def test_only_read_and_write_methods(doc):
    methods = {m for ops in doc["paths"].values() for m in ops}
    assert methods <= ALLOWED_METHODS


@pytest.mark.contract
def test_operations_have_ids_and_responses(doc):
    n = 0
    for path, ops in doc["paths"].items():
        for method, op in ops.items():
            assert isinstance(op, dict), (path, method)
            assert op.get("operationId"), (path, method)
            assert op.get("responses"), (path, method)
            n += 1
    assert n >= 100


@pytest.mark.contract
def test_project_id_dual_form(doc):
    """Postprocess hook: ``{project_id}``/project ``{pk}`` params accept
    UUID or workspace-scoped slug, with both examples."""
    seen = 0
    for path, ops in doc["paths"].items():
        if "{project_id}" not in path and "/projects/{pk}/" not in path:
            continue
        for method, op in ops.items():
            if not isinstance(op, dict):
                continue
            for param in op.get("parameters", []) or []:
                if param.get("in") != "path":
                    continue
                name = param.get("name")
                if name == "project_id" or (name == "pk" and "/projects/{pk}/" in path):
                    seen += 1
                    assert "UUID" in (param.get("description") or ""), (path, method)
                    assert set(param.get("examples", {})) == {"uuid", "slug"}, (path, method)
                    assert param["schema"]["type"] == "string"
                    assert "format" not in param["schema"]
    assert seen > 0


@pytest.mark.contract
def test_anonymous_read(doc):
    """The document is public by design (``AllowAny``): no credentials needed."""
    assert doc["info"]["title"] == "The Pi Dash REST API"
