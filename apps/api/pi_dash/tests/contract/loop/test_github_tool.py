# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""get_pull_request_status — URL parsing, status mapping, budget, never raises."""

from __future__ import annotations

import pytest

from pi_dash.assistant.tools import github
from pi_dash.tests.contract.assistant.conftest import fake_ctx, make_deps

pytestmark = pytest.mark.django_db


class _Resp:
    def __init__(self, status_code, payload=None):
        self.status_code = status_code
        self._payload = payload or {}

    def json(self):
        return self._payload


class _Client:
    def __init__(self, resp):
        self._resp = resp

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def get(self, url, headers=None):
        return self._resp


def _patch_httpx(mocker, resp):
    mocker.patch("httpx.Client", return_value=_Client(resp))


def _ctx(world):
    return fake_ctx(make_deps(world.member, world.ws, 15))


def test_merged(world, mocker):
    _patch_httpx(mocker, _Resp(200, {"merged": True, "merged_at": "2026-01-01T00:00:00Z", "title": "Fix"}))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out["state"] == "merged"
    assert out["title"] == "Fix"


def test_open(world, mocker):
    _patch_httpx(mocker, _Resp(200, {"merged": False, "state": "open", "title": "WIP"}))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out["state"] == "open"


def test_closed_unmerged(world, mocker):
    _patch_httpx(mocker, _Resp(200, {"merged": False, "merged_at": None, "state": "closed"}))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out["state"] == "closed"


def test_non_github_url(world, mocker):
    spy = mocker.patch("httpx.Client")
    out = github.get_pull_request_status(_ctx(world), "https://gitlab.com/o/r/merge_requests/1")
    assert out == {"state": "unknown", "reason": "unsupported_url"}
    spy.assert_not_called()  # no network for an unparseable URL


def test_rate_limited(world, mocker):
    _patch_httpx(mocker, _Resp(403))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out == {"state": "unknown", "reason": "rate_limited"}


def test_not_found(world, mocker):
    _patch_httpx(mocker, _Resp(404))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out["reason"] == "not_found"


def test_network_error_never_raises(world, mocker):
    mocker.patch("httpx.Client", side_effect=RuntimeError("boom"))
    out = github.get_pull_request_status(_ctx(world), "https://github.com/o/r/pull/5")
    assert out == {"state": "unknown", "reason": "network_error"}


def test_per_run_budget(world, mocker, settings):
    settings.LOOP_PR_LOOKUPS_PER_RUN = 2
    _patch_httpx(mocker, _Resp(200, {"merged": True, "merged_at": "x"}))
    ctx = _ctx(world)
    assert github.get_pull_request_status(ctx, "https://github.com/o/r/pull/1")["state"] == "merged"
    assert github.get_pull_request_status(ctx, "https://github.com/o/r/pull/2")["state"] == "merged"
    # Third call exceeds the budget.
    out = github.get_pull_request_status(ctx, "https://github.com/o/r/pull/3")
    assert out == {"state": "unknown", "reason": "budget_exhausted"}
