"""Contract tests: GitLab bind URLs with ?query/#fragment (PIDASHCONV-1462).

Django parses the repo URL with ``urlparse(candidate).path``
(``adapters/gitlab.py:236-245``), so a browser-pasted URL such as
``https://gitlab.com/group/repo?tab=readme`` resolves to ``group/repo``.
The query string and fragment are never part of the repository path.

These tests stay offline: with no provider account seeded, a URL that
parses reaches account resolution (409), while a URL whose path has no
``namespace/name`` is rejected as unsupported (400). The 400 vectors
fail on the buggy Rust parser (it keeps ``?``/``#`` in the path, so the
URL wrongly parses and answers 409) and pass after the fix.
"""

from __future__ import annotations

import pytest

from .conftest import proj_url

pytestmark = pytest.mark.contract

UNSUPPORTED = {"error": "A supported GitHub or GitLab repository URL is required"}
NO_ACCOUNT = {"error": "Connect a provider account before binding this repository"}


def test_bind_gitlab_query_url_parses(admin, world):
    """A query-string GitLab URL parses and reaches account resolution."""
    response = admin.post(
        proj_url(world, "repository", "bind"),
        json={"repo_url": "https://gitlab.com/group/repo?tab=readme"},
    )
    assert response.status_code == 409
    assert response.json() == NO_ACCOUNT


def test_bind_gitlab_fragment_url_parses(admin, world):
    """A fragment GitLab URL parses and reaches account resolution."""
    response = admin.post(
        proj_url(world, "repository", "bind"),
        json={"repo_url": "https://gitlab.com/group/repo#readme"},
    )
    assert response.status_code == 409
    assert response.json() == NO_ACCOUNT


def test_bind_gitlab_query_not_part_of_path(admin, world):
    """A query containing `/` must not create a namespace/name split.

    ``urlparse(...).path`` is ``/onlygroup`` (no ``/``), so Django rejects
    the URL as unsupported. The buggy Rust parser kept ``?tab=/extra`` in
    the path, split ``onlygroup?tab=`` / ``extra``, and answered 409.
    """
    response = admin.post(
        proj_url(world, "repository", "bind"),
        json={"repo_url": "https://gitlab.com/onlygroup?tab=/extra"},
    )
    assert response.status_code == 400
    assert response.json() == UNSUPPORTED


def test_bind_gitlab_fragment_not_part_of_path(admin, world):
    """A fragment containing `/` must not create a namespace/name split."""
    response = admin.post(
        proj_url(world, "repository", "bind"),
        json={"repo_url": "https://gitlab.com/onlygroup#/extra"},
    )
    assert response.status_code == 400
    assert response.json() == UNSUPPORTED
