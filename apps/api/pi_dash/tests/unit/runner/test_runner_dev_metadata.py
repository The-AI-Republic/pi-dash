# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.runner.services.session_service import _merge_dev_metadata


@pytest.mark.unit
def test_merge_dev_metadata_updates_working_dir_and_preserves_other_keys():
    current = {"editor": "codex", "working_dir": "/old/path"}

    result = _merge_dev_metadata(current, {"working_dir": "/new/path"})

    assert result == {"editor": "codex", "working_dir": "/new/path"}
    assert current == {"editor": "codex", "working_dir": "/old/path"}


@pytest.mark.unit
def test_merge_dev_metadata_distinguishes_omitted_from_explicit_empty_working_dir():
    current = {"editor": "codex", "working_dir": "/stale/path"}

    assert _merge_dev_metadata(current, {}) == current
    assert _merge_dev_metadata(current, {"working_dir": ""}) == {"editor": "codex"}


@pytest.mark.unit
def test_merge_dev_metadata_recovers_from_non_object_json_and_limits_path_length():
    result = _merge_dev_metadata(["unexpected"], {"working_dir": "x" * 2048})

    assert result == {"working_dir": "x" * 1024}
