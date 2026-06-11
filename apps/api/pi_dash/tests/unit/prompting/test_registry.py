# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Section registry: front-matter parsing, integrity, and validation."""

from __future__ import annotations

import pytest

from pi_dash.prompting import recipes, registry
from pi_dash.prompting.renderer import validate_syntax


@pytest.mark.unit
def test_registry_loaded_with_expected_sections():
    # 13 ported + 2 review + 3 scheduler = 18.
    assert len(registry.REGISTRY) == 18
    for key in ("intro", "pidash-cli", "review-cycle", "scheduler-task"):
        assert key in registry.REGISTRY


@pytest.mark.unit
def test_every_section_has_valid_customizable_flag():
    for section in registry.all_sections():
        assert section.customizable in {
            registry.CUSTOMIZABLE_LOCKED,
            registry.CUSTOMIZABLE_OVERRIDABLE,
        }


@pytest.mark.unit
def test_locked_and_overridable_classification():
    assert registry.get_section("pidash-cli").is_locked
    assert registry.get_section("ending-run").is_locked
    assert registry.get_section("implementation").is_overridable
    assert registry.get_section("review-cycle").is_overridable


@pytest.mark.unit
def test_every_section_body_is_valid_jinja():
    for section in registry.all_sections():
        # Raises PromptSyntaxError on bad syntax — must not raise.
        validate_syntax(section.default_body)


@pytest.mark.unit
def test_every_recipe_key_exists_in_registry():
    for kind, section_keys in recipes.RECIPES.items():
        for key in section_keys:
            assert key in registry.REGISTRY, f"{kind} references missing {key}"


@pytest.mark.unit
def test_section_key_matches_filename_stem():
    for section in registry.all_sections():
        path = registry.SECTIONS_DIR / f"{section.key}.md"
        assert path.exists()


@pytest.mark.unit
def test_get_section_unknown_raises():
    with pytest.raises(registry.PromptRegistryError):
        registry.get_section("does-not-exist")


@pytest.mark.unit
def test_parse_front_matter_rejects_missing_block(tmp_path):
    bad = tmp_path / "bad.md"
    bad.write_text("no front matter here\n", encoding="utf-8")
    with pytest.raises(registry.PromptRegistryError):
        registry._parse_front_matter(bad)


@pytest.mark.unit
def test_parse_front_matter_rejects_unknown_customizable(tmp_path):
    bad = tmp_path / "x.md"
    bad.write_text(
        "---\nkey: x\ntitle: X\ncustomizable: sometimes\n---\nbody\n", encoding="utf-8"
    )
    with pytest.raises(registry.PromptRegistryError):
        registry._parse_front_matter(bad)


@pytest.mark.unit
def test_parse_front_matter_rejects_key_filename_mismatch(tmp_path):
    bad = tmp_path / "wrongname.md"
    bad.write_text(
        "---\nkey: rightname\ntitle: X\ncustomizable: locked\n---\nbody\n",
        encoding="utf-8",
    )
    with pytest.raises(registry.PromptRegistryError):
        registry._parse_front_matter(bad)


@pytest.mark.unit
def test_parse_front_matter_requires_all_fields(tmp_path):
    bad = tmp_path / "y.md"
    bad.write_text("---\nkey: y\n---\nbody\n", encoding="utf-8")
    with pytest.raises(registry.PromptRegistryError):
        registry._parse_front_matter(bad)
