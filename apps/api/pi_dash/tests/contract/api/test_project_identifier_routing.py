# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Contract tests proving every project-scoped REST URL accepts either a
project UUID or a workspace-scoped slug (the `identifier` field).

The implementation lives in `Project.resolve` plus a hook in
`BaseAPIView.initial` / `BaseViewSet.initial` that rewrites
`kwargs["project_id"]` (or `kwargs["pk"]` for the project-detail route) to
the canonical UUID *before* DRF runs permission checks. These tests cover
one route per url file under `apps/api/pi_dash/api/urls/` so a future
regression in the rewrite hook is caught fleet-wide rather than only on the
specific route someone happens to be modifying.
"""

import pytest
from rest_framework import status

from pi_dash.db.models import ProjectMember


@pytest.fixture
def project_member(db, project, create_user):
    ProjectMember.objects.get_or_create(
        project=project,
        member=create_user,
        defaults={"role": 20, "is_active": True},
    )
    return project


@pytest.mark.contract
@pytest.mark.django_db
class TestProjectIdentifierRouting:
    """One slug-equivalence smoke per project-scoped url file (9 cases) plus
    explicit miss / mixed-case / cross-workspace cases."""

    def _both(self, project):
        return [str(project.id), project.identifier]

    @pytest.mark.parametrize(
        "path_template",
        [
            # urls/project.py — project detail (the `<str:pk>` route)
            "/api/v1/workspaces/{slug}/projects/{ident}/",
            # urls/work_item.py — work item list under a project
            "/api/v1/workspaces/{slug}/projects/{ident}/work-items/",
            # urls/cycle.py
            "/api/v1/workspaces/{slug}/projects/{ident}/cycles/",
            # urls/module.py
            "/api/v1/workspaces/{slug}/projects/{ident}/modules/",
            # urls/state.py
            "/api/v1/workspaces/{slug}/projects/{ident}/states/",
            # urls/label.py
            "/api/v1/workspaces/{slug}/projects/{ident}/labels/",
            # urls/estimate.py — defined but not registered in
            # `api/urls/__init__.py`; the `<str:project_id>` swap is in place
            # so re-enabling it later already works with slugs.
            # urls/intake.py
            "/api/v1/workspaces/{slug}/projects/{ident}/intake-issues/",
            # urls/member.py
            "/api/v1/workspaces/{slug}/projects/{ident}/members/",
        ],
    )
    def test_route_accepts_uuid_and_slug(
        self, api_key_client, workspace, project_member, path_template
    ):
        # Both forms must succeed (any non-4xx/5xx status). We don't assert
        # body equality because list endpoints' results vary with state and
        # detail endpoints embed the resolved UUID; the contract under test
        # is "URL routing accepts both forms."
        uuid_resp = api_key_client.get(
            path_template.format(slug=workspace.slug, ident=project_member.id)
        )
        slug_resp = api_key_client.get(
            path_template.format(slug=workspace.slug, ident=project_member.identifier)
        )

        assert uuid_resp.status_code < 400, (
            f"UUID form failed: {uuid_resp.status_code} {uuid_resp.content[:200]!r}"
        )
        assert slug_resp.status_code == uuid_resp.status_code, (
            f"slug returned {slug_resp.status_code} but UUID returned {uuid_resp.status_code}; "
            f"slug body: {slug_resp.content[:200]!r}"
        )

    def test_unknown_slug_returns_404(self, api_key_client, workspace, project_member):
        resp = api_key_client.get(
            f"/api/v1/workspaces/{workspace.slug}/projects/ZZZZ/work-items/"
        )
        assert resp.status_code == status.HTTP_404_NOT_FOUND

    def test_mixed_case_slug_resolves(self, api_key_client, workspace, project_member):
        # `Project.save()` uppercases identifier; resolver uses `__iexact` so
        # lowercase input from a careless client still hits.
        resp = api_key_client.get(
            f"/api/v1/workspaces/{workspace.slug}/projects/{project_member.identifier.lower()}/work-items/"
        )
        assert resp.status_code < 400

    def test_slug_does_not_leak_across_workspaces(
        self, db, api_key_client, workspace, project_member, create_user
    ):
        from pi_dash.db.models import Workspace, WorkspaceMember
        from pi_dash.db.models.project import Project

        # Identifier uniqueness is per-workspace, so the same slug can exist
        # in two workspaces and must not bleed across.
        other_ws = Workspace.objects.create(
            name="Other Workspace", owner=create_user, slug="other-workspace"
        )
        WorkspaceMember.objects.create(workspace=other_ws, member=create_user, role=20)
        Project.objects.create(
            name="Other Project",
            identifier=project_member.identifier,
            workspace=other_ws,
            created_by=create_user,
        )

        # Hit `workspace` (test-workspace) with the slug — should resolve to
        # the original project, not the new one in `other_ws`.
        resp = api_key_client.get(
            f"/api/v1/workspaces/{workspace.slug}/projects/{project_member.identifier}/"
        )
        assert resp.status_code == status.HTTP_200_OK
        assert resp.json()["id"] == str(project_member.id)

    def test_app_v1_route_accepts_slug(
        self, session_client, workspace, project_member
    ):
        # The `/api/v1/` REST tree and the `/api/` app tree mount different
        # base view classes; both got the same kwarg-rewrite hook. Hit a
        # representative app endpoint with the slug to prove the parallel
        # path works.
        slug_resp = session_client.get(
            f"/api/workspaces/{workspace.slug}/projects/{project_member.identifier}/states/"
        )
        uuid_resp = session_client.get(
            f"/api/workspaces/{workspace.slug}/projects/{project_member.id}/states/"
        )
        assert uuid_resp.status_code < 400
        assert slug_resp.status_code == uuid_resp.status_code
