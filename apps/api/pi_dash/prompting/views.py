# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Prompt-section REST surface (design §7.2).

- ``GET    /api/workspaces/<slug>/prompt-sections?kind=&scope=`` — the ordered
  section list for a kind, each resolved (default / workspace / user) with its
  customizable flag, source, and override metadata. Member-readable.
- ``PUT    /api/workspaces/<slug>/prompt-sections/<key>?scope=`` — upsert an
  override body. ``scope=workspace`` is admin-only; ``scope=user`` is any
  member editing their own row. Runs save-time validation.
- ``DELETE /api/workspaces/<slug>/prompt-sections/<key>?scope=`` — deactivate
  an override (revert to the next rung in the chain).
- ``GET    /api/workspaces/<slug>/prompts/<kind>/compiled?scope=`` — the
  assembled final template (Jinja markers intact) plus the per-section
  breakdown. When ``scope=user`` and the caller has overrides, also returns the
  workspace-only ("automatic runs") compilation for comparison.
- ``POST   /api/workspaces/<slug>/prompts/<kind>/preview`` — render the
  compiled template against a real issue (coding-task / review) or scheduler
  binding (scheduler), without creating a run. Admin-gated.
"""

from __future__ import annotations

import uuid

from django.core.exceptions import ValidationError as DjangoValidationError
from django.db import IntegrityError, transaction
from rest_framework import status
from rest_framework.permissions import IsAuthenticated
from rest_framework.response import Response
from rest_framework.throttling import UserRateThrottle
from rest_framework.views import APIView

from pi_dash.db.models.issue import Issue
from pi_dash.db.models.workspace import Workspace, WorkspaceMember
from pi_dash.prompting import recipes, registry
from pi_dash.prompting.composer import (
    SOURCE_DEFAULT,
    SOURCE_WORKSPACE,
    compile_template,
    compose,
    effective_customizability,
    load_override_index,
    resolve_section,
)
from pi_dash.prompting.context import build_context, build_scheduler_context
from pi_dash.prompting.models import PromptSectionOverride
from pi_dash.prompting.renderer import PromptRenderError
from pi_dash.prompting.serializers import (
    PromptSectionOverrideSerializer,
    ResolvedSectionSerializer,
)
from pi_dash.prompting.validation import OverrideValidationError, validate_override

#: Numeric value of ``WorkspaceMember.role`` for the "Admin" role. Mirrors
#: ``db.models.workspace.ROLE_CHOICES``.
WORKSPACE_ADMIN_ROLE = 20

SCOPE_USER = "user"
SCOPE_WORKSPACE = "workspace"


class _FakeRun:
    """Stand-in for :class:`AgentRun` that carries the fields the context
    builders consume, so a preview never has to mutate the DB."""

    def __init__(self, run_id: uuid.UUID):
        self.id = run_id
        self.work_item_id = None


def _is_workspace_admin(user, workspace: Workspace) -> bool:
    if user.is_superuser:
        return True
    return WorkspaceMember.objects.filter(
        workspace=workspace, member=user, role=WORKSPACE_ADMIN_ROLE, is_active=True
    ).exists()


def _is_workspace_member(user, workspace: Workspace) -> bool:
    if user.is_superuser:
        return True
    return WorkspaceMember.objects.filter(
        workspace=workspace, member=user, is_active=True
    ).exists()


def _get_workspace_or_404(slug: str):
    try:
        return Workspace.objects.get(slug=slug)
    except Workspace.DoesNotExist:
        return None


def _resolve_scope(request) -> str:
    """Return the requested resolution scope (``user`` default, or
    ``workspace``)."""
    scope = request.query_params.get("scope", SCOPE_USER)
    return SCOPE_WORKSPACE if scope == SCOPE_WORKSPACE else SCOPE_USER


def _section_breakdown(kind: str, *, workspace, user) -> list:
    """Resolve every section in ``kind``'s recipe and attach ``needs_attention``
    from the override row that actually resolved.

    Bulk-loads overrides once (no per-section query) via the composer's index.
    """
    override_index = load_override_index(workspace, user)
    out = []
    for key in recipes.recipe_for(kind):
        section = registry.get_section(key)
        resolved = resolve_section(
            key, workspace=workspace, project=None, user=user, override_index=override_index
        )
        # The row that resolved is the one matching the resolved source.
        if resolved.source == SOURCE_WORKSPACE:
            row = override_index.get(("workspace", key))
        elif resolved.source != SOURCE_DEFAULT:  # "user:<id>"
            row = override_index.get(("user", key))
        else:
            row = None
        # Capability flags express what the *section* permits (the caller still
        # combines these with their own admin/member role client-side).
        tier = effective_customizability(section, workspace)
        out.append(
            {
                "key": resolved.key,
                "title": resolved.title,
                "customizable": tier,
                "body": resolved.body,
                # The pristine registry default, so the editor can diff an
                # active override against it without first reverting.
                "default_body": section.default_body,
                "source": resolved.source,
                "version": resolved.version,
                "needs_attention": bool(row.needs_attention) if row is not None else False,
                "editable_at_workspace": registry.tier_allows_workspace_override(tier),
                "editable_at_personal": registry.tier_allows_personal_override(tier),
            }
        )
    return out


class PromptSectionListEndpoint(APIView):
    """``GET /api/workspaces/<slug>/prompt-sections?kind=&scope=``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def get(self, request, slug: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_member(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        kind = request.query_params.get("kind", recipes.KIND_CODING_TASK)
        if kind not in recipes.RECIPES:
            return Response(
                {"error": f"unknown kind {kind!r}", "kinds": list(recipes.all_kinds())},
                status=status.HTTP_400_BAD_REQUEST,
            )
        scope = _resolve_scope(request)
        user = request.user if scope == SCOPE_USER else None
        breakdown = _section_breakdown(kind, workspace=workspace, user=user)
        return Response(
            {
                "kind": kind,
                "scope": scope,
                "sections": ResolvedSectionSerializer(breakdown, many=True).data,
            }
        )


class PromptSectionDetailEndpoint(APIView):
    """``PUT|DELETE /api/workspaces/<slug>/prompt-sections/<key>?scope=``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def _check_write_permission(self, request, workspace, scope) -> bool:
        if scope == SCOPE_WORKSPACE:
            return _is_workspace_admin(request.user, workspace)
        # User-scope writes: any member, own row only (enforced by user=self).
        return _is_workspace_member(request.user, workspace)

    def put(self, request, slug: str, section_key: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)

        scope = request.data.get("scope") or request.query_params.get("scope", SCOPE_USER)
        scope = SCOPE_WORKSPACE if scope == SCOPE_WORKSPACE else SCOPE_USER
        if not self._check_write_permission(request, workspace, scope):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)

        if section_key not in registry.REGISTRY:
            return Response(
                {"error": f"unknown section {section_key!r}"},
                status=status.HTTP_404_NOT_FOUND,
            )
        section = registry.get_section(section_key)
        # Governance tier gate (design §9.2). ``locked`` blocks every scope;
        # ``workspace`` allows the workspace scope only; ``overridable`` allows
        # both. The role check above already ensures workspace writes are admin.
        tier = effective_customizability(section, workspace)
        if scope == SCOPE_WORKSPACE and not registry.tier_allows_workspace_override(tier):
            return Response(
                {"error": f"section {section_key!r} is locked and cannot be overridden"},
                status=status.HTTP_403_FORBIDDEN,
            )
        if scope == SCOPE_USER and not registry.tier_allows_personal_override(tier):
            return Response(
                {
                    "error": (
                        f"section {section_key!r} cannot be personally overridden"
                        if tier == registry.CUSTOMIZABLE_WORKSPACE
                        else f"section {section_key!r} is locked and cannot be overridden"
                    )
                },
                status=status.HTTP_403_FORBIDDEN,
            )

        body = request.data.get("body")
        if body is None:
            return Response({"error": "body is required"}, status=status.HTTP_400_BAD_REQUEST)

        target_user = request.user if scope == SCOPE_USER else None
        try:
            validate_override(section_key, body, workspace=workspace, user=target_user)
        except OverrideValidationError as exc:
            return Response(
                {"error": "override validation failed", "detail": str(exc)},
                status=status.HTTP_400_BAD_REQUEST,
            )

        row = self._upsert(workspace, target_user, section_key, body, request.user)
        return Response(
            PromptSectionOverrideSerializer(row).data, status=status.HTTP_200_OK
        )

    def _upsert(self, workspace, target_user, section_key, body, editor):
        """Update the active override in place (bump version) or create one.

        Runs under ``transaction.atomic()`` with ``select_for_update`` so two
        concurrent PUTs (or a PUT racing a DELETE) can't lose an update or
        write a body onto a just-deactivated row. ``version`` is bumped with
        ``F('version') + 1`` to avoid read-modify-write drift.
        """

        def _base_qs():
            qs = PromptSectionOverride.objects.filter(
                workspace=workspace, section_key=section_key, is_active=True
            )
            return qs.filter(user__isnull=True) if target_user is None else qs.filter(user=target_user)

        try:
            with transaction.atomic():
                existing = _base_qs().select_for_update().first()
                if existing is not None:
                    return self._apply_update(existing, body, editor)
                return PromptSectionOverride.objects.create(
                    workspace=workspace,
                    user=target_user,
                    section_key=section_key,
                    body=body,
                    is_active=True,
                    version=1,
                    updated_by=editor,
                )
        except IntegrityError:
            # Lost the create race — an active row now exists; lock and update.
            with transaction.atomic():
                existing = _base_qs().select_for_update().first()
                if existing is None:
                    raise
                return self._apply_update(existing, body, editor)

    @staticmethod
    def _apply_update(row, body, editor):
        from django.db.models import F

        row.body = body
        row.version = F("version") + 1
        row.needs_attention = False
        row.updated_by = editor
        row.save(
            update_fields=["body", "version", "needs_attention", "updated_by", "updated_at"]
        )
        row.refresh_from_db(fields=["version"])  # resolve F() for the response
        return row

    def delete(self, request, slug: str, section_key: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)

        scope = request.query_params.get("scope", SCOPE_USER)
        scope = SCOPE_WORKSPACE if scope == SCOPE_WORKSPACE else SCOPE_USER
        if not self._check_write_permission(request, workspace, scope):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if section_key not in registry.REGISTRY:
            return Response(
                {"error": f"unknown section {section_key!r}"},
                status=status.HTTP_404_NOT_FOUND,
            )

        target_user = request.user if scope == SCOPE_USER else None
        qs = PromptSectionOverride.objects.filter(
            workspace=workspace, section_key=section_key, is_active=True
        )
        qs = qs.filter(user__isnull=True) if target_user is None else qs.filter(user=target_user)
        row = qs.first()
        if row is None:
            return Response(
                {"error": "no active override at this scope"},
                status=status.HTTP_404_NOT_FOUND,
            )
        row.is_active = False
        row.updated_by = request.user
        row.save(update_fields=["is_active", "updated_by", "updated_at"])
        return Response(status=status.HTTP_204_NO_CONTENT)


class PromptCompiledEndpoint(APIView):
    """``GET /api/workspaces/<slug>/prompts/<kind>/compiled?scope=``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def get(self, request, slug: str, kind: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_member(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if kind not in recipes.RECIPES:
            return Response(
                {"error": f"unknown kind {kind!r}", "kinds": list(recipes.all_kinds())},
                status=status.HTTP_400_BAD_REQUEST,
            )

        scope = _resolve_scope(request)
        user = request.user if scope == SCOPE_USER else None
        compiled = compile_template(kind, workspace=workspace, project=None, user=user)
        # The per-section breakdown is served by the section-list endpoint the
        # page already calls; the compiled endpoint only needs the assembled
        # template body, so we don't re-resolve and re-serialize sections here.
        payload = {
            "kind": kind,
            "scope": scope,
            "template_body": compiled.template_body,
        }
        # Dual compilation (§9.1): when resolving for a user who has overrides,
        # also surface the workspace-only template that automatic runs (ticks,
        # scheduler beats) would use, so the seam is visible.
        if user is not None and any(
            r.source.startswith("user:") for r in compiled.resolved
        ):
            automatic = compile_template(kind, workspace=workspace, project=None, user=None)
            payload["automatic_template_body"] = automatic.template_body
        return Response(payload)


class PromptPreviewEndpoint(APIView):
    """``POST /api/workspaces/<slug>/prompts/<kind>/preview``."""

    permission_classes = [IsAuthenticated]
    throttle_classes = [UserRateThrottle]

    def post(self, request, slug: str, kind: str):
        workspace = _get_workspace_or_404(slug)
        if workspace is None:
            return Response({"error": "workspace not found"}, status=status.HTTP_404_NOT_FOUND)
        if not _is_workspace_admin(request.user, workspace):
            return Response({"error": "forbidden"}, status=status.HTTP_403_FORBIDDEN)
        if kind not in recipes.RECIPES:
            return Response(
                {"error": f"unknown kind {kind!r}", "kinds": list(recipes.all_kinds())},
                status=status.HTTP_400_BAD_REQUEST,
            )

        scope = request.data.get("scope", SCOPE_WORKSPACE)
        user = request.user if scope == SCOPE_USER else None

        if kind == recipes.KIND_SCHEDULER:
            context, project, err = self._scheduler_context(request, workspace)
        else:
            context, project, err = self._issue_context(request, workspace, kind)
        if err is not None:
            return err

        try:
            composed = compose(
                kind, workspace=workspace, project=project, user=user, context=context
            )
        except PromptRenderError as exc:
            return Response(
                {"error": "render failed", "detail": str(exc)},
                status=status.HTTP_422_UNPROCESSABLE_ENTITY,
            )
        return Response({"kind": kind, "prompt": composed.text})

    def _issue_context(self, request, workspace, kind):
        issue_id = request.data.get("issue_id")
        if not issue_id:
            return None, None, Response(
                {"error": "issue_id is required for this kind"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            issue = Issue.objects.select_related("project", "workspace", "state").get(
                id=issue_id, workspace=workspace
            )
        except (Issue.DoesNotExist, ValueError, DjangoValidationError):
            return None, None, Response(
                {"error": "issue not found"}, status=status.HTTP_404_NOT_FOUND
            )
        context = build_context(issue, _FakeRun(run_id=uuid.uuid4()))
        # Honor the requested kind even if it differs from the issue's state-
        # derived kind, so a preview of the review prompt against an In Progress
        # issue still reads as a review prompt.
        context["run"]["kind"] = kind
        return context, issue.project, None

    def _scheduler_context(self, request, workspace):
        from pi_dash.db.models.scheduler import SchedulerBinding

        binding_id = request.data.get("binding_id")
        if not binding_id:
            return None, None, Response(
                {"error": "binding_id is required for the scheduler kind"},
                status=status.HTTP_400_BAD_REQUEST,
            )
        try:
            binding = SchedulerBinding.objects.select_related(
                "scheduler", "project", "workspace"
            ).get(id=binding_id, workspace=workspace)
        except (SchedulerBinding.DoesNotExist, ValueError, DjangoValidationError):
            return None, None, Response(
                {"error": "scheduler binding not found"}, status=status.HTTP_404_NOT_FOUND
            )
        context = build_scheduler_context(binding, _FakeRun(run_id=uuid.uuid4()))
        return context, binding.project, None
