"""D-04 data-plane dual-engine check (PIDASHCONV-138).

Pure unit suite, no server and no database: every stored prompt section
body renders through the Jinja2 sandbox here, while the minijinja half
lives in ``pidash-services`` (``prompting::renderer::tests``). The two
halves share the ``EMPTY_CONTEXT_CLEAN`` agreement table — update both
sides together when a section body changes.

Also locks the fixture meaning on the Python side (FIX-render goldens
reproduce live) and the data-mirror integrity (``rust-api`` mirrors are
byte-identical to the Django sources; normalized bodies match the
fixture ``body_sha256`` values).

The Django side defaults to this checkout's ``apps/api``. When the
checkout's Django is older than the port (rust-dev's ``apps/api`` is
frozen while drift issues port from ``origin/main``), point
``PIDASH_DJANGO_PROMPTING_DIR`` at the Django-best prompting tree —
a worktree of ``origin/main`` at or after the fixtures' ``ported_from`` —
so the comparison runs against the Django the port follows.
"""

import hashlib
import json
import os
from pathlib import Path

import pytest
from jinja2 import StrictUndefined
from jinja2.sandbox import SandboxedEnvironment

REPO = Path(__file__).resolve().parents[3]
PROMPTING_DIR = Path(
    os.environ.get(
        "PIDASH_DJANGO_PROMPTING_DIR", REPO / "apps/api/pi_dash/prompting"
    )
)
SECTIONS_DIR = PROMPTING_DIR / "sections"
WORK_TYPES_DIR = PROMPTING_DIR / "work_types"
RECIPES_PATH = PROMPTING_DIR / "recipes.py"
MIRROR_DIR = REPO / "rust-api/crates/services/src/prompting/sections"
WORK_TYPES_MIRROR_DIR = REPO / "rust-api/crates/services/src/prompting/work_types"
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
# Work-type sections (PDASHOSS01-234) that render clean under an empty
# context, by namespaced key. The minijinja half folds them into its single
# key-sorted ``EMPTY_CONTEXT_CLEAN`` list.
EMPTY_CONTEXT_CLEAN_WORK_TYPES = [
    "general.execute",
    "general.review",
    "general.test",
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


def work_type_section_files():
    """``(namespaced key, path)`` for every work-type slot file (the
    ``work_type.md`` metadata files are not sections)."""
    return [
        (f"{path.parent.name}.{path.stem}", path)
        for path in sorted(WORK_TYPES_DIR.glob("*/*.md"))
        if path.name != "work_type.md"
    ]


def section_path(key: str) -> Path:
    """Source file of a registry section: core ``sections/<key>.md`` or a
    work-type ``work_types/<type>/<slot>.md`` for namespaced keys."""
    if "." in key:
        work_type, slot = key.split(".", 1)
        return WORK_TYPES_DIR / work_type / f"{slot}.md"
    return SECTIONS_DIR / f"{key}.md"


def test_mirrors_are_byte_identical_to_sources():
    sources = sorted(SECTIONS_DIR.glob("*.md"))
    assert len(sources) == 38
    for source in sources:
        mirror = MIRROR_DIR / source.name
        assert mirror.is_file(), f"mirror missing for {source.name}"
        assert (
            mirror.read_bytes() == source.read_bytes()
        ), f"mirror diverged for {source.name}"
    assert sorted(path.name for path in MIRROR_DIR.glob("*.md")) == [
        source.name for source in sources
    ], "mirror carries a section the Django tree no longer has"


def test_work_type_mirrors_are_byte_identical_to_sources():
    """Every ``work_types/<type>/*.md`` (slot sections and the
    ``work_type.md`` metadata) is mirrored byte for byte, with no extras."""
    sources = sorted(WORK_TYPES_DIR.glob("*/*.md"))
    assert len(sources) == 9
    relative = [source.relative_to(WORK_TYPES_DIR) for source in sources]
    for rel, source in zip(relative, sources):
        mirror = WORK_TYPES_MIRROR_DIR / rel
        assert mirror.is_file(), f"mirror missing for {rel}"
        assert mirror.read_bytes() == source.read_bytes(), f"mirror diverged for {rel}"
    assert sorted(
        path.relative_to(WORK_TYPES_MIRROR_DIR)
        for path in WORK_TYPES_MIRROR_DIR.glob("*/*.md")
    ) == relative


def test_fixture_registry_matches_sources():
    fixture = json.loads((FIXTURES_DIR / "FIX-registry.json").read_text())
    assert fixture["bugs"] == []
    entries = fixture["data"]["registry"]["sections"]
    assert fixture["data"]["registry"]["section_count"] == 45 == len(entries)
    for entry in entries:
        body = section_body(section_path(entry["key"]))
        assert len(body) == entry["body_len"], entry["key"]
        assert hashlib.sha256(body.encode()).hexdigest() == entry["body_sha256"], entry[
            "key"
        ]


def test_all_sections_parse(env):
    for path in sorted(SECTIONS_DIR.glob("*.md")):
        env.parse(section_body(path))  # raises on invalid syntax
    for _key, path in work_type_section_files():
        env.parse(section_body(path))


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
    clean = []
    for key, path in work_type_section_files():
        try:
            env.from_string(section_body(path)).render()
        except Exception:
            continue
        clean.append(key)
    assert clean == EMPTY_CONTEXT_CLEAN_WORK_TYPES


def budget_block(body: str) -> str:
    """Lines from ``### Budget`` to end: self-contained (balanced if/endif)."""
    lines = body.split("\n")
    return "\n".join(lines[lines.index("### Budget") :])


def tick_case(waited, allowance=None):
    """A ``_tick_context``-shaped context with the waited/allowance arms set."""
    return {
        "tick": {
            "count": 3,
            "cap": 20,
            "remaining": 17,
            "waited": waited,
            "wait_allowance": allowance,
            "spent": False,
            "clock_live": True,
            "interval_seconds": 3600,
            "interval_human": "1 hour",
        },
        "issue": {"identifier": "TP-12"},
    }


def test_tick_sentence_waited_counts(env):
    """Jinja2 half of the PIDASHCONV-1073 golden: the "Runs used" sentence.

    The minijinja half asserting the same literals is
    ``prompting::renderer::tests::tick_sentence_matches_jinja2_for_waited_counts``;
    update both sides together when the section body changes. With
    ``spent=False`` the pinned lines are identical across Django trees (the
    only tree difference sits inside the unrendered spent arm), so this
    passes with or without ``PIDASH_DJANGO_PROMPTING_DIR``.
    """
    block = budget_block(section_body(SECTIONS_DIR / "task-lifecycle.md"))
    template = env.from_string(block)
    stem = (
        "Runs used on this issue: **3 of 20** (17 remaining). This is one pool "
        "for the life of the issue, spent in any stage by runs the ticking system "
        "starts (timer ticks, and the run that follows a state move you make). "
        "Runs a human starts are free."
    )
    tail = (
        "each of those raised the cap by one, so they are counted above "
        "and cost no net budget."
    )
    cases = {
        0: stem,
        1: f"{stem} You have ended 1 run by waiting on a blocker; {tail}",
        2: f"{stem} You have ended 2 runs by waiting on a blocker; {tail}",
    }
    for waited, want in cases.items():
        out = template.render(**tick_case(waited))
        assert out.split("\n")[2] == want


def test_tick_sentence_wait_allowance(env):
    """Jinja2 half of the allowance arm (``== 0`` must hold for a present 0).

    minijinja half:
    ``prompting::renderer::tests::wait_allowance_arm_matches_jinja2``.
    """
    block = budget_block(section_body(SECTIONS_DIR / "task-lifecycle.md"))
    template = env.from_string(block)
    stem = (
        "When this issue has open blockers, they are information, not a gate \u2014 "
        "read them and decide for yourself whether the work can proceed "
        '(see "Work item relationships"). If it can, proceed and record the '
        "assumption. If it cannot, write down which blocker and why, call "
        "`pidash issue wait TP-12`, and yield `waiting_on_external`: the wait buys "
        "back this run's tick, so it costs no budget, and the next tick asks you "
        "again. Nothing watches the blocker for you \u2014 a blocker closing starts no "
        "run, and no one reads your workpad \u2014 so each tick you re-read and re-decide."
    )
    tail = (
        " While the issue stays in its current state, Pi Dash re-invokes the agent "
        "about every 1 hour."
    )
    spent_clause = (
        " **Your wait allowance is spent**: further waiting comes out of the pool "
        "above, so if you are still blocked, tell the human in a comment instead "
        "of waiting again."
    )
    cases = {
        0: f"{stem}{spent_clause}{tail}",
        2: f"{stem}{tail}",
    }
    for allowance, want in cases.items():
        out = template.render(**tick_case(0, allowance=allowance))
        assert out.split("\n")[4] == want


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

    source = RECIPES_PATH.read_text()
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

    # `SLOT_X = Slot("x")` (PDASHOSS01-234): work-type slots, encoded in
    # the fixture as `{"slot": "x"}`.
    slots = {
        node.targets[0].id: node.value.args[0].value
        for node in module.body
        if isinstance(node, ast.Assign)
        and len(node.targets) == 1
        and isinstance(node.targets[0], ast.Name)
        and isinstance(node.value, ast.Call)
        and isinstance(node.value.func, ast.Name)
        and node.value.func.id == "Slot"
    }

    class Resolve(ast.NodeTransformer):
        def visit_Name(self, node):
            if node.id in slots:
                return ast.copy_location(
                    ast.Dict(
                        keys=[ast.Constant(value="slot")],
                        values=[ast.Constant(value=slots[node.id])],
                    ),
                    node,
                )
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


DJANGO_DEFAULT_LIMITS = {
    "model_requests": 25,
    "tool_calls": 20,
    "writes": 3,
    "input_tokens": 144000,
    "output_tokens": 16000,
    "total_tokens": 160000,
    "wall_seconds": 285,
}


def test_limits_line_dict_repr(env):
    """PIDASHCONV-1072: the Limits line renders the Python dict repr.

    Jinja2 half of the dual-engine agreement (the minijinja half lives in
    ``prompting::renderer::tests``); both pin the same bytes, and the
    full-section case below pins the created prompt text end to end.
    """
    assert (
        env.from_string("Limits: {{ limits }}.").render(limits=DJANGO_DEFAULT_LIMITS)
        == "Limits: {'model_requests': 25, 'tool_calls': 20, 'writes': 3, "
        "'input_tokens': 144000, 'output_tokens': 16000, "
        "'total_tokens': 160000, 'wall_seconds': 285}."
    )
    assert (
        env.from_string("{{ limits.writes | default(0) }}").render(
            limits=DJANGO_DEFAULT_LIMITS
        )
        == "3"
    )


def test_cloud_capabilities_section_golden(env):
    """PIDASHCONV-1072: the created cloud-capabilities text, pinned."""
    body = section_body(SECTIONS_DIR / "cloud-capabilities.md")
    assert (
        env.from_string(body).render(
            available_tools=["pidash_get_current_issue"],
            unavailable_capabilities=["filesystem", "shell", "worktree"],
            extra_toolsets=False,
            limits=DJANGO_DEFAULT_LIMITS,
        )
        == "## Capabilities\n"
        "\n"
        "Available tools:\n"
        "- `pidash_get_current_issue`\n"
        "\n"
        "Unavailable capabilities: filesystem, shell, worktree.\n"
        "Limits: {'model_requests': 25, 'tool_calls': 20, 'writes': 3, "
        "'input_tokens': 144000, 'output_tokens': 16000, "
        "'total_tokens': 160000, 'wall_seconds': 285}.\n"
        "\n"
        "You do not have a local filesystem, shell, worktree, repository "
        "checkout, or Pi Dash CLI. Do not invent or simulate them.\n"
    )
