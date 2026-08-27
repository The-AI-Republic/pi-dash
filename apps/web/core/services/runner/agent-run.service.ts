/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
// services
import { APIService } from "@/services/api.service";

/**
 * Run AI button dispatch — server renders the prompt from the issue's
 * phase template (``coding-task`` for In Progress, ``review`` for In
 * Review, default otherwise) via ``composer.build_first_turn``. The
 * client never sends a prompt body; the manual button produces the
 * same prompt a state transition / tick produces for that phase.
 *
 * Server-side wiring: see ``apps/api/pi_dash/runner/views/runs.py``
 * ``_post_run_ai`` (gated on ``triggered_by === "run_ai"``).
 */
export type TRunAiPayload = {
  workspace: string;
  work_item: string;
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

/**
 * Re-tick dispatch — re-grants a fresh phase-sized ticking budget to an
 * issue whose agent-ticker budget is exhausted, and re-arms the ticker.
 * The server enforces the guardrails (issue must be in a ticking state and
 * its budget exhausted); when they don't hold it responds ``granted:
 * false`` with a machine-readable ``reason`` rather than an error.
 *
 * Server-side wiring: ``apps/api/pi_dash/runner/views/runs.py``
 * ``AgentReTickEndpoint``.
 */
export type TReTickPayload = {
  work_item: string;
};

export type TReTickResponse = {
  granted: boolean;
  reason: string;
  tick_count?: number;
  max_ticks?: number;
  enabled?: boolean;
  next_run_at?: string | null;
};

export class AgentRunService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  /**
   * Dispatch a run for the "Run AI" button. Prompt is rendered server-
   * side from the issue's phase template — the client sends only the
   * workspace + issue ids.
   */
  async runAi(data: TRunAiPayload): Promise<TAgentRun> {
    return this.post(`/api/runners/runs/`, { ...data, triggered_by: "run_ai" })
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

  /**
   * Re-grant a fresh ticking budget to an exhausted issue ticker. No-op
   * (``granted: false``) when the issue is not in a ticking state or its
   * budget is not exhausted — the server decides, not the client.
   */
  async reTick(data: TReTickPayload): Promise<TReTickResponse> {
    return this.post(`/api/runners/re-tick/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data ?? error;
      });
  }

  /**
   * Abort an in-flight run. Signals the associated runner to stop the run
   * and marks the AgentRun terminal (``cancelled``) server-side. The server
   * enforces authorization and rejects an already-terminal run.
   *
   * Server-side wiring: ``apps/api/pi_dash/runner/views/runs.py``
   * ``AgentRunCancelEndpoint``.
   */
  async abortRun(runId: string, reason?: string): Promise<TAgentRun> {
    return this.post(`/api/runners/runs/${runId}/cancel/`, { reason: reason ?? "" })
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data ?? error;
      });
  }
}
