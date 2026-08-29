# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import logging
import math

from django.conf import settings
from django.db import IntegrityError, transaction
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
    can_view_runner,
    is_workspace_admin,
    is_workspace_member,
)
from pi_dash.runner.services.pubsub import send_to_runner
from pi_dash.runner.services.validation import (
    RunCreationError,
    validate_run_creation,
)


DEFAULT_PER_PAGE = 30
MAX_PER_PAGE = 200
logger = logging.getLogger(__name__)


def _send_cancel_best_effort(runner_id, run_id, reason) -> None:
    from pi_dash.runner.services.outbox import RunnerOfflineError

    try:
        send_to_runner(
            runner_id,
            {
                "v": 1,
                "type": "cancel",
                "run_id": str(run_id),
                "reason": reason,
            },
        )
    except RunnerOfflineError:
        logger.info(
            "run cancel: runner %s offline; run %s remains cancel-requested",
            runner_id,
            run_id,
        )
    except Exception:
        logger.exception(
            "run cancel: failed to deliver cancellation for run %s",
            run_id,
        )


def _parse_pagination(query_params) -> tuple[int, int]:
    """Resolve ``page`` (1-based) and ``per_page`` from query params.

    Invalid or out-of-bounds values fall back to safe defaults rather than
    erroring: ``page`` clamps to a minimum of 1, ``per_page`` to ``[1,
    MAX_PER_PAGE]`` with a ``DEFAULT_PER_PAGE`` fallback.
    """

    def _to_int(value, default):
        try:
            return int(value)
        except (TypeError, ValueError):
            return default

    page = max(1, _to_int(query_params.get("page"), 1))
    per_page = _to_int(query_params.get("per_page"), DEFAULT_PER_PAGE)
    per_page = max(1, min(per_page, MAX_PER_PAGE))
    return page, per_page


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
    # Private-runner gate: a run executing on someone else's private machine
    # is visible only to the run creator and the runner owner (both handled
    # above) — not to issue participants or workspace admins. Runs without a
    # runner (queued local work, Cloud Agent runs) fall through to the
    # involvement grants below.
    if run.runner_id is not None and not can_view_runner(user, run.runner):
        return False
    if run.work_item_id is not None:
        if run.work_item.created_by_id == user.id:
            return True
        if run.work_item.assignees.filter(pk=user.id).exists():
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

        member_workspaces = WorkspaceMember.objects.filter(member=request.user).values("workspace_id")
        admin_workspaces = WorkspaceMember.objects.filter(member=request.user, role=20).values("workspace_id")
        qs = (
            AgentRun.objects.filter(workspace_id__in=member_workspaces)
            .filter(
                Q(created_by=request.user)
                | Q(runner__owner=request.user)
                | Q(work_item__created_by=request.user)
                | Q(work_item__assignees=request.user)
                | Q(workspace_id__in=admin_workspaces)
            )
            # Private-runner gate, mirroring ``_can_view_run``: involvement or
            # admin standing never reveals a run executing on someone else's
            # private machine — only the run creator and the runner owner see
            # those rows. Runner-less runs (queued, Cloud Agent) are unaffected.
            .filter(Q(runner__isnull=True) | Q(runner__owner=request.user) | Q(created_by=request.user))
            # ``pod__project`` is read by AgentRunSerializer.pod_detail
            # (PodMiniSerializer.project_identifier); join it to avoid an
            # N+1 across the up-to-200 rows serialized below.
            .select_related("pod__project")
            # AgentRunSerializer renders ``tool_calls`` inline; prefetch so a
            # page of runs costs one extra query instead of one per row.
            .prefetch_related("tool_calls")
            .distinct()
            .order_by("-created_at")
        )
        workspace_id = request.query_params.get("workspace")
        if workspace_id:
            qs = qs.filter(workspace_id=workspace_id)

        # Project scope for the per-project AI Workers panel. A run reaches its
        # project through its pod (``pod__project``), and a project owns several
        # pods, so this narrows to every run whose pod belongs to the project.
        # Kept as its own chained ``.filter()`` so it AND-combines without
        # touching the private-runner ``Q`` gate above — the visibility rule
        # holds regardless of scope. Pod-less runs (never assigned to a project)
        # are correctly excluded from a project view.
        project_id = request.query_params.get("project")
        if project_id:
            qs = qs.filter(pod__project_id=project_id)

        # Page-number pagination. The list grew unbounded (previously capped at
        # a flat 200), so the client now requests one page at a time and only
        # the first page loads by default. ``page`` is 1-based and reflected in
        # the runs-view URL so a page is directly linkable. Out-of-range pages
        # return an empty ``results`` with the real ``total_pages`` so the UI
        # can recover.
        page, per_page = _parse_pagination(request.query_params)
        total_count = qs.count()
        total_pages = max(1, math.ceil(total_count / per_page))
        offset = (page - 1) * per_page
        results = qs[offset : offset + per_page]
        return Response(
            {
                "results": AgentRunSerializer(results, many=True).data,
                "count": len(results),
                "total_count": total_count,
                "total_pages": total_pages,
                "page": page,
                "per_page": per_page,
            }
        )

    def post(self, request):
        triggered_by = (request.data.get("triggered_by") or "").strip()

        # Comment & Run flow — reuse the per-issue continuation pipeline
        # (parent resolution, runner pinning, drain) instead of creating
        # a fresh AgentRun from a prompt body. See
        # ``.ai_design/issue_ticking_system/design.md`` §4.6.
        if triggered_by == "comment_and_run":
            return self._post_comment_and_run(request)

        # Run AI button — same templated-prompt path as a state transition
        # into the issue's current ticking phase. The frontend sends only
        # the issue id; the prompt is rendered server-side from the phase
        # template via ``composer.build_first_turn``.
        if triggered_by == "run_ai":
            return self._post_run_ai(request)

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

        from pi_dash.cloud_agent.creation import dispatch_after_commit, execution_fields
        from pi_dash.db.models.issue import Issue

        try:
            with transaction.atomic():
                execution = execution_fields(
                    project=ctx.pod.project,
                    run_kind="direct",
                    has_issue=ctx.work_item_id is not None,
                    required_capabilities=request.data.get("required_capabilities") or [],
                    actor=ctx.created_by,
                    # Honor the work item's execution-target override when this
                    # direct run is bound to one; free-form runs inherit the
                    # project default.
                    requested=(
                        Issue.objects.filter(pk=ctx.work_item_id)
                        .values_list("agent_executor", flat=True)
                        .first()
                        if ctx.work_item_id is not None
                        else None
                    ),
                )
                run = AgentRun.objects.create(
                    workspace_id=ctx.workspace_id,
                    created_by=ctx.created_by,
                    pod=ctx.pod,
                    prompt="",
                    prompt_manifest=None,
                    run_config=request.data.get("run_config") or {},
                    required_capabilities=request.data.get("required_capabilities") or [],
                    work_item_id=ctx.work_item_id,
                    **execution,
                    # Owner stays NULL until assignment (design §5.3).
                )
                from pi_dash.prompting.composer import build_direct_turn

                run.prompt = build_direct_turn(
                    prompt,
                    run,
                    run.work_item if run.work_item_id else None,
                )
                # The prompt-size cap is a Cloud Agent execution limit; local
                # runs stream the prompt to the runner verbatim and never had
                # a limit — do not break that workflow.
                if (
                    run.executor_kind == "cloud_agent"
                    and len(run.prompt.encode()) > settings.CLOUD_AGENT_MAX_PROMPT_BYTES
                ):
                    raise OverflowError("prompt_too_large")
                run.save(update_fields=["prompt", "prompt_manifest"])
                dispatch_after_commit(run.id)
        except (ValueError, RuntimeError) as exc:
            code = getattr(exc, "code", str(exc))
            if code == "run_quota_exceeded":
                payload = {
                    "error": str(exc),
                    "code": code,
                    "retry_after_seconds": max(1, getattr(exc, "retry_after_seconds", 1)),
                }
                return Response(payload, status=status.HTTP_429_TOO_MANY_REQUESTS)
            if code == "admission_unavailable":
                return Response(
                    {"error": str(exc), "code": code},
                    status=status.HTTP_503_SERVICE_UNAVAILABLE,
                )
            if code == "cloud_capability_unavailable":
                return Response(
                    {"error": str(exc), "code": code},
                    status=status.HTTP_422_UNPROCESSABLE_ENTITY,
                )
            return Response({"error": str(exc), "code": code}, status=status.HTTP_409_CONFLICT)
        except OverflowError:
            return Response(
                {"error": "prompt exceeds Cloud Agent limit", "code": "prompt_too_large"},
                status=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE,
            )
        except IntegrityError:
            return Response(
                {"error": "work item already has an active run", "code": "active_run_exists"},
                status=status.HTTP_409_CONFLICT,
            )

        run.refresh_from_db()
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )

    def _post_run_ai(self, request):
        """Dispatch a run for the "Run AI" button.

        Body must include ``work_item`` (issue id). Builds the prompt
        from the phase template (``coding-task`` for In Progress,
        ``review`` for In Review, default otherwise) so the manual
        button produces the same prompt as a state-transition or tick
        for that phase.
        """
        from pi_dash.db.models.issue import Issue
        from pi_dash.orchestration import scheduling

        work_item_id = request.data.get("work_item")
        if not work_item_id:
            return Response(
                {"error": "work_item is required for run_ai"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        issue = Issue.all_objects.filter(pk=work_item_id).first()
        if issue is None:
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)
        if not is_workspace_member(request.user, issue.workspace_id):
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)

        with transaction.atomic():
            # Run AI is explicit human re-engagement — mirror the Comment
            # & Run reset so the next automatic tick budget restarts
            # cleanly. Reset BEFORE dispatch: the prompt renders the tick
            # budget at dispatch time, so resetting after would bake the
            # stale pre-reset count ("23 of 24 used") into a prompt whose
            # budget this same request just refunded. The rollback below
            # keeps the reset conditional on a run actually committing
            # (active-run-exists / no-pod return None).
            scheduling.reset_ticker_after_comment_and_run(issue)
            run = scheduling.dispatch_run_ai_run(issue, actor=request.user)
            if run is None:
                transaction.set_rollback(True)
        if run is None:
            return Response(
                {"error": ("could not dispatch — issue may already have an active run, or no pod is available")},
                status=status.HTTP_409_CONFLICT,
            )
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
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)
        if not is_workspace_member(request.user, issue.workspace_id):
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)

        with transaction.atomic():
            # Reset BEFORE dispatch: the prompt renders the tick budget at
            # dispatch time, so resetting after would bake the stale
            # pre-reset count into the prompt this same request refunds.
            # The reset must still only stick when the dispatch actually
            # commits a run — otherwise (active-run-exists / no-prior-run
            # / no-pod, all of which return None) the user's existing
            # tick_count and next_run_at must stay intact: they didn't
            # trigger a new invocation, so the cap budget shouldn't be
            # refunded and the next-tick clock shouldn't be pushed out.
            # Hence the rollback instead of a conditional reset.
            scheduling.reset_ticker_after_comment_and_run(issue)
            run = scheduling.dispatch_continuation_run(
                issue,
                triggered_by=scheduling.TRIGGER_COMMENT_AND_RUN,
                actor=request.user,
            )
            if run is None:
                transaction.set_rollback(True)
        if run is None:
            return Response(
                {
                    "error": (
                        "could not dispatch — issue may already have an active run, or no prior run / pod is available"
                    )
                },
                status=status.HTTP_409_CONFLICT,
            )
        return Response(
            AgentRunSerializer(run).data,
            status=status.HTTP_201_CREATED,
        )


