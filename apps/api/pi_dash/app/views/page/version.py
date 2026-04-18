# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

# Third party imports
from rest_framework import status
from rest_framework.response import Response

# Module imports
from pi_dash.db.models import PageVersion
from ..base import BaseAPIView
from pi_dash.app.serializers import PageVersionSerializer, PageVersionDetailSerializer
from pi_dash.app.permissions import ProjectPagePermission


class PageVersionEndpoint(BaseAPIView):
    permission_classes = [ProjectPagePermission]

    def get(self, request, slug, project_id, page_id, pk=None):
        # Check if pk is provided
        if pk:
            # Return a single page version
            page_version = PageVersion.objects.get(workspace__slug=slug, page_id=page_id, pk=pk)
            # Serialize the page version
            serializer = PageVersionDetailSerializer(page_version)
            return Response(serializer.data, status=status.HTTP_200_OK)
        # Return all page versions
        page_versions = PageVersion.objects.filter(workspace__slug=slug, page_id=page_id)
        # Serialize the page versions
        serializer = PageVersionSerializer(page_versions, many=True)
        return Response(serializer.data, status=status.HTTP_200_OK)
