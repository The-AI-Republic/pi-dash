# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

from django.core.validators import RegexValidator
from rest_framework import serializers

from pi_dash.runner.models import (
    ALLOWED_REASONING_EFFORTS,
    RUNNER_MODEL_AGENTS,
    AgentChatApprovalRequest,
    AgentChatEvent,
    AgentChatMessage,
    AgentChatSession,
    AgentRun,
    AgentRunToolCall,
    AgentRunEvent,
    ApprovalRequest,
    DevMachine,
    Pod,
    Runner,
    RunnerLiveState,
    RunnerModelCatalog,
)
from pi_dash.runner.diagnostics import classify_run_error


# Mirrors the runner-side charset rule in `runner/src/util/runner_name.rs`
# and the CLI-created runner endpoint's `_RUNNER_NAME_RE`.
RUNNER_NAME_CHARSET = RegexValidator(
    regex=r"^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$",
    message=(
        "runner_name must start with a letter, digit, or underscore "
        "and contain only letters, digits, underscore, dot, or dash"
    ),
)


class PodSerializer(serializers.ModelSerializer):
    runner_count = serializers.SerializerMethodField()
    # Web's Add Runner picker needs to filter pods to a chosen project,
    # and to backfill the project field when the user picks a pod first.
    # ``project_identifier`` is the human-friendly slug; ``project`` is
    # the FK uuid kept for callers that want it.
    project_identifier = serializers.CharField(source="project.identifier", read_only=True)

    class Meta:
        model = Pod
        fields = [
            "id",
            "name",
            "description",
            "is_default",
            "workspace",
            "project",
            "project_identifier",
            "created_by",
            "runner_count",
            "created_at",
            "updated_at",
        ]
        read_only_fields = [
            "id",
            "is_default",
            "workspace",
            "project",
            "project_identifier",
            "created_by",
            "runner_count",
            "created_at",
            "updated_at",
        ]

    def get_runner_count(self, pod: Pod) -> int:
        return pod.runners.count()


class RunnerLiveStateSerializer(serializers.ModelSerializer):
    """Per-active-run agent observability snapshot.

    See ``.ai_design/runner_agent_bridge/design.md`` §4.5.4. All fields
    nullable; the UI renders ``null`` as ``"—"`` and derives the activity
    badge client-side from raw scalars.
    """

    class Meta:
        model = RunnerLiveState
        fields = [
            "observed_run_id",
            "last_event_at",
            "last_event_kind",
            "last_event_summary",
            "agent_pid",
            "agent_subprocess_alive",
            "approvals_pending",
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "usage",
            "llm_model",
            "turn_count",
            "agent_health",
            "agent_last_activity_at",
            "updated_at",
        ]
        read_only_fields = fields


