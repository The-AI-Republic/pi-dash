# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Assistant data model.

``AssistantTurn`` is the unit of agent execution and the *only* source of LLM
history replay (its ``model_messages`` is the verbatim serialized pydantic-ai
message list). ``AssistantMessage`` rows are a UI transcript projection;
``AssistantEvent`` rows are the SSE replay log. See
``.ai_design/integrate_ai_agent/02-backend.md`` §1.
"""

from __future__ import annotations

import re
import uuid

from django.conf import settings
from django.db import models


class ThreadKind(models.TextChoices):
    CHAT = "chat", "Chat"
    LOOP = "loop", "Loop"


class AssistantThread(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    workspace = models.ForeignKey(
        "db.Workspace", on_delete=models.CASCADE, related_name="assistant_threads"
    )
    user = models.ForeignKey(
        settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="assistant_threads"
    )
    title = models.CharField(max_length=255, blank=True, default="")
    # "chat" = user-driven conversation (visible in the assistant UI);
    # "loop" = Auto Project Management run thread (hidden from the thread list).
    kind = models.CharField(max_length=16, choices=ThreadKind.choices, default=ThreadKind.CHAT)
    is_archived = models.BooleanField(default=False)
    # The single in-flight turn for this thread (one-active-turn flag). Nullable
    # FK rather than a bool so cancellation/sweep have a handle on the turn.
    active_turn = models.ForeignKey(
        "assistant.AssistantTurn",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="+",
    )
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "assistant_thread"
        ordering = ("-updated_at",)
        indexes = [
            models.Index(fields=["workspace", "user", "-updated_at"], name="asst_thread_ws_user_idx"),
        ]

    def __str__(self) -> str:
        return f"AssistantThread({self.id})"


class TurnStatus(models.TextChoices):
    QUEUED = "queued", "Queued"
    RUNNING = "running", "Running"
    COMPLETED = "completed", "Completed"
    FAILED = "failed", "Failed"
    CANCELLED = "cancelled", "Cancelled"


class AssistantTurn(models.Model):
    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    thread = models.ForeignKey(
        AssistantThread, on_delete=models.CASCADE, related_name="turns"
    )
    user_message = models.ForeignKey(
        "assistant.AssistantMessage",
        on_delete=models.SET_NULL,
        null=True,
        blank=True,
        related_name="+",
    )
    status = models.CharField(max_length=16, choices=TurnStatus.choices, default=TurnStatus.QUEUED)
    # Verbatim ``result.new_messages()`` serialized via ModelMessagesTypeAdapter;
    # the ONLY history-replay source. Written once on completion.
    model_messages = models.JSONField(null=True, blank=True)
    usage = models.JSONField(null=True, blank=True)
    model_used = models.CharField(max_length=255, blank=True, default="")
    error_code = models.CharField(max_length=64, blank=True, default="")
    error_detail = models.TextField(blank=True, default="")
    created_at = models.DateTimeField(auto_now_add=True)
    started_at = models.DateTimeField(null=True, blank=True)
    completed_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "assistant_turn"
        ordering = ("created_at",)
        indexes = [
            models.Index(fields=["thread", "created_at"], name="asst_turn_thread_idx"),
            models.Index(fields=["status", "started_at"], name="asst_turn_status_idx"),
        ]

    def __str__(self) -> str:
        return f"AssistantTurn({self.id}, {self.status})"


class MessageKind(models.TextChoices):
    USER = "user", "User"
    ASSISTANT = "assistant", "Assistant"
    TOOL_CALL = "tool_call", "Tool call"
    TOOL_RESULT = "tool_result", "Tool result"
    ERROR = "error", "Error"


class MessageStatus(models.TextChoices):
    STREAMING = "streaming", "Streaming"
    COMPLETED = "completed", "Completed"
    FAILED = "failed", "Failed"
    CANCELLED = "cancelled", "Cancelled"


class AssistantMessage(models.Model):
    """UI transcript projection. NEVER used for LLM history reconstruction."""

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    thread = models.ForeignKey(
        AssistantThread, on_delete=models.CASCADE, related_name="messages"
    )
    turn = models.ForeignKey(
        AssistantTurn, on_delete=models.CASCADE, null=True, blank=True, related_name="messages"
    )
    seq = models.BigIntegerField(default=0)  # transcript ordering only
    kind = models.CharField(max_length=16, choices=MessageKind.choices)
    display_content = models.TextField(blank=True, default="")
    payload = models.JSONField(default=dict, blank=True)
    status = models.CharField(max_length=16, choices=MessageStatus.choices, default=MessageStatus.COMPLETED)
    created_at = models.DateTimeField(auto_now_add=True)
    completed_at = models.DateTimeField(null=True, blank=True)

    class Meta:
        db_table = "assistant_message"
        ordering = ("seq",)
        indexes = [
            models.Index(fields=["thread", "seq"], name="asst_msg_thread_seq_idx"),
        ]

    def __str__(self) -> str:
        return f"AssistantMessage({self.id}, {self.kind})"


class AssistantEvent(models.Model):
    """SSE replay log. Independent ``seq`` counter from AssistantMessage."""

    id = models.BigAutoField(primary_key=True)
    thread = models.ForeignKey(
        AssistantThread, on_delete=models.CASCADE, related_name="events"
    )
    turn = models.ForeignKey(
        AssistantTurn, on_delete=models.CASCADE, null=True, blank=True, related_name="events"
    )
    seq = models.BigIntegerField(default=0)  # SSE replay cursor
    kind = models.CharField(max_length=64)
    message_id = models.UUIDField(null=True, blank=True)
    payload = models.JSONField(default=dict, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)

    class Meta:
        db_table = "assistant_event"
        ordering = ("seq",)
        indexes = [
            models.Index(fields=["thread", "seq"], name="asst_event_thread_seq_idx"),
        ]

    def __str__(self) -> str:
        return f"AssistantEvent({self.thread_id}, {self.kind}, seq={self.seq})"


class ProviderKind(models.TextChoices):
    OPENAI_COMPATIBLE = "openai_compatible", "OpenAI-compatible"
    ANTHROPIC = "anthropic", "Anthropic"


class AssistantMCPServer(models.Model):
    """A user-configured MCP tool server the assistant may call during a turn.

    Per-user and global across workspaces, mirroring ``UserLLMConfig``'s
    scoping. Each enabled row becomes one pydantic-ai toolset for the duration
    of an agent run (see ``pi_dash.ee.assistant.model_provider``), so its tools
    sit alongside the built-in ``@assistant.tool`` functions. ``tool_prefix``
    keeps names from colliding between servers and with the built-ins.

    The optional ``auth_header_encrypted`` holds a full ``Authorization``
    header value (e.g. ``Bearer …``), encrypted at rest with the same backend
    as BYOK provider keys.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    user = models.ForeignKey(
        settings.AUTH_USER_MODEL,
        on_delete=models.CASCADE,
        related_name="assistant_mcp_servers",
    )
    name = models.CharField(max_length=80)
    url = models.URLField(max_length=500)
    auth_header_encrypted = models.BinaryField(null=True, blank=True)
    is_enabled = models.BooleanField(default=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "assistant_mcp_server"
        ordering = ("created_at",)
        constraints = [
            models.UniqueConstraint(fields=["user", "name"], name="assistant_mcp_server_user_name_uniq"),
        ]

    def __str__(self) -> str:
        return f"AssistantMCPServer({self.user_id}, {self.name})"

    @property
    def has_auth_header(self) -> bool:
        return bool(self.auth_header_encrypted)

    @property
    def tool_prefix(self) -> str:
        """Slugified ``name`` used to namespace this server's tool names.

        The reserved ``mcp_`` namespace keeps user-provided tool names disjoint
        from the assistant's built-in tools and deployment-provided toolsets.
        Falls back to the row id when the name has no alphanumeric content, so
        a prefix is always non-empty and stable.
        """
        slug = re.sub(r"[^a-z0-9]+", "_", self.name.strip().lower()).strip("_")
        return f"mcp_{slug}" if slug else f"mcp_{str(self.id).replace('-', '')[:8]}"


class UserLLMConfig(models.Model):
    """Per-user BYOK configuration (global across workspaces)."""

    user = models.OneToOneField(
        settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="assistant_llm_config"
    )
    provider_kind = models.CharField(
        max_length=32, choices=ProviderKind.choices, default=ProviderKind.OPENAI_COMPATIBLE
    )
    base_url = models.URLField(max_length=500, blank=True, default="")
    model_name = models.CharField(max_length=255, blank=True, default="")
    api_key_encrypted = models.BinaryField(null=True, blank=True)
    last_verified_at = models.DateTimeField(null=True, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        db_table = "assistant_user_llm_config"

    def __str__(self) -> str:
        return f"UserLLMConfig({self.user_id}, {self.provider_kind})"

    @property
    def has_api_key(self) -> bool:
        return bool(self.api_key_encrypted)
