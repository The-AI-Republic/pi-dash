"""Per-run, tenant-scoped tool catalog for the stateless Cloud Agent."""

from __future__ import annotations

import hashlib
import json
import uuid
from contextvars import ContextVar
from pathlib import PurePosixPath
from crum import impersonate
from django.conf import settings
from django.db import transaction
from django.db.models import Max, Q
from django.utils import timezone

from pi_dash.assistant.runtime.markdown import to_safe_html
from pi_dash.cloud_agent import events
from pi_dash.core.permissions import ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, check_project_role, is_workspace_member
from pi_dash.db.models import GitCodeReviewLink, GitRepositoryBinding, Issue, IssueComment, Project, State
from pi_dash.runner.models import AgentRun, AgentRunToolCall, ToolCallStatus


class ToolDenied(RuntimeError):
    pass


current_tool_call_id: ContextVar[str | None] = ContextVar("cloud_agent_tool_call_id", default=None)


def _canonical(value) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), default=str).encode()


def _fingerprint(value) -> str:
    return hashlib.sha256(_canonical(value)).hexdigest()


def _bounded(value):
    raw = _canonical(value)
    if len(raw) > settings.CLOUD_AGENT_MAX_TOOL_RESULT_BYTES:
        raise RuntimeError("tool_result_too_large")
    return value


def _scope(run_id, *, write=False):
    run = AgentRun.objects.select_related(
        "created_by",
        "workspace",
        "work_item",
        "work_item__project",
        "scheduler_binding__project",
        "pod__project",
    ).get(pk=run_id)
    if run.cancel_requested_at:
        raise ToolDenied("run_cancelled")
    if (
        not run.created_by.is_active
        or run.created_by.is_bot
        or not is_workspace_member(run.created_by, run.workspace_id)
    ):
        raise ToolDenied("actor_no_longer_authorized")
    project = (
        run.work_item.project
        if run.work_item_id
        else run.scheduler_binding.project
        if run.scheduler_binding_id
        else run.pod.project
    )
    roles = [ROLE_ADMIN, ROLE_MEMBER] if write else [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST]
    if not check_project_role(run.created_by, run.workspace.slug, project.id, roles):
        raise ToolDenied("actor_no_longer_authorized")
    return run, project


