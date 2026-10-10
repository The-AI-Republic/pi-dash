# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier module write flows (PIDASHCONV-823).

Built from the ``app_modules`` contract suite: test_modules.py (CRUD,
duplicate/date validation, sort order), test_module_issues.py
(attach/detach both directions), test_module_links.py, test_archive.py,
test_favorites_properties.py.
"""

from ..session import Request, Session

_B = "/api/workspaces/{ws}/projects/{project}"
_MODULES = _B + "/modules/"
_MODULE = _B + "/modules/{module}/"


def _location_pin(url: str):
    """Exact-header pin for PIDASHCONV-873 (one per link POST).

    DRF's create sets ``Location`` to the serializer's ``url`` field
    (the link target); Rust sends no header. Any change on either
    side fails loudly.
    """
    return [("location", "PIDASHCONV-873", url, None)]

SESSIONS = (
    Session(
        name="app_modules_lifecycle",
        domain="app_modules",
        source="app_modules/test_modules.py + test_module_issues.py + test_module_links.py + test_archive.py + test_favorites_properties.py",
        auth_users=("admin",),
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        requests=(
            Request(
                "POST", _MODULES,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/",
                json={"name": "Second module"},
                auth="admin", capture={"module": "id"}, expect_status=201,
            ),
            Request(
                "POST", _MODULES,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/",
                json={"name": "Second module"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _MODULES,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/",
                json={"name": "Bad dates", "start_date": "2026-10-01", "target_date": "2026-09-01"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _MODULE,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
                json={"name": "Renamed module"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _MODULE,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
                json={"name": "Put module", "description": "via put"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Module issue"},
                auth="admin", capture={"missue": "id"}, expect_status=201,
            ),
            Request(
                "POST", _MODULE + "issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/issues/",
                json={"issues": ["{missue}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _MODULE + "issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/issues/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _MODULE + "issues/{missue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/issues/{issue_id}/",
                json={"name": "x"},
                auth="admin", expect_status=500,
            ),
            Request(
                "PUT", _MODULE + "issues/{missue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/issues/{issue_id}/",
                json={"name": "x"},
                auth="admin", expect_status=500,
            ),
            Request(
                "DELETE", _MODULE + "issues/{missue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/issues/{issue_id}/",
                auth="admin", expect_status=204,
            ),
            # A second issue keeps one bridge per (issue, module) pair:
            # re-attaching {missue} here would build the two-bridge
            # state that 500s Rust module DELETE (PIDASHCONV-875).
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Module issue 2"},
                auth="admin", capture={"missue2": "id"}, expect_status=201,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/issues/{missue2}/modules/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/",
                json={"modules": ["{module}"], "removed_modules": []},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/issues/{missue2}/modules/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/",
                json={"modules": [], "removed_modules": ["{module}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _MODULE + "module-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/",
                json={"title": "Specs", "url": "https://example.com/specs"},
                auth="admin", capture={"mlink": "id"}, expect_status=201,
                known_headers=_location_pin("https://example.com/specs"),
            ),
            Request(
                "POST", _MODULE + "module-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/",
                json={"title": "Docs", "url": "example.com/docs"},
                auth="admin", capture={"mlink2": "id"}, expect_status=201,
                known_headers=_location_pin("http://example.com/docs"),
            ),
            Request(
                "POST", _MODULE + "module-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/",
                json={"title": "Bad", "url": "not a url at all !!"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _MODULE + "module-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/",
                json={"title": "Dup", "url": "http://example.com/dup"},
                auth="admin", capture={"mlink3": "id"}, expect_status=201,
                known_headers=_location_pin("http://example.com/dup"),
            ),
            Request(
                "POST", _MODULE + "module-links/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/",
                json={"title": "Dup again", "url": "http://example.com/dup"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _MODULE + "module-links/{mlink}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/{link_id}/",
                json={"title": "Renamed"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PUT", _MODULE + "module-links/{mlink}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/{link_id}/",
                json={"title": "Replaced", "url": "http://example.com/replaced"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _MODULE + "module-links/{mlink}/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/module-links/{link_id}/",
                auth="admin", expect_status=204,
                # PIDASHCONV-874: Rust link_destroy skips the mixin
                # soft_delete enqueue entirely.
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-874",
                },
            ),
            Request(
                "POST", _MODULE + "archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/",
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _MODULE,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
                json={"status": "completed"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _MODULE + "archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _MODULE,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
                json={"name": "Nope"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _MODULE + "archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _B + "/user-favorite-modules/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/",
                json={"module": "{module}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _B + "/user-favorite-modules/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/",
                json={"module": "{module}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _B + "/user-favorite-modules/{module}/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/user-favorite-modules/{module}/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _MODULE + "user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/user-properties/",
                auth="admin", capture={"mprops": "id"}, expect_status=200,
            ),
            Request(
                "PATCH", _MODULE + "user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/user-properties/",
                json={"filters": {"priority": "high"}},
                auth="admin", expect_status=201,
            ),
            Request(
                "DELETE", _MODULE,
                route="/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
                auth="admin", expect_status=204,
                # PIDASHCONV-874: module_destroy also skips the mixin
                # soft_delete enqueue (second instance of that issue).
                known_absent_jobs={
                    "pi_dash.bgtasks.deletion_task.soft_delete_related_objects": "PIDASHCONV-874",
                },
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/projects/{project}/issues/{missue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/projects/{project}/issues/{missue2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