class DevMachineSerializer(serializers.ModelSerializer):
    runner_count = serializers.IntegerField(read_only=True)
    online_runner_count = serializers.IntegerField(read_only=True)
    last_heartbeat_at = serializers.DateTimeField(read_only=True)
    # True when the machine has a recently-seen machine control session,
    # i.e. the daemon can execute cloud-pushed commands (``create_runner``)
    # right now. Annotated by the list endpoint; defaults False when the
    # queryset didn't annotate it.
    control_online = serializers.BooleanField(read_only=True, default=False)
    # ``never`` / ``active`` / ``expiring`` / ``expired`` / ``signed_out`` /
    # ``revoked`` (built_engine_as_runner §4.3), with expiry derived now.
    engine_sign_in_state = serializers.SerializerMethodField()
    engine_paused = serializers.BooleanField(read_only=True)

    class Meta:
        model = DevMachine
        fields = [
            "id",
            "host_label",
            "label",
            "visibility",
            # ``desktop_bundled`` marks a legacy per-workspace desktop machine.
            "provisioning",
            "runner_count",
            "online_runner_count",
            "control_online",
            # Built-in engine controls and state (built_engine_as_runner §8,
            # §11). Machines are private, so only the owner ever sees these.
            "engine_paused_by_owner",
            "engine_auto_pause_reason",
            "engine_paused",
            "engine_team_runs",
            "engine_sign_in_state",
            "engine_credential_expires_at",
            "engine_state",
            "engine_state_reported_at",
            "merged_into",
            "last_seen_at",
            "last_heartbeat_at",
            "revoked_at",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields

    def get_engine_sign_in_state(self, obj) -> str:
        return obj.current_engine_sign_in_state()


class PodMiniSerializer(serializers.ModelSerializer):
    # The runner detail page shows which project a runner is bound to.
    # ``project`` is the FK uuid; ``project_identifier`` is the
    # human-friendly slug (e.g. ``PDASHOSS01``) rendered in the UI.
    project_identifier = serializers.CharField(source="project.identifier", read_only=True)

    class Meta:
        model = Pod
        fields = ["id", "name", "is_default", "project", "project_identifier"]
        read_only_fields = fields


class DevMachineMiniSerializer(serializers.ModelSerializer):
    """Identity of the dev machine a runner is bound to, for the runner
    detail page. ``label`` is the user-chosen name; ``host_label`` is the
    hostname hint reported by the local CLI."""

    class Meta:
        model = DevMachine
        fields = [
            "id",
            "host_label",
            "label",
            "engine_paused_by_owner",
            "engine_auto_pause_reason",
            "engine_team_runs",
            "engine_sign_in_state",
            "engine_credential_expires_at",
        ]
        read_only_fields = fields

    def to_representation(self, instance):
        data = super().to_representation(instance)
        data["engine_sign_in_state"] = instance.current_engine_sign_in_state()
        return data


class RunnerSerializer(serializers.ModelSerializer):
    pod_detail = PodMiniSerializer(source="pod", read_only=True)
    # Identity of the dev machine this runner runs on. ``None`` when the
    # dev_machine FK is unset (legacy ``pidash connect`` enrollments).
    dev_machine_detail = DevMachineMiniSerializer(source="dev_machine", read_only=True)
    # Optional one-to-one observability snapshot. ``None`` when the row
    # doesn't exist yet (pre-flag runner that has never reported).
    live_state = RunnerLiveStateSerializer(read_only=True)
    # Built-in engine runner (``provisioning = desktop_bundled``), and whether
    # its machine's engine is paused (built_engine_as_runner §8, §11).
    is_engine = serializers.BooleanField(read_only=True)
    engine_paused = serializers.BooleanField(read_only=True)
    # Per-runner operator pause (``paused_at`` set), any runner kind.
    dispatch_paused = serializers.SerializerMethodField()
    # "Built-in engine · <machine>" is rendered from this.
    machine_display_name = serializers.CharField(read_only=True)
    # Dedicated-runner eligibility read (design §7.4): added to each row
    # only when the list was asked with ``?assignable_for=``. Populated in
    # ``to_representation`` from the serializer context so plain list reads
    # keep their current shape.
    assignable = serializers.SerializerMethodField()
    not_assignable_code = serializers.SerializerMethodField()
    is_busy = serializers.SerializerMethodField()
    queue_depth = serializers.SerializerMethodField()

    class Meta:
        model = Runner
        fields = [
            "id",
            "name",
            "status",
            "host_label",
            "is_engine",
            "engine_paused",
            "dispatch_paused",
            "machine_display_name",
            # Read-only: derived from the enrolling machine. Exposed so admin
            # and support tooling can answer "is this the bundled desktop
            # runner or one the user installed?" without a second query.
            "provisioning",
            # Whether the runner takes pod work (built_engine_as_runner §7).
            # Changed through the owner-only PATCH.
            "pod_dispatch",
            "os",
            "arch",
            "runner_version",
            "dev_metadata",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "dev_machine",
            "dev_machine_detail",
            "visibility",
            "pod",
            "pod_detail",
            "live_state",
            "enrolled_at",
            "revoked_at",
            "revoked_reason",
            "paused_at",
            "paused_by",
            "created_at",
            "updated_at",
            # Eligibility read (§7.4). Declared so the OpenAPI schema
            # documents them; values are gated by context in
            # ``to_representation`` below.
            "assignable",
            "not_assignable_code",
            "is_busy",
            "queue_depth",
        ]
        read_only_fields = [
            "id",
            "status",
            # Never client-settable: a runner must not be able to claim it is
            # Pi Dash-managed and thereby become eligible for pinned work.
            "provisioning",
            "pod_dispatch",
            "os",
            "arch",
            "runner_version",
            "dev_metadata",
            "protocol_version",
            "capabilities",
            "last_heartbeat_at",
            "owner",
            "dev_machine",
            "dev_machine_detail",
            "visibility",
            "pod_detail",
            "live_state",
            "enrolled_at",
            "revoked_at",
            "revoked_reason",
            "paused_at",
            "paused_by",
            "created_at",
            "updated_at",
            "assignable",
            "not_assignable_code",
            "is_busy",
            "queue_depth",
        ]

    def get_dispatch_paused(self, obj) -> bool:
        return obj.paused_at is not None

    def _assignable_context(self):
        ctx = self.context or {}
        user = ctx.get("user")
        issue = ctx.get("assignable_issue")
        return user, issue

    def _assignable_result(self, obj):
        from pi_dash.runner.services.issue_assignment import assignable_for_runner

        user, issue = self._assignable_context()
        return assignable_for_runner(obj, user=user, issue=issue)

    def to_representation(self, instance):
        data = super().to_representation(instance)
        ctx = self.context or {}
        if "assignable_issue" not in ctx and "user" not in ctx:
            # Plain list read: keep the pre-§7.4 shape (keys absent, as
            # ``listRunners`` without ``assignableFor`` expects).
            for key in ("assignable", "not_assignable_code", "is_busy", "queue_depth"):
                data.pop(key, None)
            return data
        ok, code = self._assignable_result(instance)
        from pi_dash.runner.services.issue_assignment import is_busy as _is_busy

        from pi_dash.runner.services.issue_assignment import queue_depth as _queue_depth

        data["assignable"] = ok
        data["not_assignable_code"] = code
        data["is_busy"] = _is_busy(instance)
        data["queue_depth"] = _queue_depth(instance)
        return data

    def get_assignable(self, obj):
        ok, _code = self._assignable_result(obj)
        return ok

    def get_not_assignable_code(self, obj):
        _ok, code = self._assignable_result(obj)
        return code

    def get_is_busy(self, obj):
        from pi_dash.runner.services.issue_assignment import is_busy as _is_busy

        return _is_busy(obj)

    def get_queue_depth(self, obj):
        from pi_dash.runner.services.issue_assignment import queue_depth as _queue_depth

        return _queue_depth(obj)


class RunnerEnrollRequestSerializer(serializers.Serializer):
    """``POST /api/v1/runner/runners/enroll/`` body."""

    enrollment_token = serializers.CharField(min_length=16, max_length=128)
    dev_machine_id = serializers.UUIDField(required=False)
    host_label = serializers.CharField(max_length=255)
    name = serializers.CharField(
        max_length=128,
        required=False,
        allow_blank=True,
        default="",
        validators=[RUNNER_NAME_CHARSET],
    )
    os = serializers.CharField(max_length=32, allow_blank=True, default="")
    arch = serializers.CharField(max_length=32, allow_blank=True, default="")
    version = serializers.CharField(max_length=32, allow_blank=True, default="")


class AgentRunToolCallSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentRunToolCall
        fields = [
            "id",
            "tool_call_id",
            "source",
            "server_key",
            "tool_name",
            "risk",
            "status",
            "error_code",
            "prepared_at",
            "submitted_at",
            "completed_at",
        ]
        read_only_fields = fields


class AgentRunSerializer(serializers.ModelSerializer):
    pod_detail = PodMiniSerializer(source="pod", read_only=True)
    error_diagnostic = serializers.SerializerMethodField()
    tool_calls = AgentRunToolCallSerializer(many=True, read_only=True)
    scheduler_binding_detail = serializers.SerializerMethodField()
    # Pin + waiting state (design §14.3): on every run, with
    # ``dispatch_wait`` computed only for QUEUED pinned runs.
    pinned_runner_name = serializers.SerializerMethodField()
    dispatch_wait = serializers.SerializerMethodField()

    def get_error_diagnostic(self, run: AgentRun):
        # Only the per-run detail drawer renders this block. Skip the
        # classifier's per-row string scans when serializing a list (the
        # runs table uses ``many=True``), which would otherwise pay the
        # cost on every row for data the list view discards.
        if isinstance(self.parent, serializers.ListSerializer):
            return None
        return classify_run_error(run.error)

    def get_pinned_runner_name(self, run: AgentRun):
        if run.pinned_runner_id is None:
            return None
        pinned = getattr(run, "pinned_runner", None)
        if pinned is not None:
            return pinned.name
        from pi_dash.runner.models import Runner

        row = Runner.objects.filter(pk=run.pinned_runner_id).first()
        return row.name if row else None

    def get_dispatch_wait(self, run: AgentRun):
        from pi_dash.runner.services.issue_assignment import build_dispatch_wait

        ctx = self.context or {}
        viewer = ctx.get("user")
        if viewer is None:
            request = ctx.get("request")
            viewer = getattr(request, "user", None) if request is not None else None
        return build_dispatch_wait(run, viewer=viewer)

    def get_scheduler_binding_detail(self, run: AgentRun):
        # "Fired by scheduler X" for scheduler-triggered runs, so run views
        # can label the run and link back to the binding detail page. Callers
        # that serialize lists select_related ``scheduler_binding__scheduler``
        # to keep this join out of the per-row query count. FK access uses the
        # base manager, so a soft-deleted (uninstalled) binding still resolves
        # and its history stays attributed.
        if run.scheduler_binding_id is None:
            return None
        binding = run.scheduler_binding
        return {
            "id": str(binding.id),
            "scheduler_name": binding.scheduler.name,
            "scheduler_slug": binding.scheduler.slug,
            "project": str(binding.project_id),
        }

    class Meta:
        model = AgentRun
        fields = [
            "id",
            "status",
            "executor_kind",
            "dispatch_attempts",
            "cancel_requested_at",
            "cancel_reason",
            "prompt",
            "thread_id",
            "agent_metadata",
            "runner",
            "pinned_runner",
            "pinned_runner_name",
            "pin_kind",
            "dispatch_wait",
            "work_item",
            "scheduler_binding",
            "scheduler_binding_detail",
            "pod",
            "pod_detail",
            "created_by",
            "owner",
            "created_at",
            "assigned_at",
            "started_at",
            "ended_at",
            "done_payload",
            "error",
            "error_code",
            "error_diagnostic",
            "refusal_category",
            "llm_model",
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "usage",
            "tool_plan",
            "tool_calls",
        ]
        read_only_fields = fields


class AgentRunEventSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentRunEvent
        fields = ["id", "seq", "kind", "payload", "created_at"]
        read_only_fields = fields


class ApprovalRequestSerializer(serializers.ModelSerializer):
    class Meta:
        model = ApprovalRequest
        fields = [
            "id",
            "agent_run",
            "kind",
            "payload",
            "reason",
            "status",
            "decision_source",
            "requested_at",
            "decided_at",
            "expires_at",
        ]
        read_only_fields = fields


class ApprovalDecisionSerializer(serializers.Serializer):
    decision = serializers.ChoiceField(choices=["accept", "decline"])


class AgentChatSessionSerializer(serializers.ModelSerializer):
    runner_detail = RunnerSerializer(source="runner", read_only=True)

    class Meta:
        model = AgentChatSession
        fields = [
            "id",
            "workspace",
            "runner",
            "runner_detail",
            "created_by",
            "pod",
            "status",
            "agent_kind",
            "local_thread_id",
            "local_session_id",
            "cwd",
            "model",
            "active_turn_id",
            "active_message_id",
            "close_requested",
            "last_message_at",
            "closed_at",
            "error",
            "created_at",
            "updated_at",
        ]
        read_only_fields = fields


class AgentChatMessageSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentChatMessage
        fields = [
            "id",
            "session",
            "role",
            "content",
            "content_parts",
            "status",
            "local_item_id",
            "local_turn_id",
            "seq",
            "created_at",
            "completed_at",
        ]
        read_only_fields = fields


class AgentChatEventSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentChatEvent
        fields = [
            "id",
            "session",
            "message",
            "seq",
            "kind",
            "payload",
            "created_at",
        ]
        read_only_fields = fields


class RunnerModelCatalogSerializer(serializers.ModelSerializer):
    """Row shape for the runner LLM model catalog (PDASHOSS01-364).

    The public read returns only enabled rows grouped by agent; the
    admin CRUD returns the full row. ``reasoning_efforts`` is a JSON
    list; ``reasoning_effort`` (singular) is not a field — the picker
    combines model + effort client-side for Codex rows.
    """

    agent = serializers.ChoiceField(choices=[(a, a) for a in RUNNER_MODEL_AGENTS])
    model = serializers.CharField(max_length=128, allow_blank=False, trim_whitespace=True)
    label = serializers.CharField(max_length=128, allow_blank=False, trim_whitespace=True)
    reasoning_efforts = serializers.JSONField(required=False, default=list)
    sort_order = serializers.IntegerField(min_value=0, required=False, default=0)
    is_enabled = serializers.BooleanField(required=False, default=True)
    is_preselected = serializers.BooleanField(required=False, default=False)

    class Meta:
        model = RunnerModelCatalog
        fields = [
            "id",
            "agent",
            "model",
            "label",
            "reasoning_efforts",
            "sort_order",
            "is_enabled",
            "is_preselected",
            "created_at",
            "updated_at",
        ]
        read_only_fields = ["id", "created_at", "updated_at"]

    def validate_reasoning_efforts(self, value):
        if value is None:
            return []
        if not isinstance(value, list):
            raise serializers.ValidationError("reasoning_efforts must be a list")
        seen = set()
        for item in value:
            if not isinstance(item, str):
                raise serializers.ValidationError("reasoning_efforts entries must be strings")
            if item not in ALLOWED_REASONING_EFFORTS:
                allowed = ", ".join(ALLOWED_REASONING_EFFORTS)
                raise serializers.ValidationError(f"reasoning_efforts entries must be one of: {allowed}")
            if item in seen:
                raise serializers.ValidationError("reasoning_efforts entries must be unique")
            seen.add(item)
        return list(value)

    def validate(self, attrs):
        # Enforce non-empty model/label after trim (CharField already
        # trims but a whitespace-only string becomes "" and would slip
        # through allow_blank=False as an empty string; keep it a 400).
        for field in ("model", "label"):
            if field in attrs and not str(attrs[field]).strip():
                raise serializers.ValidationError({field: "This field may not be blank."})
        # Duplicate model within an agent — friendly 400 instead of the
        # generic IntegrityError handler.
        agent = attrs.get("agent", getattr(self.instance, "agent", None))
        model = attrs.get("model", getattr(self.instance, "model", None))
        if agent and model:
            qs = RunnerModelCatalog.objects.filter(agent=agent, model=model)
            if self.instance is not None:
                qs = qs.exclude(pk=self.instance.pk)
            if qs.exists():
                raise serializers.ValidationError(
                    {"model": "A model with this agent and model already exists."}
                )
        return attrs


class AgentChatApprovalRequestSerializer(serializers.ModelSerializer):
    class Meta:
        model = AgentChatApprovalRequest
        fields = [
            "id",
            "session",
            "local_approval_id",
            "kind",
            "payload",
            "reason",
            "status",
            "decision_source",
            "decided_by",
            "requested_at",
            "expires_at",
            "decided_at",
        ]
        read_only_fields = fields
