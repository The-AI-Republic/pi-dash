# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Runner LLM model catalog — public read (PDASHOSS01-364).

``GET /api/runners/model-catalog/`` is the add-runner picker source.
Any signed-in user may read it; it returns only enabled rows grouped
by agent in sort order. The frontend renders "Default (agent's
built-in model)" first and keeps today's hardcoded list as a fallback
when this call fails or the backend predates the endpoint.
"""

from collections import OrderedDict

from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import RUNNER_MODEL_AGENTS, RunnerModelCatalog
from pi_dash.runner.serializers import RunnerModelCatalogSerializer


class ModelCatalogEndpoint(APIView):
    """``GET /api/runners/model-catalog/`` — enabled models grouped by agent.

    Response shape (pinned for the Rust port)::

        {"agents": {"claude-code": [...], "codex": [...], ...}}

    Every agent key is present even when empty, each value is ordered by
    ``sort_order``/``created_at`` and carries the preselected flag, so a
    picker can render without a client-side regroup. Disabled rows are
    omitted. Shape and ordering are frozen for both backends.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        rows = (
            RunnerModelCatalog.objects.filter(is_enabled=True)
            .order_by("agent", "sort_order", "created_at")
        )
        grouped: dict[str, list] = OrderedDict((agent, []) for agent in RUNNER_MODEL_AGENTS)
        for row in RunnerModelCatalogSerializer(rows, many=True).data:
            grouped[row["agent"]].append(row)
        return Response({"agents": grouped}, status=status.HTTP_200_OK)
