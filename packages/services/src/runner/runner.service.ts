/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@apple-pi-dash/constants";
import { APIService } from "../api.service";

export interface IRunner {
  id: string;
  name: string;
  status: "online" | "offline" | "busy" | "revoked";
  os: string;
  arch: string;
  runner_version: string;
  protocol_version: number;
  capabilities: string[];
  last_heartbeat_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface IRegistrationTokenResult {
  registration: {
    id: string;
    label: string;
    expires_at: string;
    consumed_at: string | null;
    created_at: string;
  };
  /** Plaintext code shown ONCE — copy it immediately. */
  token: string;
}

export interface IAgentRun {
  id: string;
  status: string;
  prompt: string;
  thread_id: string;
  runner: string | null;
  work_item: string | null;
  created_at: string;
  assigned_at: string | null;
  started_at: string | null;
  ended_at: string | null;
  done_payload: Record<string, unknown> | null;
  error: string;
  events?: IAgentRunEvent[];
}

export interface IAgentRunEvent {
  id: number;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface IApprovalRequest {
  id: string;
  agent_run: string;
  kind: "command_execution" | "file_change" | "network_access" | "other";
  payload: Record<string, unknown>;
  reason: string;
  status: "pending" | "accepted" | "declined" | "expired";
  decision_source: string;
  requested_at: string;
  decided_at: string | null;
  expires_at: string | null;
}

/**
 * Apple Pi Dash runner web-app API client. Session-authenticated; mounted at
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

  async revoke(runnerId: string): Promise<IRunner> {
    return this.post(`/api/runners/${runnerId}/revoke/`)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async listTokens(): Promise<IRegistrationTokenResult["registration"][]> {
    return this.get("/api/runners/tokens/")
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async mintToken(workspaceId: string, label?: string): Promise<IRegistrationTokenResult> {
    return this.post("/api/runners/tokens/", { workspace: workspaceId, label: label ?? "" })
      .then((r) => r?.data)
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

  async decideApproval(
    approvalId: string,
    decision: "accept" | "decline" | "accept_for_session"
  ): Promise<IApprovalRequest> {
    return this.post(`/api/runners/approvals/${approvalId}/decide/`, { decision })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }
}
