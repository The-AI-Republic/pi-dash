"""Contract: UUID-shaped project ids that do not name a live project.

PIDASHCONV-1167. Django's ``_rewrite_project_kwarg``
(``app/views/base.py:48-82``) returns early for ANY UUID-shaped
``project_id`` (``uuid.UUID(str(raw))`` succeeds) and never checks that the
project exists. Only non-UUID values run ``Project.resolve`` (the
``{"detail":"Project not found"}`` 404). A UUID therefore reaches the
permission layer untouched:

* class-gated page routes (``ProjectPagePermission``) deny a non-member
  with the DRF default 403 ``{"detail": ...}``;
* the favorite routes (``@allow_permission``) deny with the
  ``{"error": "You don't have the required permissions."}`` 403.

The same passthrough lets a soft-deleted project whose membership row is
still active pass the permission layer and reach the view body, where
``Project.objects.get(pk=project_id)`` (list/retrieve/summary) raises
``DoesNotExist`` → 404 ``{"error":"The required object does not exist."}``;
the favorite create never reads the project row, so it still answers 204.
"""

import uuid

from .conftest import page_url, pages_base

DRF_DENIED_BODY = {"detail": "You do not have permission to perform this action."}
PERMISSION_DENIED_BODY = {"error": "You don't have the required permissions."}
OBJECT_NOT_FOUND_BODY = {"error": "The required object does not exist."}
PROJECT_NOT_FOUND_BODY = {"detail": "Project not found"}


def favorite_url(world, project_id, page_id):
    return f"{pages_base(world).rsplit('/projects/', 1)[0]}/projects/{project_id}/favorite-pages/{page_id}/"


def workspace_projects_base(world):
    return pages_base(world).rsplit("/projects/", 1)[0] + "/projects"


# -- unknown / cross-workspace UUID: 403, not 404 ---------------------------


def test_list_unknown_uuid_project_is_403(user_client, world):
    url = f"{workspace_projects_base(world)}/{uuid.uuid4()}/pages/"
    response = user_client.get(url)
    assert response.status_code == 403, response.text
    assert response.json() == DRF_DENIED_BODY


def test_list_cross_workspace_uuid_project_is_403(user_client, world, world2):
    url = f"{workspace_projects_base(world)}/{world2['project']['id']}/pages/"
    response = user_client.get(url)
    assert response.status_code == 403, response.text
    assert response.json() == DRF_DENIED_BODY


def test_favorite_unknown_uuid_project_is_403(user_client, world):
    url = favorite_url(world, uuid.uuid4(), world["page"]["id"])
    response = user_client.post(url)
    assert response.status_code == 403, response.text
    assert response.json() == PERMISSION_DENIED_BODY


def test_favorite_cross_workspace_uuid_project_is_403(user_client, world, world2):
    url = favorite_url(world, world2["project"]["id"], world["page"]["id"])
    response = user_client.post(url)
    assert response.status_code == 403, response.text
    assert response.json() == PERMISSION_DENIED_BODY


# -- soft-deleted project with a live membership row ------------------------


def _soft_delete_project(db, project_id):
    db.execute("UPDATE projects SET deleted_at = now() WHERE id = %s", (project_id,))


def test_list_soft_deleted_project_is_404(user_client, world, db):
    _soft_delete_project(db, world["project"]["id"])
    response = user_client.get(page_url(world))
    assert response.status_code == 404, response.text
    assert response.json() == OBJECT_NOT_FOUND_BODY


def test_retrieve_soft_deleted_project_is_404(user_client, world, db):
    _soft_delete_project(db, world["project"]["id"])
    response = user_client.get(page_url(world, world["page"]["id"]))
    assert response.status_code == 404, response.text
    assert response.json() == OBJECT_NOT_FOUND_BODY


def test_favorite_soft_deleted_project_is_204(user_client, world, db):
    _soft_delete_project(db, world["project"]["id"])
    response = user_client.post(
        favorite_url(world, world["project"]["id"], world["page"]["id"])
    )
    assert response.status_code == 204, response.text
    assert response.content == b""


# -- unchanged: non-UUID identifiers still run Project.resolve --------------


def test_list_non_uuid_identifier_miss_is_404_project_not_found(user_client, world):
    url = f"{workspace_projects_base(world)}/NOPE-999/pages/"
    response = user_client.get(url)
    assert response.status_code == 404, response.text
    assert response.json() == PROJECT_NOT_FOUND_BODY


def test_favorite_non_uuid_identifier_miss_is_404_project_not_found(user_client, world):
    url = favorite_url(world, "NOPE-999", world["page"]["id"])
    response = user_client.post(url)
    assert response.status_code == 404, response.text
    assert response.json() == PROJECT_NOT_FOUND_BODY
