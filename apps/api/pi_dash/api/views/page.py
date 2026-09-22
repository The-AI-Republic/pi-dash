# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Project page endpoints on the public ``/api/v1/`` surface.

Project pages are a project-scoped wiki. Until now they were reachable only
through the first-party session API, which means every agent route into Pi
Dash — the ``pidash`` CLI and the MCP connector, both of which speak
``/api/v1/`` — was blind to them. These endpoints are the read path and the
write path (create, update, archive / unarchive). Deleting and locking are
deliberately absent: archive is reversible, and a lock is a human's control
over agent writes.

Access rules mirror ``pi_dash.app.views.page.base.PageViewSet``:

* the caller must be an active member of the project, and the project must
  not be archived — enforced by ``ProjectEntityPermission`` plus the
  queryset, so a non-member gets 403;
* a page is visible when its access is public or the caller owns it. A
  private page owned by somebody else is filtered out of the queryset rather
  than rejected by a permission check, so it reads as 404 — the API does not
  confirm that somebody else's private page exists.

Like the first-party view, these endpoints do **not** consult
``Project.page_view``: that flag only drives which tabs the web app renders
and is not enforced anywhere in the page read path.

Writing a body
--------------

A page is a collaborative Yjs document and ``description_binary`` is what
the editor loads; ``description_html`` is only read when the binary is
empty. So every body write regenerates HTML, JSON and binary together
through the live server (:func:`pi_dash.utils.live_document.convert_document`)
and applies the change on top of the page's current binary, so browsers
holding a cached copy merge it cleanly. If the live server is unavailable
the write fails with 503 and nothing is saved.

