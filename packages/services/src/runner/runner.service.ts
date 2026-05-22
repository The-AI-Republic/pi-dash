/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IAgentRun,
  IAgentChatApprovalRequest,
  IAgentChatMessage,
  IAgentChatSession,
  IApprovalRequest,
  IConnection,
  IConnectionWithToken,
  IRunner,
  IRunnerInvite,
  TApprovalDecision,
} from "@pi-dash/types";
import { APIService } from "../api.service";

/**
 * Pi Dash runner web-app API client. Session-authenticated; mounted at
 * /api/runners/ on the Django server.
 */
export class RunnerService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async list(workspaceId?: string): Promise<IRunner[]> {
    const params = workspaceId ? { params: { workspace: workspaceId } } : {};
    return this.get("/api/runners/", params)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /**
   * Hard-delete a runner cloud-side. Pass ``purgeLocal: true`` to also
   * cascade the teardown to the daemon (strips the ``[[runner]]``
   * block from ``config.toml`` and deletes the per-runner data dir);
   * pass ``false`` (or omit) to leave the local install intact.
   *
   * Default is ``true`` so a caller that doesn't pass options matches
   * the spec's default-checked checkbox.
   */
  async deleteRunner(runnerId: string, options?: { purgeLocal?: boolean }): Promise<void> {
    const purge = options?.purgeLocal ?? true;
    const url = `/api/runners/${runnerId}/?purge_local=${purge ? "true" : "false"}`;
    return this.delete(url)
      .then(() => undefined)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``POST /api/runners/<id>/revoke/`` — hard-revoke without removing
   * the row. Cascades to sessions, in-flight runs, and pinned follow-ups
   * via the model's ``revoke()``. Idempotent. */
  async revokeRunner(runnerId: string): Promise<IRunner> {
    return this.post(`/api/runners/${runnerId}/revoke/`, {})
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``POST /api/runners/<id>/revive/`` — mint a fresh enrollment token
   * on an existing PENDING-or-REVOKED runner row. The response payload
   * is identical to ``createRunnerInvite`` so the same install-command
   * panel can render it. 409 if the runner is currently active — revoke
   * it first. */
  async reviveRunner(runnerId: string): Promise<IRunnerInvite> {
    return this.post(`/api/runners/${runnerId}/revive/`, {})
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** Move a runner to a different pod (same workspace). Owner or admin only. */
  async move(runnerId: string, podId: string, name?: string): Promise<IRunner> {
    const body: Record<string, unknown> = { pod: podId };
    if (name !== undefined) body.name = name;
    return this.patch(`/api/runners/${runnerId}/`, body)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async getDetail(runnerId: string): Promise<IRunner> {
    return this.get(`/api/runners/${runnerId}/`)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``POST /api/runners/invites/`` — create a per-runner enrollment
   * invite. Each runner has its own enrollment token; the daemon
   * exchanges it for a long-lived refresh token via ``pidash connect``.
   * Replaces the legacy connection-level ``POST /api/runners/connections/``
   * — see ``.ai_design/move_to_https/design.md`` §5.1.
   */
  async createRunnerInvite(input: {
    workspaceId: string;
    projectIdentifier: string;
    podName?: string;
    name?: string;
  }): Promise<IRunnerInvite> {
    return this.post("/api/runners/invites/", {
      workspace: input.workspaceId,
      project: input.projectIdentifier,
      pod: input.podName ?? "",
      name: input.name ?? "",
    })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``GET /api/runners/connections/`` — list this user's connections. */
  async listConnections(): Promise<IConnection[]> {
    return this.get("/api/runners/connections/")
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``POST /api/runners/connections/`` — create a connection in PENDING state.
   * Response carries the one-time enrollment token; the daemon's
   * ``pidash connect`` consumes it. */
  async createConnection(workspaceId: string, name?: string): Promise<IConnectionWithToken> {
    return this.post("/api/runners/connections/", { workspace: workspaceId, name: name ?? "" })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``PATCH /api/runners/connections/<id>/`` — rename. */
  async renameConnection(connectionId: string, name: string): Promise<IConnection> {
    return this.patch(`/api/runners/connections/${connectionId}/`, { name })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  /** ``DELETE /api/runners/connections/<id>/`` — hard-delete the
   * connection and cascade to every runner under it.
   *
   * ``onlyIfPending`` adds a server-side guard that refuses the delete
   * when the connection has already enrolled. Used by the dismiss-token
   * flow so a daemon that enrolls between the UI's last refresh and the
   * delete call doesn't get its active connection silently nuked. */
  async deleteConnection(connectionId: string, onlyIfPending = false): Promise<void> {
    const url = onlyIfPending
      ? `/api/runners/connections/${connectionId}/?only_if_pending=true`
      : `/api/runners/connections/${connectionId}/`;
    return this.delete(url)
      .then(() => undefined)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listRuns(workspaceId?: string): Promise<IAgentRun[]> {
    const params = workspaceId ? { params: { workspace: workspaceId } } : {};
    return this.get("/api/runners/runs/", params)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async getRun(runId: string, includeEvents = false): Promise<IAgentRun> {
    const params = includeEvents ? { params: { include_events: "1" } } : {};
    return this.get(`/api/runners/runs/${runId}/`, params)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async cancelRun(runId: string, reason?: string): Promise<IAgentRun> {
    return this.post(`/api/runners/runs/${runId}/cancel/`, { reason: reason ?? "" })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async createRun(input: {
    workspace: string;
    prompt: string;
    run_config?: Record<string, unknown>;
    required_capabilities?: string[];
    work_item?: string;
    /** Optional pod override; defaults to issue.assigned_pod or workspace.default_pod. */
    pod?: string;
  }): Promise<IAgentRun> {
    return this.post("/api/runners/runs/", input)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listApprovals(): Promise<IApprovalRequest[]> {
    return this.get("/api/runners/approvals/")
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async decideApproval(approvalId: string, decision: TApprovalDecision): Promise<IApprovalRequest> {
    return this.post(`/api/runners/approvals/${approvalId}/decide/`, { decision })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listChatSessions(workspaceId: string, runnerId?: string): Promise<IAgentChatSession[]> {
    const params: Record<string, string> = { workspace: workspaceId };
    if (runnerId) params.runner = runnerId;
    return this.get("/api/runners/chat/sessions/", { params })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async createChatSession(input: {
    workspace: string;
    runner: string;
    model?: string;
    cwd?: string;
  }): Promise<IAgentChatSession> {
    return this.post("/api/runners/chat/sessions/", input)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async getChatSession(sessionId: string): Promise<IAgentChatSession> {
    return this.get(`/api/runners/chat/sessions/${sessionId}/`)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listChatMessages(sessionId: string): Promise<IAgentChatMessage[]> {
    return this.get(`/api/runners/chat/sessions/${sessionId}/messages/`)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async sendChatMessage(sessionId: string, content: string): Promise<IAgentChatMessage> {
    return this.post(`/api/runners/chat/sessions/${sessionId}/messages/`, {
      content,
      content_parts: [],
    })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async warmChatSession(sessionId: string): Promise<{ ok: boolean; skipped?: string }> {
    return this.post(`/api/runners/chat/sessions/${sessionId}/warm/`, {})
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async cancelChat(sessionId: string, reason?: string): Promise<{ ok: boolean }> {
    return this.post(`/api/runners/chat/sessions/${sessionId}/cancel/`, { reason: reason ?? "" })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async closeChat(sessionId: string): Promise<IAgentChatSession> {
    return this.post(`/api/runners/chat/sessions/${sessionId}/close/`, {})
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listChatApprovals(): Promise<IAgentChatApprovalRequest[]> {
    return this.get("/api/runners/chat/approvals/")
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async decideChatApproval(approvalId: string, decision: TApprovalDecision): Promise<IAgentChatApprovalRequest> {
    return this.post(`/api/runners/chat/approvals/${approvalId}/decide/`, { decision })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  chatEventsUrl(sessionId: string, after = 0): string {
    return `${this.baseURL}/api/runners/chat/sessions/${sessionId}/events/?after=${after}`;
  }
}