class AgentReTickEndpoint(APIView):
    """Re-grant a fresh tick budget to an exhausted issue ticker.

    Manual "re-ticking" from the issue detail AgentRun card. Body must
    include ``work_item`` (issue id). Grants an extra phase-sized budget
    and re-arms the ticker **only** when the issue is still in a ticking
    state and the current budget is exhausted; otherwise it is a no-op.
    See ``scheduling.re_tick_ticker`` for the exact rules.
    """

    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def post(self, request):
        import uuid

        from pi_dash.db.models.issue import Issue
        from pi_dash.orchestration import scheduling

        work_item_id = request.data.get("work_item")
        if not work_item_id:
            return Response(
                {"error": "work_item is required for re-tick"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            uuid.UUID(str(work_item_id))
        except (ValueError, TypeError):
            # A malformed value would make the ``pk=`` lookup raise Django's
            # ValidationError (an unhandled 500) — return a clean 400 instead.
            return Response(
                {"error": "invalid work_item UUID format"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        issue = Issue.all_objects.filter(pk=work_item_id).first()
        if issue is None:
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)
        if not is_workspace_member(request.user, issue.workspace_id):
            return Response({"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND)

        result = scheduling.re_tick_ticker(issue)
        ticker = result["ticker"]
        payload = {
            "granted": result["granted"],
            "reason": result["reason"],
        }
        if ticker is not None:
            payload["tick_count"] = ticker.tick_count
            payload["max_ticks"] = ticker.effective_max_ticks()
            payload["enabled"] = ticker.enabled
            payload["next_run_at"] = ticker.next_run_at.isoformat() if ticker.next_run_at else None
        return Response(payload, status=status.HTTP_200_OK)


class AgentRunDetailEndpoint(APIView):
    authentication_classes = [BaseSessionAuthentication]
    permission_classes = [IsAuthenticated]

    def get(self, request, run_id):
        run = AgentRun.objects.select_related("runner", "work_item").filter(id=run_id).first()
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
        run = AgentRun.objects.select_related("runner", "work_item").filter(id=run_id).first()
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_cancel_run(request.user, run):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)

        runner_id = None
        reason = (request.data.get("reason") or "cancelled by user")[:512]
        if run.executor_kind == "cloud_agent" and run.status == AgentRunStatus.RUNNING:
            updated = AgentRun.objects.filter(
                pk=run.id, status=AgentRunStatus.RUNNING, cancel_requested_at__isnull=True
            ).update(cancel_requested_at=timezone.now(), cancel_reason=reason)
            if updated:
                run.refresh_from_db()
                return Response(AgentRunSerializer(run).data, status=status.HTTP_202_ACCEPTED)
        with transaction.atomic():
            locked = AgentRun.objects.select_for_update().filter(id=run_id).first()
            if locked is None:
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
            if locked.is_terminal:
                return Response(
                    {"error": "run already terminal", "code": "run_already_terminal"},
                    status=status.HTTP_409_CONFLICT,
                )
            if locked.status == AgentRunStatus.CANCEL_REQUESTED:
                run = locked
            elif locked.executor_kind == "cloud_agent" and locked.status == AgentRunStatus.RUNNING:
                # A running cloud run must never be finalized from here — the
                # Celery worker is still executing, and finalizing would
                # release workspace capacity mid-flight. A duplicate cancel
                # lands in this branch because the fast path above already
                # consumed cancel_requested_at; a first cancel can also land
                # here when the run went RUNNING between the unlocked read
                # and this lock. Record the request and let the worker's
                # cancellation poll finalize.
                if locked.cancel_requested_at is None:
                    locked.cancel_requested_at = timezone.now()
                    locked.cancel_reason = reason
                    locked.save(update_fields=["cancel_requested_at", "cancel_reason"])
                run = locked
            elif (
                locked.executor_kind != "cloud_agent"
                and locked.runner_id is not None
                and locked.status
                not in {
                    AgentRunStatus.QUEUED,
                    AgentRunStatus.PAUSED_AWAITING_INPUT,
                }
            ):
                # Do not free the runner in DB terms until its daemon confirms
                # the agent process has stopped. This closes the race where a
                # new Assign arrives while the cancelled process is still
                # winding down.
                locked.status = AgentRunStatus.CANCEL_REQUESTED
                locked.queue_position = None
                locked.cancel_requested_at = timezone.now()
                locked.cancel_reason = reason
                locked.save(update_fields=["status", "queue_position", "cancel_requested_at", "cancel_reason"])
                run = locked
            else:
                from pi_dash.runner.services.agent_run_finalization import finalize_agent_run

                finalize_agent_run(
                    locked.id,
                    AgentRunStatus.CANCELLED,
                    updates={
                        "cancel_requested_at": timezone.now(),
                        "cancel_reason": reason,
                        "error_code": "cancelled",
                        "error": reason,
                    },
                )
                locked.refresh_from_db()
                run = locked
            runner_id = locked.runner_id

        # Best-effort cancellation after commit. Offline runners receive the
        # persisted cancel request when they next open a session.
        if runner_id and run.status == AgentRunStatus.CANCEL_REQUESTED:
            transaction.on_commit(
                lambda rid=runner_id, reason=request.data.get("reason", ""): _send_cancel_best_effort(
                    rid,
                    run_id,
                    reason,
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
        run = AgentRun.objects.select_related("runner", "pinned_runner").filter(id=run_id).first()
        if run is None:
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _can_cancel_run(request.user, run):
            return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
        if run.executor_kind == "cloud_agent":
            return Response(
                {"error": "Cloud Agent runs cannot be pinned", "code": "executor_not_local"},
                status=status.HTTP_409_CONFLICT,
            )

        with transaction.atomic():
            locked = AgentRun.objects.select_for_update().filter(id=run_id).first()
            if locked is None:
                return Response({"error": "not found"}, status=status.HTTP_404_NOT_FOUND)
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
            if locked.parent_run is not None and locked.parent_run.thread_id:
                locked.parent_run.thread_id = ""
                locked.parent_run.save(update_fields=["thread_id"])
            locked.save(update_fields=["pinned_runner"])
            run = locked

        if run.pod_id is not None:
            transaction.on_commit(lambda pid=run.pod_id: matcher.drain_pod_by_id(pid))
        return Response(AgentRunSerializer(run).data)