Known limitation: while somebody has the page open in the editor, the live
server holds the document in memory and writes it back on its next save,
which overwrites a concurrent API write. The live server's force-close path
does not avoid this — closing the connections unloads the document, and
unloading stores the in-memory copy.
"""

# Python imports
import base64
import json

# Django imports
from django.core.serializers.json import DjangoJSONEncoder
from django.db import transaction
from django.db.models import Exists, OuterRef, Q
from django.utils import timezone

# Third party imports
from rest_framework import status
from rest_framework.response import Response
from drf_spectacular.utils import OpenApiRequest, OpenApiResponse

# Module imports
from pi_dash.api.serializers import (
    PageCreateSerializer,
    PageDetailSerializer,
    PageLiteSerializer,
    PageUpdateSerializer,
)
from pi_dash.app.permissions import ROLE, ProjectEntityPermission
from pi_dash.app.serializers import PageBinaryUpdateSerializer, PageSerializer
from pi_dash.app.views.page.base import unarchive_archive_page_and_descendants
from pi_dash.bgtasks.page_transaction_task import page_transaction
from pi_dash.bgtasks.page_version_task import track_page_version
from pi_dash.db.models import Page, Project, ProjectMember, ProjectPage, UserFavorite
from pi_dash.utils.content_validator import validate_html_content
from pi_dash.utils.error_codes import ERROR_CODES
from pi_dash.utils.live_document import LiveConversionError, convert_document
from pi_dash.utils.markdown_converter import markdown_to_html
from pi_dash.utils.openapi import (
    page_docs,
    PAGE_ID_PARAMETER,
    CURSOR_PARAMETER,
    PER_PAGE_PARAMETER,
    FIELDS_PARAMETER,
    EXPAND_PARAMETER,
    INCLUDE_ARCHIVED_PARAMETER,
    create_paginated_response,
    # Response Examples
    PAGE_DETAIL_EXAMPLE,
    PAGE_CREATE_EXAMPLE,
    PAGE_UPDATE_EXAMPLE,
)

from .base import BaseAPIView

#: Query-string values that turn a boolean flag on. ``format`` is reserved by
#: DRF for renderer selection, which is why the include-archived flag is
#: spelled ``include_archived``.
TRUE_VALUES = {"true", "1", "yes"}

EMPTY_BODY_HTML = "<p></p>"


def _error(message, http_status, code=None):
    """v1 error envelope; ``code`` adds the numeric ``error_code`` the
    first-party page views use for the same condition."""
    body = {"error": message}
    if code:
        body["error_code"] = ERROR_CODES[code]
        body["error_message"] = code
    return Response(body, status=http_status)


def _conversion_failed(exc):
    return _error(
        f"The page body could not be saved and nothing was written: {exc}.",
        status.HTTP_503_SERVICE_UNAVAILABLE,
    )


def render_body_html(data):
    """Turn a validated write payload's body into sanitised Tiptap HTML.

    Raises ``ValueError`` when the body cannot be accepted.
    """
    if "description_markdown" in data:
        return markdown_to_html(data["description_markdown"])
    html = data.get("description_html") or ""
    if not html.strip():
        return EMPTY_BODY_HTML
    is_valid, error_message, clean_html = validate_html_content(html)
    if not is_valid:
        raise ValueError(error_message)
    return clean_html or EMPTY_BODY_HTML


def _document_fields(document):
    """Validate a live-server document through the first-party serializer,
    which sanitises the HTML and checks the binary, and return the three
    model fields to store."""
    serializer = PageBinaryUpdateSerializer(
        data={
            "description_html": document.description_html,
            "description_json": document.description_json,
            "description_binary": base64.b64encode(document.description_binary).decode("ascii"),
        }
    )
    serializer.is_valid(raise_exception=True)
    return {
        "description_html": serializer.validated_data.get("description_html") or EMPTY_BODY_HTML,
        "description_json": serializer.validated_data.get("description_json") or {},
        "description_binary": serializer.validated_data["description_binary"],
    }


def _record_body_write(page_id, old_description_html, new_description_html, user_id):
    """The bookkeeping ``PagesDescriptionViewSet.partial_update`` does on
    every body write: mentions / backlinks, then page history."""
    page_transaction.delay(
        new_description_html=new_description_html,
        old_description_html=old_description_html,
        page_id=page_id,
    )
    track_page_version.delay(
        page_id=page_id,
        existing_instance=json.dumps({"description_html": old_description_html}, cls=DjangoJSONEncoder)
        if old_description_html is not None
        else None,
        user_id=user_id,
    )


class BasePageReadAPIEndpoint(BaseAPIView):
    """Shared visibility queryset for the page read endpoints."""

    model = Page
    permission_classes = [ProjectEntityPermission]
    use_read_replica = True

    def get_queryset(self):
        # One `Exists` over the through table rather than a join through the
        # `projects` m2m: the m2m would fan the row out per linked project and
        # let the project filters match on *different* projects.
        linked_to_project = ProjectPage.objects.filter(
            page_id=OuterRef("pk"),
            project_id=self.kwargs.get("project_id"),
            project__archived_at__isnull=True,
            project__project_projectmember__member=self.request.user,
            project__project_projectmember__is_active=True,
        )
        return (
            Page.objects.filter(workspace__slug=self.kwargs.get("slug"))
            .filter(Exists(linked_to_project))
            .filter(Q(access=Page.PUBLIC_ACCESS) | Q(owned_by=self.request.user))
            .select_related("workspace")
            .select_related("owned_by")
        )

    def validate_parent(self, parent_id, page=None):
        """Return an error response when ``parent_id`` cannot be the parent.

        The parent must be a page the caller can see in this project, not
        archived, and — on update — not the page itself or one of its
        descendants.
        """
        if parent_id is None:
            return None
        parent = self.get_queryset().filter(pk=parent_id).first()
        if parent is None:
            return _error("Parent page not found in this project", status.HTTP_400_BAD_REQUEST)
        if parent.archived_at is not None:
            return _error("Parent page is archived", status.HTTP_400_BAD_REQUEST)
        if page is not None:
            ancestor_id, seen = parent.id, set()
            while ancestor_id is not None and ancestor_id not in seen:
                if ancestor_id == page.id:
                    return _error("A page cannot be nested under itself", status.HTTP_400_BAD_REQUEST)
                seen.add(ancestor_id)
                ancestor_id = Page.objects.filter(pk=ancestor_id).values_list("parent_id", flat=True).first()
        return None

    def get_page_or_error(self, page_id):
        page = self.get_queryset().filter(pk=page_id).first()
        if page is None:
            return None, _error("Page not found", status.HTTP_404_NOT_FOUND)
        return page, None

    def detail_response(self, page_id, http_status=status.HTTP_200_OK):
        page = self.get_queryset().get(pk=page_id)
        return Response(PageDetailSerializer(page).data, status=http_status)


class PageListAPIEndpoint(BasePageReadAPIEndpoint):
    """Project Page List Endpoint"""

    serializer_class = PageLiteSerializer

    @page_docs(
        operation_id="list_pages",
        summary="List pages",
        description=(
            "Retrieve the pages of a project. Returns metadata only — fetch a single page to read its body. "
            "Private pages owned by another member are not listed."
        ),
        parameters=[
            CURSOR_PARAMETER,
            PER_PAGE_PARAMETER,
            FIELDS_PARAMETER,
            EXPAND_PARAMETER,
            INCLUDE_ARCHIVED_PARAMETER,
        ],
        responses={
            200: create_paginated_response(
                PageLiteSerializer,
                "PaginatedPageResponse",
                "Paginated list of pages",
                "Paginated Pages",
            ),
        },
    )
    def get(self, request, slug, project_id):
        """List pages

        Retrieve the pages of a project. Returns page metadata only; archived
        pages are excluded unless `include_archived` is set.
        """
        queryset = self.get_queryset()
        if request.GET.get("include_archived", "false").lower() not in TRUE_VALUES:
            queryset = queryset.filter(archived_at__isnull=True)
        return self.paginate(
            request=request,
            queryset=queryset.order_by("-created_at"),
            on_results=lambda pages: PageLiteSerializer(pages, many=True, fields=self.fields, expand=self.expand).data,
        )

    @page_docs(
        operation_id="create_page",
        summary="Create page",
        description=(
            "Create a page in the project, owned by the caller. Send the body as `description_markdown` "
            "(preferred) or Tiptap `description_html`, not both. The body is stored as HTML, JSON and the "
            "collaborative document binary together; if the live document service is unavailable the "
            "request fails with 503 and no page is created."
        ),
        request=OpenApiRequest(request=PageCreateSerializer),
        examples=[PAGE_CREATE_EXAMPLE],
        responses={
            201: OpenApiResponse(
                description="Page created",
                response=PageDetailSerializer,
                examples=[PAGE_DETAIL_EXAMPLE],
            ),
            400: OpenApiResponse(description="Invalid request body or parent"),
            409: OpenApiResponse(description="The project is archived"),
            503: OpenApiResponse(description="The live document service is unavailable; nothing was written"),
        },
    )
    def post(self, request, slug, project_id):
        """Create page

        Create a page owned by the caller, with an optional markdown or HTML
        body, parent page and access level.
        """
        serializer = PageCreateSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        if Project.objects.filter(pk=project_id, archived_at__isnull=False).exists():
            return _error("The project is archived", status.HTTP_409_CONFLICT)

        parent_error = self.validate_parent(data.get("parent"))
        if parent_error:
            return parent_error

        try:
            html = render_body_html(data) if serializer.has_body else EMPTY_BODY_HTML
        except ValueError as exc:
            return _error(str(exc), status.HTTP_400_BAD_REQUEST)

        try:
            document = convert_document(html, title=data["name"])
        except LiveConversionError as exc:
            return _conversion_failed(exc)

        # Store the HTML as the editor serialises it (returned by the live
        # server), so all three formats describe the same document.
        fields = _document_fields(document)
        page_serializer = PageSerializer(
            data={
                "name": data["name"],
                "access": data.get("access", Page.PUBLIC_ACCESS),
                "parent": data.get("parent"),
            },
            context={"project_id": project_id, "owned_by_id": request.user.id, **fields},
        )
        page_serializer.is_valid(raise_exception=True)
        with transaction.atomic():
            page = page_serializer.save()

        _record_body_write(page.id, None, page.description_html, request.user.id)
        return self.detail_response(page.id, status.HTTP_201_CREATED)


class PageDetailAPIEndpoint(BasePageReadAPIEndpoint):
    """Project Page Detail Endpoint"""

    serializer_class = PageDetailSerializer

    @page_docs(
        operation_id="retrieve_page",
        summary="Retrieve page",
        description=(
            "Retrieve one page with its body rendered as HTML, plain text and markdown. "
            "`description_markdown` is derived on read and is the rendering agents should prefer."
        ),
        parameters=[PAGE_ID_PARAMETER, FIELDS_PARAMETER, EXPAND_PARAMETER],
        responses={
            200: OpenApiResponse(
                description="Page retrieved",
                response=PageDetailSerializer,
                examples=[PAGE_DETAIL_EXAMPLE],
            ),
        },
    )
    def get(self, request, slug, project_id, page_id):
        """Retrieve page

        Retrieve one page, including `description_markdown`. Archived pages
        are returned; a private page owned by another member is a 404.
        """
        serializer = PageDetailSerializer(
            self.get_queryset().get(pk=page_id),
            fields=self.fields,
            expand=self.expand,
        )
        return Response(serializer.data, status=status.HTTP_200_OK)

    @page_docs(
        operation_id="update_page",
        summary="Update page",
        description=(
            "Update a page's name, body, parent or access. Send only the fields to change; the body as "
            "`description_markdown` (preferred) or Tiptap `description_html`, not both. A locked page "
            "rejects every write (`PAGE_LOCKED`), an archived page rejects body writes (`PAGE_ARCHIVED`), "
            "and only the page owner can change `access`. A body write or rename regenerates the "
            "collaborative document through the live server; if it is unavailable the request fails with "
            "503 and nothing is written. An edit made while somebody has the page open in the editor can "
            "be overwritten by the editor's next save."
        ),
        parameters=[PAGE_ID_PARAMETER],
        request=OpenApiRequest(request=PageUpdateSerializer),
        examples=[PAGE_UPDATE_EXAMPLE],
        responses={
            200: OpenApiResponse(
                description="Page updated",
                response=PageDetailSerializer,
                examples=[PAGE_DETAIL_EXAMPLE],
            ),
            400: OpenApiResponse(description="Invalid request body or parent"),
            409: OpenApiResponse(description="The page is locked (`PAGE_LOCKED`) or archived (`PAGE_ARCHIVED`)"),
            503: OpenApiResponse(description="The live document service is unavailable; nothing was written"),
        },
    )
    def patch(self, request, slug, project_id, page_id):
        """Update page

        Partially update a page. Body writes are recorded in the page's
        version history.
        """
        page, error = self.get_page_or_error(page_id)
        if error:
            return error

        serializer = PageUpdateSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        data = serializer.validated_data

        if page.is_locked:
            return _error("Page is locked", status.HTTP_409_CONFLICT, "PAGE_LOCKED")
        if serializer.has_body and page.archived_at is not None:
            return _error("Page is archived", status.HTTP_409_CONFLICT, "PAGE_ARCHIVED")
        if "access" in data and data["access"] != page.access and page.owned_by_id != request.user.id:
            return _error("Only the page owner can change its access", status.HTTP_403_FORBIDDEN)
        if "parent" in data:
            parent_error = self.validate_parent(data["parent"], page=page)
            if parent_error:
                return parent_error

        renamed = "name" in data and data["name"] != page.name
        document = None
        # A rename must also rewrite the title stored inside the binary, or
        # the editor keeps showing the old name. A page with no binary yet
        # is converted from HTML + name the first time it is opened.
        if serializer.has_body or (renamed and page.description_binary):
            try:
                html = render_body_html(data) if serializer.has_body else page.description_html
            except ValueError as exc:
                return _error(str(exc), status.HTTP_400_BAD_REQUEST)
            try:
                document = convert_document(
                    html or EMPTY_BODY_HTML,
                    base_binary=page.description_binary,
                    title=data.get("name", page.name),
                )
            except LiveConversionError as exc:
                return _conversion_failed(exc)

        old_description_html = page.description_html
        update_fields = ["updated_at", "updated_by"]
        for field in ("name", "access"):
            if field in data:
                setattr(page, field, data[field])
                update_fields.append(field)
        if "parent" in data:
            page.parent_id = data["parent"]
            update_fields.append("parent")
        if document is not None:
            for field, value in _document_fields(document).items():
                setattr(page, field, value)
                update_fields.append(field)
            update_fields.append("description_stripped")

        # The live conversion is a network round trip, so re-check the guards
        # under a row lock and write only the fields this request changes: a
        # lock or archive applied meanwhile must win, not be reverted by the
        # stale instance.
        with transaction.atomic():
            current = Page.objects.select_for_update().only("is_locked", "archived_at").get(pk=page.pk)
            if current.is_locked:
                return _error("Page is locked", status.HTTP_409_CONFLICT, "PAGE_LOCKED")
            if serializer.has_body and current.archived_at is not None:
                return _error("Page is archived", status.HTTP_409_CONFLICT, "PAGE_ARCHIVED")
            page.save(update_fields=update_fields)

        if serializer.has_body:
            _record_body_write(page.id, old_description_html, page.description_html, request.user.id)
        return self.detail_response(page.id)


class PageArchiveAPIEndpoint(BasePageReadAPIEndpoint):
    """Archive (POST) and unarchive (DELETE) a page.

    Mirrors ``PageViewSet.archive`` / ``unarchive``: the page's descendants
    move with it, favourites are dropped on archive, and only the page owner
    or a project admin may do it. A locked page cannot be archived or
    unarchived — the lock is how a human freezes a page against agent writes.
    """

    serializer_class = PageDetailSerializer

    def _check_can_archive(self, request, page, project_id):
        if page.is_locked:
            return _error("Page is locked", status.HTTP_409_CONFLICT, "PAGE_LOCKED")
        is_admin = ProjectMember.objects.filter(
            project_id=project_id, member=request.user, is_active=True, role=ROLE.ADMIN.value
        ).exists()
        if page.owned_by_id != request.user.id and not is_admin:
            return _error(
                "Only the page owner or a project admin can archive or unarchive it",
                status.HTTP_403_FORBIDDEN,
            )
        return None

    @page_docs(
        operation_id="archive_page",
        summary="Archive page",
        description=(
            "Archive a page and its sub-pages. Only the owner or a project admin can archive a page; a "
            "locked page cannot be archived. Archiving an archived page is a no-op."
        ),
        parameters=[PAGE_ID_PARAMETER],
        request=None,
        responses={
            200: OpenApiResponse(description="Page archived", response=PageDetailSerializer),
            409: OpenApiResponse(description="The page is locked (`PAGE_LOCKED`)"),
        },
    )
    def post(self, request, slug, project_id, page_id):
        """Archive page

        Archive a page together with its sub-pages.
        """
        page, error = self.get_page_or_error(page_id)
        if error:
            return error
        error = self._check_can_archive(request, page, project_id)
        if error:
            return error

        if page.archived_at is None:
            UserFavorite.objects.filter(
                entity_type="page",
                entity_identifier=page_id,
                project_id=project_id,
                workspace__slug=slug,
            ).delete()
            unarchive_archive_page_and_descendants(page_id, timezone.now().date())
        return self.detail_response(page.id)

    @page_docs(
        operation_id="unarchive_page",
        summary="Unarchive page",
        description=(
            "Restore an archived page and its sub-pages. If the page's parent is still archived the page "
            "is moved to the top level. Only the owner or a project admin can unarchive a page."
        ),
        parameters=[PAGE_ID_PARAMETER],
        request=None,
        responses={
            200: OpenApiResponse(description="Page unarchived", response=PageDetailSerializer),
            409: OpenApiResponse(description="The page is locked (`PAGE_LOCKED`)"),
        },
    )
    def delete(self, request, slug, project_id, page_id):
        """Unarchive page

        Restore an archived page together with its sub-pages.
        """
        page, error = self.get_page_or_error(page_id)
        if error:
            return error
        error = self._check_can_archive(request, page, project_id)
        if error:
            return error

        if page.archived_at is not None:
            # Same as the first-party view: an unarchived child of a still
            # archived parent would be unreachable, so it moves to the top.
            if page.parent_id and page.parent.archived_at:
                page.parent = None
                page.save(update_fields=["parent"])
            unarchive_archive_page_and_descendants(page_id, None)
        return self.detail_response(page.id)
