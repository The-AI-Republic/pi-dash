# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from rest_framework import status
from rest_framework.response import Response

from pi_dash.assistant.models import AssistantThread, ThreadKind
from pi_dash.assistant.serializers import AssistantThreadSerializer
from pi_dash.assistant.tasks import cancel_key
from pi_dash.assistant.views._base import AssistantBaseView
from pi_dash.settings.redis import redis_instance


class AssistantThreadListCreateEndpoint(AssistantBaseView):
    def get(self, request, slug):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        # Only chat threads surface in the assistant UI; loop (Auto Project
        # Management) threads are hidden — that filter is the entire opacity
        # mechanism (design §6.4).
        threads = AssistantThread.objects.filter(
            user=request.user, workspace__slug=slug, is_archived=False, kind=ThreadKind.CHAT
        ).order_by("-updated_at")[:50]
        return Response(AssistantThreadSerializer(threads, many=True).data)

    def post(self, request, slug):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        from pi_dash.db.models import Workspace

        workspace = Workspace.objects.filter(slug=slug).first()
        if workspace is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        thread = AssistantThread.objects.create(
            workspace=workspace,
            user=request.user,
            title=(request.data.get("title") or "")[:255],
        )
        return Response(AssistantThreadSerializer(thread).data, status=status.HTTP_201_CREATED)


class AssistantThreadDetailEndpoint(AssistantBaseView):
    def patch(self, request, slug, thread_id):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        thread = self.owned_thread(request, slug, thread_id)
        if thread is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        if "title" in request.data:
            thread.title = (request.data.get("title") or "")[:255]
        if "is_archived" in request.data:
            thread.is_archived = bool(request.data.get("is_archived"))
        thread.save(update_fields=["title", "is_archived", "updated_at"])
        return Response(AssistantThreadSerializer(thread).data)

    def delete(self, request, slug, thread_id):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        thread = self.owned_thread(request, slug, thread_id)
        if thread is None:
            return Response({"error": "not_found"}, status=status.HTTP_404_NOT_FOUND)
        # Cancel any active turn first so its worker stops touching the rows.
        if thread.active_turn_id:
            try:
                redis_instance().set(cancel_key(thread.active_turn_id), "1", ex=600)
            except Exception:
                pass
        thread.delete()
        return Response(status=status.HTTP_204_NO_CONTENT)
