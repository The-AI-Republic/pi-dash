# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Read-only project page endpoints on the public ``/api/v1/`` surface.

Project pages are a project-scoped wiki. Until now they were reachable only
through the first-party session API, which means every agent route into Pi
Dash — the ``pidash`` CLI and the MCP connector, both of which speak
``/api/v1/`` — was blind to them. These two endpoints are the read path.

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
"""

# Django imports
from django.db.models import Exists, OuterRef, Q

# Third party imports
from rest_framework import status
from rest_framework.response import Response
from drf_spectacular.utils import OpenApiResponse

# Module imports
from pi_dash.api.serializers import PageDetailSerializer, PageLiteSerializer
from pi_dash.app.permissions import ProjectEntityPermission
from pi_dash.db.models import Page, ProjectPage
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
)

from .base import BaseAPIView

#: Query-string values that turn a boolean flag on. ``format`` is reserved by
#: DRF for renderer selection, which is why the include-archived flag is
#: spelled ``include_archived``.
TRUE_VALUES = {"true", "1", "yes"}


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
