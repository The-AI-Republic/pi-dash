# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Instance-admin runner model catalog CRUD (PDASHOSS01-364).

Mounted under ``/api/instances/runner-models/`` (see ``license/urls.py``).
Every method requires ``InstanceAdminPermission``; authenticated
non-admins get 403. Setting ``is_preselected=true`` clears the flag on
the agent's other rows in the same transaction, so at most one row
per agent stays preselected.
"""

from django.db import transaction
from rest_framework import status
from rest_framework.response import Response

from pi_dash.license.api.views.base import BaseAPIView
from pi_dash.runner.models import RunnerModelCatalog
from pi_dash.runner.serializers import RunnerModelCatalogSerializer


def _clear_preselected(agent: str, exclude_pk=None) -> None:
    qs = RunnerModelCatalog.objects.filter(agent=agent, is_preselected=True)
    if exclude_pk is not None:
        qs = qs.exclude(pk=exclude_pk)
    qs.update(is_preselected=False)


class RunnerModelListEndpoint(BaseAPIView):
    """``GET/POST /api/instances/runner-models/`` — list and create rows."""

    def get(self, request):
        rows = RunnerModelCatalog.objects.order_by("agent", "sort_order", "created_at")
        return Response(RunnerModelCatalogSerializer(rows, many=True).data, status=status.HTTP_200_OK)

    def post(self, request):
        serializer = RunnerModelCatalogSerializer(data=request.data)
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
        with transaction.atomic():
            validated = dict(serializer.validated_data)
            if validated.get("is_preselected"):
                _clear_preselected(validated["agent"])
            row = RunnerModelCatalogSerializer().create(validated)
        return Response(RunnerModelCatalogSerializer(row).data, status=status.HTTP_201_CREATED)


class RunnerModelDetailEndpoint(BaseAPIView):
    """``PATCH/DELETE /api/instances/runner-models/<id>/`` — edit or remove one row.

    PATCH body may carry any subset of the serializer's writable fields.
    ``is_preselected=true`` clears the agent's other rows atomically.
    DELETE removes only the catalog row; runners already storing the
    model string in their own config are unaffected.
    """

    def patch(self, request, pk):
        row = RunnerModelCatalog.objects.filter(pk=pk).first()
        if row is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        serializer = RunnerModelCatalogSerializer(row, data=request.data, partial=True)
        if not serializer.is_valid():
            return Response(serializer.errors, status=status.HTTP_400_BAD_REQUEST)
        with transaction.atomic():
            validated = dict(serializer.validated_data)
            new_agent = validated.get("agent", row.agent)
            if validated.get("is_preselected"):
                _clear_preselected(new_agent, exclude_pk=row.pk)
            updated = serializer.save()
        return Response(RunnerModelCatalogSerializer(updated).data, status=status.HTTP_200_OK)

    def delete(self, request, pk):
        row = RunnerModelCatalog.objects.filter(pk=pk).first()
        if row is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        row.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
