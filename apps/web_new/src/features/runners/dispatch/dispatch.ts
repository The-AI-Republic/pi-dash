// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run dispatch service (NEWFRONT-547, RUN-046/RUN-048). The issues area
// mounts the controls; this module owns the dispatch itself: desktop
// preparation through the runtime service, the Run AI and Comment & Run
// posts, the abort write, and the grant-or-refuse re-tick. Executor
// selection stays read-only — dispatch never writes it.

import {
  cancelRun,
  commentAndRun,
  reTickIssue,
  runAi,
  type AgentRun,
  type AgentRunCreateResult,
  type AgentRunQueued,
  type ApiClient,
  type ReTickResponse,
} from "@pidash/api-client";

import type { AgentRuntimeApi } from "../../../core/platform/index.js";
import { prepareAgentRun, type HeartbeatPoll } from "../runtime/service.js";

/** Which server pipeline renders the run's prompt. */
export type DispatchMode = "run_ai" | "comment_and_run";

export interface TriggerAgentRunInput {
  client: ApiClient;
  /** Absent on web and cloud, where preparation makes no calls. */
  runtime?: AgentRuntimeApi | undefined;
  workspaceId: string;
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  mode: DispatchMode;
  heartbeatPoll?: HeartbeatPoll | undefined;
}

/** True when the dispatch queued behind an already-active run. */
export function isQueuedDispatch(result: AgentRunCreateResult): result is AgentRunQueued {
  return "queued" in result && result.queued === true;
}

/**
 * Start an agent run on a work item. On desktop the managed runner is
 * prepared first (connected, daemon awaited); everywhere else the run
 * posts at once. The client sends only ids plus the trigger — prompt
 * rendering and executor choice stay server-side.
 */
export async function triggerAgentRun(input: TriggerAgentRunInput): Promise<AgentRunCreateResult> {
  const { client, runtime, workspaceId, workspaceSlug, projectId, issueId, mode, heartbeatPoll } = input;
  await prepareAgentRun({ client, runtime, target: { workspaceSlug, projectId, issueId }, heartbeatPoll });
  const payload = { workspace: workspaceId, work_item: issueId };
  return mode === "run_ai" ? runAi(client, payload) : commentAndRun(client, payload);
}

/**
 * Abort an in-flight run from an issue card. Same endpoint as the runs
 * UI cancel; the server rejects already-terminal runs.
 */
export async function abortAgentRun(client: ApiClient, runId: string, reason?: string): Promise<AgentRun> {
  return cancelRun(client, runId, reason);
}

/**
 * Ask the server to re-grant an issue's run budget. The answer is a
 * grant or a machine-readable refusal — never an error for a "no".
 */
export async function requestReTick(client: ApiClient, issueId: string): Promise<ReTickResponse> {
  return reTickIssue(client, { work_item: issueId });
}