def _audit(run_id, tool_name, risk, args, operation, *, source="internal", server_key=""):
    """Execute one tool call with a durable outcome and bounded replay value."""
    call_id = current_tool_call_id.get() or str(uuid.uuid4())
    fp = _fingerprint(args)
    write = risk == "write"
    if not settings.CLOUD_AGENT_ENABLED:
        raise ToolDenied("cloud_agent_disabled")
    if tool_name in set(settings.CLOUD_AGENT_DISABLED_TOOLS):
        raise ToolDenied("tool_disabled")
    if write and not settings.CLOUD_AGENT_WRITES_ENABLED:
        raise ToolDenied("writes_disabled")
    if source == "mcp" and server_key == "github" and not settings.CLOUD_AGENT_GITHUB_TOOLS_ENABLED:
        raise ToolDenied("github_tools_disabled")
    existing = AgentRunToolCall.objects.filter(agent_run_id=run_id, tool_call_id=call_id).first()
    if existing is not None:
        if existing.request_fingerprint != fp:
            raise ToolDenied("tool_call_fingerprint_mismatch")
        if existing.status == ToolCallStatus.SUCCEEDED and existing.safe_replay_result is not None:
            return existing.safe_replay_result
        raise ToolDenied("tool_call_already_submitted")
    if write:
        with transaction.atomic():
            run, _ = _scope(run_id, write=True)
            if (
                run.tool_calls.filter(risk="write", status=ToolCallStatus.SUCCEEDED).count()
                >= settings.CLOUD_AGENT_WRITE_CALL_LIMIT
            ):
                raise ToolDenied("write_limit")
            if run.tool_calls.filter(tool_name=tool_name, risk="write", status=ToolCallStatus.SUCCEEDED).exists():
                raise ToolDenied("write_tool_already_used")
            ledger = AgentRunToolCall.objects.create(
                agent_run=run,
                tool_call_id=call_id,
                source=source,
                server_key=server_key,
                tool_name=tool_name,
                risk=risk,
                status=ToolCallStatus.PREPARED,
                request_fingerprint=fp,
                idempotency_key_hash=_fingerprint({"run": str(run_id), "tool": tool_name}),
            )
            ledger.status = ToolCallStatus.SUBMITTED
            ledger.submitted_at = timezone.now()
            ledger.save(update_fields=["status", "submitted_at"])
            result = _bounded(operation(run))
            replay = result if len(_canonical(result)) <= 4096 else {"ok": True}
            ledger.status = ToolCallStatus.SUCCEEDED
            ledger.safe_replay_result = replay
            ledger.result_fingerprint = _fingerprint(result)
            ledger.completed_at = timezone.now()
            ledger.save(update_fields=["status", "safe_replay_result", "result_fingerprint", "completed_at"])
    else:
        run, _ = _scope(run_id)
        ledger = AgentRunToolCall.objects.create(
            agent_run=run,
            tool_call_id=call_id,
            source=source,
            server_key=server_key,
            tool_name=tool_name,
            risk=risk,
            status=ToolCallStatus.SUBMITTED,
            request_fingerprint=fp,
            submitted_at=timezone.now(),
        )
        try:
            result = _bounded(operation(run))
        except Exception as exc:
            ledger.status = ToolCallStatus.FAILED
            ledger.error_code = type(exc).__name__[:64]
            ledger.completed_at = timezone.now()
            ledger.save(update_fields=["status", "error_code", "completed_at"])
            raise
        ledger.status = ToolCallStatus.SUCCEEDED
        ledger.safe_replay_result = result if len(_canonical(result)) <= 4096 else {"ok": True}
        ledger.result_fingerprint = _fingerprint(result)
        ledger.completed_at = timezone.now()
        ledger.save(update_fields=["status", "safe_replay_result", "result_fingerprint", "completed_at"])
    events.append(run_id, "tool_completed", {"tool": tool_name, "risk": risk, "status": "succeeded"})
    return result


def _issue_data(issue, *, detail=False):
    data = {
        "id": str(issue.id),
        "identifier": f"{issue.project.identifier}-{issue.sequence_id}",
        "name": issue.name,
        "state": issue.state.name if issue.state_id else None,
        "state_group": issue.state.group if issue.state_id else None,
        "priority": issue.priority,
    }
    if detail:
        data.update(description=(issue.description_stripped or "")[:20_000], workpad=(issue.workpad or "")[:32_000])
    return data


def _project_id(run):
    if run.work_item_id:
        return run.work_item.project_id
    if run.scheduler_binding_id:
        return run.scheduler_binding.project_id
    return run.pod.project_id


