# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.db import transaction
from django.db.models import Q
from django.utils import timezone
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.views import APIView

from pi_dash.authentication.session import BaseSessionAuthentication
from pi_dash.runner.models import AgentRun, AgentRunEvent, AgentRunStatus
from pi_dash.runner.serializers import (
    AgentRunEventSerializer,
    AgentRunSerializer,
)
from pi_dash.runner.services import matcher
from pi_dash.runner.services.permissions import (
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import send_to_runner
from pi_dash.runner.services.validation import (
    RunCreationError,
    validate_run_creation,
)


def _can_view_run(user, run: AgentRun) -> bool:
    """View is allowed for the creator, the runner's owner, or a workspace admin.

    Workspace membership is always required first — a user removed from the
    workspace must not be able to see runs there, even if they still appear as
    ``runner.owner`` (an admin bond that does not track current membership).
    """
    if not is_workspace_member(user, run.workspace_id):
        return False
    if run.created_by_id == user.id:
        return True
    if run.runner_id is not None and run.runner.owner_id == user.id:
        return True
    return is_workspace_admin(user, run.workspace_id)


def _can_cancel_run(user, run: AgentRun) -> bool:
    """Cancellation is permitted for the same set as view."""
    return _can_view_run(user, run)


class AgentRunListEndpoint(APIView):
    """List the caller's runs, or create a new one."""

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request):
        # "My runs" — runs the caller is involved with. Three involvement
        # signals are surfaced:
        #   1. created_by == caller (free-form runs they kicked off)
        #   2. work_item.created_by == caller (their issues)
        #   3. work_item.assignees contains caller (issues assigned to them)
        # Tick-driven runs carry created_by = agent system bot per
        # ``orchestration/scheduling._resolve_creator_for_trigger``, so a
        # creator-only filter would hide them from the human owner of the
        # issue. The OR over (1)+(2)+(3) puts them back in view.
        # ``distinct()`` guards against duplicates from the assignees join
        # when the caller satisfies more than one clause.
        #
        # Mandatory workspace-membership scope: clause (2) and (3) join
        # through ``work_item`` whose project lives in some workspace —
        # without an outer membership constraint a user removed from a
        # workspace would still see runs there because IssueAssignee /
        # Issue.created_by survive workspace removal. The subquery uses
        # the live (non-soft-deleted) WorkspaceMember default manager.
        from pi_dash.db.models import WorkspaceMember

        member_workspaces = WorkspaceMember.objects.filter(
            member=request.user
        ).values("workspace_id")
        qs = (
            AgentRun.objects.filter(workspace_id__in=member_workspaces)
            .filter(
                Q(created_by=request.user)
                | Q(work_item__created_by=request.user)
                | Q(work_item__assignees=request.user)
            )
            .distinct()
            .order_by("-created_at")
        )
        workspace_id = request.query_params.get("workspace")
        if workspace_id:
            qs = qs.filter(workspace_id=workspace_id)
        return Response(AgentRunSerializer(qs[:200], many=True).data)

    def post(self, request):
        triggered_by = (request.data.get("triggered_by") or "").strip()

        # Comment & Run flow — reuse the per-issue continuation pipeline
        # (parent resolution, runner pinning, drain) instead of creating
        # a fresh AgentRun from a prompt body. See
        # ``.ai_design/issue_ticking_system/design.md`` §4.6.
        if triggered_by == "comment_and_run":
            return self._post_comment_and_run(request)

        prompt = request.data.get("prompt")
        workspace_id = request.data.get("workspace")
        if not prompt:
            return Response(
                {"error": "prompt is required"},
                status=status.HTTP_400_BAD_REQUEST,
            )

        try:
            ctx = validate_run_creation(
                request.user,
                workspace_id,
                work_item_id=request.data.get("work_item"),
                pod_id=request.data.get("pod"),
            )
        except RunCreationError as exc:
            return Response(
                {"error": exc.message, "code": exc.code},
                status=exc.status,
            )

        with transaction.atomic():
            run = AgentRun.objects.create(
                workspace_id=ctx.workspace_id,
                created_by=ctx.created_by,
                pod=ctx.pod,
                prompt=prompt,
                run_config=request.data.get("run_config") or {},
                required_capabilities=request.data.get("required_capabilities") or [],
                work_item_id=ctx.work_item_id,
                # Owner stays NULL until assignment (design §5.3).
            )

        # Drain the pod's queue — assigns this run (or any predecessor) to
        # an idle runner if one exists. Non-blocking on commit.
        matcher.drain_pod(ctx.pod)
        run.refresh_from_db()
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )

    def _post_comment_and_run(self, request):
        """Dispatch a follow-up run for an issue (Comment & Run button).

        Body must include ``work_item`` (issue id). The just-posted comment
        is expected to already exist on the issue (the client posts it
        before calling this endpoint); the agent reads it from the comment
        thread via ``pidash comment list`` when the run executes.
        """
        from pi_dash.db.models.issue import Issue
        from pi_dash.orchestration import scheduling

        work_item_id = request.data.get("work_item")
        if not work_item_id:
            return Response(
                {"error": "work_item is required for comment_and_run"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        issue = Issue.all_objects.filter(pk=work_item_id).first()
        if issue is None:
            return Response(
                {"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND
            )
        if not is_workspace_member(request.user, issue.workspace_id):
            return Response(
                {"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND
            )

        with transaction.atomic():
            run = scheduling.dispatch_continuation_run(
                issue,
                triggered_by=scheduling.TRIGGER_COMMENT_AND_RUN,
                actor=request.user,
            )
            # Only reset the schedule when the dispatch actually committed
            # a run. Otherwise (active-run-exists / no-prior-run / no-pod
            # — all of which return None) the user's existing tick_count
            # and next_run_at must stay intact: they didn't trigger a new
            # invocation, so the cap budget shouldn't be refunded and the
            # next-tick clock shouldn't be pushed out.
            if run is not None:
                scheduling.reset_ticker_after_comment_and_run(issue)
        if run is None:
            return Response(
                {
                    "error": (
                        "could not dispatch — issue may already have an "
                        "active run, or no prior run / pod is available"
                    )
                },
                status=status.HTTP_409_CONFLICT,
            )
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )


class AgentRunDetailEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, run_id):
        run = (
            AgentRun.objects.select_related("runner").filter(id=run_id).first()
        )
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_view_run(request.user, run):
            # 404 not 403 — do not confirm run existence across workspaces.
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        include_events = request.query_params.get("include_events") == "1"
        payload = AgentRunSerializer(run).data
        if include_events:
            events = AgentRunEvent.objects.filter(agent_run=run).order_by("seq")[:500]
            payload["events"] = AgentRunEventSerializer(events, many=True).data
        return Response(payload)


class AgentRunCancelEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, run_id):
        # Authorization check happens on a non-locked read; re-check terminal
        # state after acquiring the row lock to avoid racing with
        # Runner.revoke() (which holds select_for_update on in-flight runs).
        run = (
            AgentRun.objects.select_related("runner").filter(id=run_id).first()
        )
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_cancel_run(request.user, run):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

        runner_id = run.runner_id
        with transaction.atomic():
            locked = (
                AgentRun.objects.select_for_update()
                .filter(id=run_id)
                .first()
            )
            if locked is None:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            if locked.is_terminal:
                return Response(
                    {"error": "run already terminal"},
                    status=status.HTTP_409_CONFLICT,
                )
            locked.status = AgentRunStatus.CANCELLED
            locked.ended_at = timezone.now()
            locked.save(update_fields=["status", "ended_at"])
            run = locked

        # Best-effort WS fan-out after commit; runner may already be offline or
        # revoked, in which case the frame is dropped silently.
        if runner_id:
            transaction.on_commit(
                lambda rid=runner_id, reason=request.data.get("reason", ""): send_to_runner(
                    rid,
                    {
                        "v": 1,
                        "type": "cancel",
                        "run_id": str(run_id),
                        "reason": reason,
                    },
                )
            )
        return Response(AgentRunSerializer(run).data)


class AgentRunReleasePinEndpoint(APIView):
    """Operator escape hatch: clear ``pinned_runner_id`` on a stuck QUEUED run.

    Used when the pinned runner is offline indefinitely and the human would
    rather give up native session resume than wait. The run remains QUEUED
    and falls into the pod's general queue; whichever runner picks it up
    starts a fresh session, with the issue + handoff comment as context.

    See §5.7 of ``.ai_design/issue_run_improve/design.md``.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request, run_id):
        run = (
            AgentRun.objects.select_related("runner", "pinned_runner")
            .filter(id=run_id)
            .first()
        )
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_cancel_run(request.user, run):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

        with transaction.atomic():
            locked = (
                AgentRun.objects.select_for_update().filter(id=run_id).first()
            )
            if locked is None:
                return Response(
                    {"error": "not found"}, status=status.HTTP_404_NOT_FOUND
                )
            if locked.status != AgentRunStatus.QUEUED:
                return Response(
                    {"error": "run not queued"},
                    status=status.HTTP_409_CONFLICT,
                )
            if locked.pinned_runner_id is None:
                return Response(
                    {"error": "run not pinned"},
                    status=status.HTTP_409_CONFLICT,
                )
            locked.pinned_runner = None
            # Also clear parent's stale thread_id so the upcoming dispatch
            # builds an Assign without a resume hint — the new runner has
            # no session to resume against. The handoff comment carries
            # the human-readable state.
            if (
                locked.parent_run is not None
                and locked.parent_run.thread_id
            ):
                locked.parent_run.thread_id = ""
                locked.parent_run.save(update_fields=["thread_id"])
            locked.save(update_fields=["pinned_runner"])
            run = locked

        if run.pod_id is not None:
            transaction.on_commit(
                lambda pid=run.pod_id: matcher.drain_pod_by_id(pid)
            )
        return Response(AgentRunSerializer(run).data)
