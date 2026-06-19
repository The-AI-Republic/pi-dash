# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from __future__ import annotations

from datetime import timedelta

from django.db.models import Exists, OuterRef
from django.utils import timezone
from rest_framework import status
from rest_framework.response import Response

from pi_dash.assistant.models import AssistantThread, AssistantTurn, ThreadKind
from pi_dash.assistant.serializers import AssistantThreadSerializer
from pi_dash.assistant.tasks import cancel_key
from pi_dash.assistant.views._base import AssistantBaseView
from pi_dash.settings.redis import redis_instance

# How long an empty, untitled chat thread is kept before it's reaped. The
# window protects a thread the user just opened (via "New chat") and is typing
# into — it has no turn yet and an empty title, so without the grace a
# concurrent thread-list refresh would delete it out from under them.
EMPTY_THREAD_GRACE = timedelta(hours=1)


def _reap_empty_threads(user, slug):
    """Delete the user's abandoned empty conversations: chat threads with no
    title, no turns (no chat history at all), and no in-flight turn, once
    they're older than the grace window. Keeps storage free of "Untitled"
    threads left behind when a new chat is opened but never used."""
    cutoff = timezone.now() - EMPTY_THREAD_GRACE
    has_turn = AssistantTurn.objects.filter(thread=OuterRef("pk"))
    (
        AssistantThread.objects.filter(
            user=user,
            workspace__slug=slug,
            kind=ThreadKind.CHAT,
            title="",
            active_turn__isnull=True,
            created_at__lt=cutoff,
        )
        .annotate(_has_turn=Exists(has_turn))
        .filter(_has_turn=False)
        .delete()
    )


class AssistantThreadListCreateEndpoint(AssistantBaseView):
    def get(self, request, slug):
        denied = self.require_member(request, slug)
        if denied:
            return denied
        # Reap abandoned empty conversations before listing so they neither
        # clutter the UI nor accumulate in storage.
        _reap_empty_threads(request.user, slug)
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