def build_tools(run_id, allowed_names, *, source="internal", server_key=""):
    """Return only tools granted by the immutable creation plan."""
    allowed = set(allowed_names)

    def audit(tool_name, risk, args, operation):
        return _audit(
            run_id,
            tool_name,
            risk,
            args,
            operation,
            source=source,
            server_key=server_key,
        )

    def granted(func):
        return func if func.__name__ in allowed else None

    def pidash_get_current_issue():
        """Read the issue bound to this run."""
        return audit("pidash_get_current_issue", "read", {}, lambda run: _issue_data(run.work_item, detail=True))

    def pidash_list_current_issue_comments():
        """List the newest 50 comments on the issue bound to this run."""

        def op(run):
            return [
                {"id": str(c.id), "body": (c.comment_stripped or "")[:10_000], "created_at": c.created_at.isoformat()}
                for c in run.work_item.issue_comments.order_by("-created_at")[:50]
            ]

        return audit("pidash_list_current_issue_comments", "read", {}, op)

    def pidash_list_project_states():
        """List valid workflow states for the bound project."""
        return audit(
            "pidash_list_project_states",
            "read",
            {},
            lambda run: [
                {"id": str(s.id), "name": s.name, "group": s.group}
                for s in State.objects.filter(project_id=_project_id(run)).order_by("sequence")
            ],
        )

    def pidash_search_project_issues(query: str, limit: int = 20):
        """Search issue titles and descriptions in the bound project (max 50)."""
        limit = max(1, min(limit, 50))

        def op(run):
            project_id = _project_id(run)
            qs = (
                Issue.objects.filter(project_id=project_id)
                .filter(Q(name__icontains=query) | Q(description_stripped__icontains=query))
                .select_related("project", "state")
                .distinct()[:limit]
            )
            return [_issue_data(issue) for issue in qs]

        return audit("pidash_search_project_issues", "read", {"query": query, "limit": limit}, op)

    def pidash_get_project_issue(issue_id: str):
        """Get one issue by UUID, limited to this run's project."""

        def op(run):
            project_id = _project_id(run)
            issue = Issue.objects.select_related("project", "state").get(pk=issue_id, project_id=project_id)
            return _issue_data(issue, detail=True)

        return audit("pidash_get_project_issue", "read", {"issue_id": issue_id}, op)

    def pidash_list_linked_code_reviews():
        """List code reviews linked to the current issue."""

        def op(run):
            return [
                {
                    "id": str(link.id),
                    "provider": link.provider,
                    "url": link.url,
                    "title": link.title,
                    "state": link.state,
                    "number": link.external_iid,
                }
                for link in GitCodeReviewLink.objects.filter(
                    issue_id=run.work_item_id,
                    deleted_at__isnull=True,
                )[:20]
            ]

        return audit("pidash_list_linked_code_reviews", "read", {}, op)

    def pidash_add_current_issue_comment(body: str):
        """Add one Markdown comment to the current issue."""
        if not body.strip() or len(body) > 10_000:
            raise ValueError("comment must contain 1-10000 characters")

        def op(run):
            with impersonate(run.created_by):
                comment = IssueComment.objects.create(
                    issue=run.work_item,
                    project=run.work_item.project,
                    workspace=run.workspace,
                    actor=run.created_by,
                    comment_html=to_safe_html(body),
                    comment_json={},
                    speaker_type="agent",
                    speaker_label="Pi Dash Cloud Agent",
                    speaker_agent_run_id=run.id,
                )
            return {"created": True, "comment_id": str(comment.id)}

        return audit("pidash_add_current_issue_comment", "write", {"body": body}, op)

    def pidash_update_current_issue_workpad(body: str):
        """Replace the current issue's durable Markdown workpad."""
        if len(body) > 32_000:
            raise ValueError("workpad exceeds 32000 characters")

        def op(run):
            Issue.objects.filter(pk=run.work_item_id, project_id=run.work_item.project_id).update(
                workpad=body, updated_at=timezone.now()
            )
            return {"updated": True, "issue_id": str(run.work_item_id)}

        return audit("pidash_update_current_issue_workpad", "write", {"body": body}, op)

    def pidash_transition_current_issue(state_id: str):
        """Move the current issue to a state in the same project."""

        def op(run):
            state = State.objects.get(pk=state_id, project_id=run.work_item.project_id)
            Issue.objects.filter(pk=run.work_item_id).update(state=state, updated_at=timezone.now())
            return {"updated": True, "state": state.name, "state_id": str(state.id)}

        return audit("pidash_transition_current_issue", "write", {"state_id": state_id}, op)

    def pidash_create_project_issue(title: str, description: str = ""):
        """Create one backlog/default-state issue in the scheduler's project."""
        if not title.strip() or len(title) > 255 or len(description) > 20_000:
            raise ValueError("invalid issue title or description length")

        def op(run):
            project = Project.objects.select_for_update().get(pk=run.scheduler_binding.project_id)
            state = (
                State.objects.filter(project=project, default=True).first()
                or State.objects.filter(project=project, group="backlog").order_by("sequence").first()
            )
            seq = (Issue.objects.filter(project=project).aggregate(value=Max("sequence_id"))["value"] or 0) + 1
            with impersonate(run.created_by):
                issue = Issue.objects.create(
                    workspace=run.workspace,
                    project=project,
                    state=state,
                    name=title.strip(),
                    description_html=to_safe_html(description),
                    description_json={},
                    sequence_id=seq,
                    created_by=run.created_by,
                    created_via="cloud_agent",
                )
            return {"created": True, **_issue_data(issue)}

        return audit("pidash_create_project_issue", "write", {"title": title, "description": description}, op)

    def github_get_file(path: str, ref: str = ""):
        """Read a UTF-8 repository file from the verified GitHub binding."""
        if (
            not path
            or len(path) > 1024
            or path.startswith("/")
            or "\\" in path
            or "\x00" in path
            or ".." in PurePosixPath(path).parts
        ):
            raise ValueError("invalid relative repository path")
        if len(ref) > 255 or "://" in ref or "\x00" in ref:
            raise ValueError("invalid repository ref")

        def op(run):
            client, repo = _github_context(run)
            project = (
                run.work_item.project
                if run.work_item_id
                else run.scheduler_binding.project
                if run.scheduler_binding_id
                else run.pod.project
            )
            return client.get_file(
                repo.namespace,
                repo.name,
                path,
                ref=ref or repo.default_branch or project.base_branch,
            )

        return audit("github_get_file", "read", {"path": path, "ref": ref}, op)

    def github_get_linked_pull_request(aspect: str = "summary"):
        """Read summary, diff, files, checks, reviews, or comments for the linked GitHub PR."""
        if aspect not in {"summary", "diff", "files", "checks", "reviews", "comments"}:
            raise ValueError("invalid pull request aspect")

        def op(run):
            client, repo = _github_context(run)
            link = GitCodeReviewLink.objects.filter(
                issue_id=run.work_item_id,
                provider="github",
                host_url="https://github.com",
                namespace=repo.namespace,
                repo_name=repo.name,
                deleted_at__isnull=True,
            ).first()
            if link is None:
                raise RuntimeError("no_linked_pull_request")
            number = int(link.external_iid)
            methods = {
                "summary": client.get_pull_request,
                "diff": client.get_pull_request_diff,
                "files": client.list_pull_request_files,
                "checks": client.list_pull_request_checks,
                "reviews": client.list_pull_request_reviews,
                "comments": client.list_pull_request_comments,
            }
            return methods[aspect](link.namespace, link.repo_name, number)

        return audit("github_get_linked_pull_request", "read", {"aspect": aspect}, op)

    catalog = locals()
    return [catalog[name] for name in sorted(allowed) if name in catalog and callable(catalog[name])]


