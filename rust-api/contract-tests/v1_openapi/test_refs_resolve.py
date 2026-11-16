"""Self-consistency of the served OpenAPI document: every ``$ref`` resolves.

Django's ``drf-spectacular`` ``SchemaGenerator.get_schema``
(0.28.0 ``generators.py:282-293``) writes ``components=self.registry.build(...)``
into the document it serves, so every component an operation references is
present and every ``$ref`` resolves. The served document is therefore a valid
OpenAPI 3.0.3 spec that swagger-ui renders and codegen accepts.

The Rust port drops the optional per-operation keys it did not capture
(D-23 minimal disposition) but must still serve a *self-consistent*
document: a ``$ref`` pointing at a name ``components.schemas`` does not
contain makes the document unparseable as OpenAPI at all. PIDASHCONV-1208
fixed the port to emit ``components.schemas`` from Django's document.
"""

import pytest

from . import client as sc  # noqa: F401  (keeps the domain client import stable)


def _iter_refs(node):
    """Yield every ``$ref`` string in the document, depth-first."""
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                yield value
            else:
                yield from _iter_refs(value)
    elif isinstance(node, list):
        for item in node:
            yield from _iter_refs(item)


def _resolve(doc, ref):
    """Resolve a ``#/a/b`` JSON pointer inside ``doc``; ``None`` when it dangles."""
    if not ref.startswith("#/"):
        return None
    node = doc
    for raw in ref[2:].split("/"):
        token = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(node, dict) and token in node:
            node = node[token]
        elif isinstance(node, list) and token.isdigit() and int(token) < len(node):
            node = node[int(token)]
        else:
            return None
    return node


@pytest.mark.contract
def test_document_carries_refs(doc):
    """Guard against a vacuous pass: the served document must carry ``$ref``s."""
    refs = list(_iter_refs(doc))
    assert refs, "served document carries no $ref at all"


@pytest.mark.contract
def test_every_ref_resolves_inside_document(doc):
    """No ``$ref`` may point at a target the served document does not contain."""
    refs = list(_iter_refs(doc))
    dangling = sorted({r for r in refs if _resolve(doc, r) is None})
    assert dangling == [], (
        f"{len(dangling)} dangling $ref target(s), first 10: {dangling[:10]}"
    )


@pytest.mark.contract
def test_referenced_schemas_are_emitted(doc):
    """Every schema name an operation references is present in ``components.schemas``."""
    schemas = doc.get("components", {}).get("schemas", {})
    assert schemas, "components.schemas is missing or empty"
    names = {r.rsplit("/", 1)[-1] for r in _iter_refs(doc) if "/schemas/" in r}
    assert names, "no schema references found"
    assert names <= set(schemas), f"missing schemas: {sorted(names - set(schemas))}"
