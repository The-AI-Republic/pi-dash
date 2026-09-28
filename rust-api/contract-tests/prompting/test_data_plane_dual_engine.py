"""D-04 data-plane dual-engine check (PIDASHCONV-138).

Pure unit suite, no server and no database: every stored prompt section
body renders through the Jinja2 sandbox here, while the minijinja half
lives in ``pidash-services`` (``prompting::renderer::tests``). The two
halves share the ``EMPTY_CONTEXT_CLEAN`` agreement table — update both
sides together when a section body changes.

Also locks the fixture meaning on the Python side (FIX-render goldens
reproduce live) and the data-mirror integrity (``rust-api`` mirrors are
byte-identical to ``apps/api`` sources; normalized bodies match the
fixture ``body_sha256`` values).
"""

import hashlib
import json
from pathlib import Path

import pytest
from jinja2 import StrictUndefined
from jinja2.sandbox import SandboxedEnvironment

REPO = Path(__file__).resolve().parents[3]
SECTIONS_DIR = REPO / "apps/api/pi_dash/prompting/sections"
MIRROR_DIR = REPO / "rust-api/crates/services/src/prompting/sections"
FIXTURES_DIR = REPO / "rust-api/fixtures/prompting"

# Sections that render clean under an empty context in the Jinja2 sandbox
# (verified against Jinja2 3.1.6; the minijinja half asserts the same set
# in ``prompting::renderer::tests::all_stored_sections_render_like_jinja2``).
EMPTY_CONTEXT_CLEAN = [
    "autonomy",
    "cloud-direct-task",
    "cloud-ending",
    "cloud-execution-loop",
    "cloud-review-loop",
    "cloud-scheduler-intro",
    "cloud-scheduler-loop",
    "cloud-test-loop",
    "default-posture",
    "guardrails",
    "review-cycle",
    "scheduler-ending",
    "test-cycle",
]


@pytest.fixture(scope="module")
def env():
    return SandboxedEnvironment(
        autoescape=False,
        trim_blocks=False,
        lstrip_blocks=False,
        undefined=StrictUndefined,
    )


def section_body(path: Path) -> str:
    """The registry default body (``registry.py:146`` normalization)."""
    _, _, body = path.read_text(encoding="utf-8").split("---", 2)
    return body.lstrip("\n").rstrip("\n") + "\n"


def test_mirrors_are_byte_identical_to_sources():
    sources = sorted(SECTIONS_DIR.glob("*.md"))
    assert len(sources) == 38
    for source in sources:
        mirror = MIRROR_DIR / source.name
        assert mirror.is_file(), f"mirror missing for {source.name}"
        assert (
            mirror.read_bytes() == source.read_bytes()
        ), f"mirror diverged for {source.name}"


def test_fixture_registry_matches_sources():
    fixture = json.loads((FIXTURES_DIR / "FIX-registry.json").read_text())
    assert fixture["bugs"] == []
    entries = fixture["data"]["registry"]["sections"]
    assert fixture["data"]["registry"]["section_count"] == 38 == len(entries)
    for entry in entries:
        body = section_body(SECTIONS_DIR / f"{entry['key']}.md")
        assert len(body) == entry["body_len"], entry["key"]
        assert hashlib.sha256(body.encode()).hexdigest() == entry["body_sha256"], entry[
            "key"
        ]


def test_all_sections_parse(env):
    for path in sorted(SECTIONS_DIR.glob("*.md")):
        env.parse(section_body(path))  # raises on invalid syntax


def test_empty_context_render_agreement(env):
    clean = []
    for path in sorted(SECTIONS_DIR.glob("*.md")):
        body = section_body(path)
        try:
            env.from_string(body).render()
        except Exception:
            continue
        clean.append(path.stem)
    assert clean == EMPTY_CONTEXT_CLEAN


def test_render_goldens_reproduce_live(env):
    """FIX-render success outputs and error classes reproduce in Jinja2."""
    fixture = json.loads((FIXTURES_DIR / "FIX-render.json").read_text())
    assert fixture["bugs"] == []
    renders = fixture["data"]["renderer"]["renders"]
    assert env.from_string("Hello {{ name }}!").render(name="Ada") == renders["var"]["text"]
    assert (
        env.from_string("{% for x in items %}{{ x }};{% endfor %}")
        .render(items=["a", "b"])
        == renders["for_loop"]["text"]
    )
    assert (
        env.from_string('{{ items|join(", ") }}').render(items=["a", "b"])
        == renders["filter_join"]["text"]
    )
    assert (
        env.from_string("{% if flag %}yes{% endif %}").render(flag=True)
        == renders["if_true"]["text"]
    )
    with pytest.raises(Exception):
        env.from_string("{% if missing %}yes{% endif %}").render()
    assert renders["if_missing_var"]["ok"] is False
    with pytest.raises(Exception):
        env.from_string('{{ "".__class__ }}').render()
    assert renders["sandbox_blocked"]["ok"] is False
    syntax = fixture["data"]["renderer"]["syntax"]
    env.parse("Hello {{ name }}!")
    assert syntax["ok_simple"]["ok"] is True
    with pytest.raises(Exception):
        env.parse("{% if %}")
    assert syntax["bad_tag"]["ok"] is False


def test_recipe_goldens_match_sources():
    """FIX-recipes tables reproduce the RECIPES/CLOUD_RECIPES literals.

    Parsed with ``ast`` (no Django import: ``recipes.py`` pulls in
    ``pi_dash.core.agent_execution`` at call time for ``recipe_for``).
    """
    import ast

    source = (REPO / "apps/api/pi_dash/prompting/recipes.py").read_text()
    module = ast.parse(source)
    # Module-level `NAME = "..."` string constants (the KIND_* names the
    # recipe tables reference).
    consts = {
        node.targets[0].id: node.value.value
        for node in module.body
        if isinstance(node, ast.Assign)
        and len(node.targets) == 1
        and isinstance(node.targets[0], ast.Name)
        and isinstance(node.value, ast.Constant)
    }

    class Resolve(ast.NodeTransformer):
        def visit_Name(self, node):
            return ast.copy_location(ast.Constant(value=consts[node.id]), node)

    tables = {}
    for node in module.body:
        if (
            isinstance(node, ast.AnnAssign)
            and isinstance(node.target, ast.Name)
            and node.target.id in ("RECIPES", "CLOUD_RECIPES")
        ):
            tables[node.target.id] = ast.literal_eval(Resolve().visit(node.value))
    fixture = json.loads((FIXTURES_DIR / "FIX-recipes.json").read_text())
    assert fixture["bugs"] == []
    data = fixture["data"]["recipes"]
    assert {k: list(v) for k, v in tables["RECIPES"].items()} == data["local_recipes"]
    assert {k: list(v) for k, v in tables["CLOUD_RECIPES"].items()} == data[
        "cloud_recipes"
    ]
    assert list(tables["RECIPES"]) == list(data["all_kinds"])
