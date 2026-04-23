/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IAgentRun,
  IApprovalRequest,
  IRegistrationTokenResult,
  IRunner,
  IRunnerRegistration,
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

  async revoke(runnerId: string): Promise<IRunner> {
    return this.post(`/api/runners/${runnerId}/revoke/`)
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

  async listTokens(): Promise<IRunnerRegistration[]> {
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
}
