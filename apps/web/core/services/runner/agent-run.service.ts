/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
// services
import { APIService } from "@/services/api.service";

export type TCreateAgentRunPayload = {
  workspace: string;
  prompt: string;
  work_item?: string;
  pod?: string;
  run_config?: Record<string, unknown>;
  required_capabilities?: string[];
};

/**
 * Comment & Run dispatch — reuses the continuation pipeline (parent
 * resolution, runner pinning, drain). The just-posted comment lives in
 * IssueComment; the prompt is rebuilt from issue + comments at dispatch
 * time, so no `prompt` body is required.
 *
 * Server-side wiring: see ``apps/api/pi_dash/runner/views/runs.py``
 * ``_post_comment_and_run`` (gated on ``triggered_by === "comment_and_run"``).
 */
export type TCommentAndRunPayload = {
  workspace: string;
  work_item: string;
};

export type TAgentRun = {
  id: string;
  workspace: string;
  status: string;
  prompt: string;
  work_item?: string | null;
  created_at: string;
  // additional fields exist on the server response but are not typed here
  // because the UI does not consume them yet.
};

export class AgentRunService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async createAgentRun(data: TCreateAgentRunPayload): Promise<TAgentRun> {
    return this.post(`/api/runners/runs/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data ?? error;
      });
  }

  /**
   * Dispatch a continuation run for the Comment & Run flow. The just-
   * posted comment must already exist on the issue.
   */
  async commentAndRun(data: TCommentAndRunPayload): Promise<TAgentRun> {
    return this.post(`/api/runners/runs/`, { ...data, triggered_by: "comment_and_run" })
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data ?? error;
      });
  }
}
