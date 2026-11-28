# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Move a page to another project of the same workspace.

Shared by the public API (``api/views/page.py``) and the web app's page views
(``app/views/page/base.py``). A page reaches a project through its
``ProjectPage`` row, so a move relinks that row: the ``Page`` row itself —
its id, owner, body, labels and version history — is untouched apart from
``moved_to_project`` (and ``parent``, see below).

* The page's sub-pages move with it, at any depth.
* A page moved on its own leaves its parent behind: a parent must be in the
  same project, so the moved page becomes a top-level page in the target.
* Favourites of the moved pages scoped to the source project are removed.
* The move is recorded in the page's log (``PageLog``) as
  ``entity_name="moved_to_project"``, ``entity_identifier=<target project>``.

Checks, in order (the first that fails decides the response):

1. ``project_id`` missing or not a UUID → 400
2. caller is not an active admin of the source project → 403
3. target is the source project → 400
4. target project not found in this workspace → 400
5. target project archived → 400
6. caller is not an active admin or member of the target project → 403
7. page locked → 400 ``PAGE_LOCKED``
"""

# Python imports
import uuid

# Django imports
from django.db import connection, transaction
from django.utils import timezone

# Module imports
from pi_dash.app.permissions import ROLE
from pi_dash.db.models import Page, PageLog, Project, ProjectMember, ProjectPage, UserFavorite
from pi_dash.utils.error_codes import ERROR_CODES

MOVE_LOG_ENTITY_NAME = "moved_to_project"
MOVE_LOG_ENTITY_TYPE = "project"


class PageMoveError(Exception):
    def __init__(self, http_status, message, code=None):
        super().__init__(message)
        self.http_status = http_status
        self.body = {"error": message}
        if code:
            self.body["error_code"] = ERROR_CODES[code]
            self.body["error_message"] = code


def _subtree_ids(page_id):
    """The page and all its live descendants, in a stable order."""
    sql = """
    WITH RECURSIVE descendants AS (
        SELECT id FROM pages WHERE id = %s
        UNION
        SELECT pages.id FROM pages JOIN descendants ON pages.parent_id = descendants.id
        WHERE pages.deleted_at IS NULL
    )
    SELECT id FROM descendants
    """
    with connection.cursor() as cursor:
        cursor.execute(sql, [page_id])
        return [row[0] for row in cursor.fetchall()]


def _is_member(project_id, user, roles):
    return ProjectMember.objects.filter(
        project_id=project_id, member=user, is_active=True, role__in=[role.value for role in roles]
    ).exists()


def move_page(*, page, source_project_id, data, user):
    """Move ``page`` (already resolved as visible in the source project) to
    ``data["project_id"]``. Returns the target project id; raises
    :class:`PageMoveError` when the move is refused."""
    raw_target = data.get("project_id") if hasattr(data, "get") else None
    if raw_target in (None, ""):
        raise PageMoveError(400, "project_id is required")
    try:
        target_project_id = uuid.UUID(str(raw_target))
    except ValueError:
        raise PageMoveError(400, "project_id must be a valid project id")
    source_project_id = uuid.UUID(str(source_project_id))

    if not _is_member(source_project_id, user, [ROLE.ADMIN]):
        raise PageMoveError(403, "Only a project admin can move a page to another project")
    if target_project_id == source_project_id:
        raise PageMoveError(400, "The page is already in this project")
    target = Project.objects.filter(pk=target_project_id, workspace_id=page.workspace_id).first()
    if target is None:
        raise PageMoveError(400, "The target project is not in this workspace")
    if target.archived_at is not None:
        raise PageMoveError(400, "The target project is archived")
    if not _is_member(target_project_id, user, [ROLE.ADMIN, ROLE.MEMBER]):
        raise PageMoveError(403, "You need write access to the target project to move a page into it")
    if page.is_locked:
        raise PageMoveError(400, "Page is locked", "PAGE_LOCKED")

    with transaction.atomic():
        current = Page.objects.select_for_update().only("is_locked", "parent_id").get(pk=page.pk)
        if current.is_locked:
            raise PageMoveError(400, "Page is locked", "PAGE_LOCKED")

        now = timezone.now()
        subtree = _subtree_ids(page.pk)
        source_links = ProjectPage.objects.filter(project_id=source_project_id, page_id__in=subtree)
        moved_ids = list(source_links.values_list("page_id", flat=True))
        already_in_target = set(
            ProjectPage.objects.filter(project_id=target_project_id, page_id__in=moved_ids).values_list(
                "page_id", flat=True
            )
        )
        # A page already linked to the target keeps that link; its source
        # link is dropped instead of relinked (one live link per pair).
        if already_in_target:
            source_links.filter(page_id__in=already_in_target).delete()
        ProjectPage.objects.filter(project_id=source_project_id, page_id__in=moved_ids).update(
            project_id=target_project_id, updated_at=now, updated_by_id=user.id
        )

        Page.objects.filter(pk__in=moved_ids).update(
            moved_to_project=target_project_id, updated_at=now, updated_by_id=user.id
        )
        if current.parent_id is not None:
            Page.objects.filter(pk=page.pk).update(parent_id=None)

        UserFavorite.objects.filter(
            entity_type="page",
            entity_identifier__in=moved_ids,
            project_id=source_project_id,
            workspace_id=page.workspace_id,
        ).delete()

        PageLog(
            transaction=uuid.uuid4(),
            page_id=page.pk,
            entity_identifier=target_project_id,
            entity_name=MOVE_LOG_ENTITY_NAME,
            entity_type=MOVE_LOG_ENTITY_TYPE,
            workspace_id=page.workspace_id,
        ).save(created_by_id=user.id)

    return target_project_id