def _github_context(run):
    project_id = (
        run.work_item.project_id
        if run.work_item_id
        else run.scheduler_binding.project_id
        if run.scheduler_binding_id
        else run.pod.project_id
    )
    binding = GitRepositoryBinding.objects.select_related("repository", "provider_account__workspace_integration").get(
        project_id=project_id, deleted_at__isnull=True
    )
    account = binding.provider_account
    if (
        binding.workspace_id != run.workspace_id
        or binding.repository.provider != "github"
        or binding.repository.host_url != "https://github.com"
        or account.workspace_id != run.workspace_id
        or account.provider != "github"
        or account.host_url != "https://github.com"
        or account.auth_type != "github_app"
        or account.status != "connected"
        or not account.verified_at
        or account.workspace_integration_id is None
    ):
        raise ToolDenied("github_binding_unavailable")
    from pi_dash.db.models import GithubAppInstallation

    installation = GithubAppInstallation.objects.filter(
        workspace_integration_id=account.workspace_integration_id,
        suspended_at__isnull=True,
        verified_at__isnull=False,
    ).first()
    if installation is None:
        raise ToolDenied("github_installation_unavailable")
    from pi_dash.utils.github_client import GithubClient

    return GithubClient.for_installation(
        installation.installation_id, timeout=settings.CLOUD_AGENT_TOOL_TIMEOUT_SECONDS
    ), binding.repository
