/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export type TRunnerStatus = "online" | "offline" | "busy" | "revoked";

export interface IPodMini {
  id: string;
  name: string;
  is_default: boolean;
  /** Project FK uuid that owns this pod. */
  project: string;
  /** Project's human-friendly identifier (slug), e.g. ``PDASHOSS01``. */
  project_identifier: string;
}

/** Identity of the dev machine a runner is bound to, surfaced on the
 * runner detail page. ``null`` on legacy ``pidash connect`` runners with
 * no dev_machine FK. */
export interface IDevMachineMini {
  id: string;
  host_label: string;
  label: string;
}

export interface IPod {
  id: string;
  name: string;
  description: string;
  is_default: boolean;
  workspace: string;
  /** Project FK uuid that owns this pod. */
  project: string;
  /** Project's human-friendly identifier (slug), e.g. ``BROWSERXTE``. */
  project_identifier: string;
  created_by: string | null;
  runner_count: number;
  created_at: string;
  updated_at: string;
}

/** Per-active-run agent observability snapshot.
 *
 * All fields nullable; ``null`` is the canonical "unknown" sentinel.
 * The activity badge is derived client-side from ``last_event_at`` +
 * ``agent_subprocess_alive`` + ``approvals_pending`` — there is no
 * server-side ``agent_state`` enum to keep coherent.
 *
 * See ``.ai_design/runner_agent_bridge/design.md`` §4.5.4. */
export interface IRunnerLiveState {
  observed_run_id: string | null;
  last_event_at: string | null;
  last_event_kind: string | null;
  last_event_summary: string | null;
  agent_pid: number | null;
  agent_subprocess_alive: boolean | null;
  approvals_pending: number | null;
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  llm_model: string | null;
  turn_count: number | null;
  updated_at: string;
}

export interface IDevMachine {
  id: string;
  host_label: string;
  label: string;
  visibility: number;
  runner_count: number;
  online_runner_count: number;
  /** True when the machine's control session polled recently, i.e. the
   * daemon can execute cloud-pushed commands (create runner) right now. */
  control_online: boolean;
  last_seen_at: string | null;
  last_heartbeat_at: string | null;
  revoked_at: string | null;
  created_at: string;
  updated_at: string;
}

/** Body of ``POST /api/runners/dev-machines/<mid>/create-runner/``. */
export interface ICreateRunnerOnMachineRequest {
  project: string;
  pod?: string;
  name?: string;
  working_dir?: string;
  agent?: string;
  model?: string;
  reasoning_effort?: string;
}

/** Daemon-reported outcome of a cloud-driven runner creation. */
export interface ICreateRunnerOnMachineStatus {
  request_id: string;
  status: "pending" | "ok" | "error";
  runner_id?: string;
  runner_name?: string;
  error?: string;
}

export interface IRunnerDevMetadata {
  /** Absolute path reported by the runner at session-open. */
  working_dir?: string;
}

export interface IRunner {
  id: string;
  name: string;
  status: TRunnerStatus;
  os: string;
  arch: string;
  runner_version: string;
  /** Extensible metadata reported by the runner's local development environment. */
  dev_metadata: IRunnerDevMetadata;
  protocol_version: number;
  capabilities: string[];
  last_heartbeat_at: string | null;
  owner: string | null;
  pod: string;
  pod_detail: IPodMini | null;
  /** Dev machine this runner runs on. ``null`` on legacy runners. */
  dev_machine_detail: IDevMachineMini | null;
  /** Connection that owns this runner. Required post-refactor. */
  connection: string;
  /** Volatile per-active-run agent snapshot. Optional / null when the
   * runner has not yet reported any observability data (pre-flag runner). */
  live_state?: IRunnerLiveState | null;
  /** Set the first time the daemon successfully exchanges its enrollment
   * token for a refresh token. Null = PENDING (still needs to enroll). */
  enrolled_at: string | null;
  /** Set when the runner is hard-revoked (manual, replay, membership
   * change). The row stays visible for history; attach a new runner
   * from the target machine with `pidash runner add`. */
  revoked_at: string | null;
  revoked_reason: string;
  created_at: string;
  updated_at: string;
}

export type TAgentRunStatus =
  | "queued"
  | "assigned"
  | "waiting_for_worktree"
  | "running"
  | "cancel_requested"
  | "awaiting_approval"
  | "awaiting_reauth"
  | "paused_awaiting_input"
  | "blocked"
  | "completed"
  | "failed"
  | "cancelled";

export const AGENT_RUN_TERMINAL_STATUSES: readonly TAgentRunStatus[] = [
  "blocked",
  "completed",
  "failed",
  "cancelled",
] as const;

export interface IAgentRunEvent {
  id: number;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export type TAgentRunErrorSource = "agent" | "pidash_runner" | "pidash_cloud" | "unknown";

export interface IAgentRunErrorDiagnostic {
  source: TAgentRunErrorSource;
  source_label: string;
  kind: string;
  summary: string;
  action: string;
}

export interface IAgentRun {
  id: string;
  status: TAgentRunStatus;
  prompt: string;
  thread_id: string;
  runner: string | null;
  work_item: string | null;
  pod: string;
  pod_detail: IPodMini | null;
  /** Run creator — the access principal for permission checks. */
  created_by: string;
  /** Billable party — the runner's owner once assigned. NULL until assignment. */
  owner: string | null;
  created_at: string;
  assigned_at: string | null;
  started_at: string | null;
  ended_at: string | null;
  done_payload: Record<string, unknown> | null;
  error: string;
  error_diagnostic: IAgentRunErrorDiagnostic | null;
  llm_model: string;
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  /** Place in the runner's local worktree queue while
   * ``waiting_for_worktree``; ``null`` otherwise (display only). */
  queue_position?: number | null;
  events?: IAgentRunEvent[];
}

/** One page of agent runs from `GET /api/runners/runs/`. */
export interface IAgentRunPage {
  results: IAgentRun[];
  /** Number of items on this page. */
  count: number;
  /** Total items across all pages. */
  total_count: number;
  /** Total number of pages (at least 1, even when empty). */
  total_pages: number;
  /** Current 1-based page. */
  page: number;
  per_page: number;
}

export type TApprovalKind = "command_execution" | "file_change" | "network_access" | "other";
export type TApprovalDecision = "accept" | "decline" | "accept_for_session";
export type TApprovalStatus = "pending" | "accepted" | "declined" | "expired";

export interface IApprovalRequest {
  id: string;
  agent_run: string;
  kind: TApprovalKind;
  payload: Record<string, unknown>;
  reason: string;
  status: TApprovalStatus;
  decision_source: string;
  requested_at: string;
  decided_at: string | null;
  expires_at: string | null;
}

export type TAgentChatSessionStatus = "open" | "closed" | "failed";
export type TAgentChatMessageRole = "user" | "assistant" | "tool" | "system";
export type TAgentChatMessageStatus = "queued" | "sent" | "streaming" | "completed" | "failed" | "cancelled";

export interface IAgentChatSession {
  id: string;
  workspace: string;
  runner: string;
  runner_detail?: IRunner | null;
  created_by: string;
  pod: string;
  status: TAgentChatSessionStatus;
  agent_kind: string;
  local_thread_id: string;
  local_session_id: string;
  cwd: string;
  model: string;
  active_turn_id: string;
  active_message_id: string | null;
  close_requested: boolean;
  last_message_at: string | null;
  closed_at: string | null;
  error: string;
  created_at: string;
  updated_at: string;
}

export interface IAgentChatMessage {
  id: string;
  session: string;
  role: TAgentChatMessageRole;
  content: string;
  content_parts: unknown[];
  status: TAgentChatMessageStatus;
  local_item_id: string;
  local_turn_id: string;
  seq: number;
  created_at: string;
  completed_at: string | null;
}

export interface IAgentChatEvent {
  id: number;
  session: string;
  message: string | null;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface IAgentChatApprovalRequest {
  id: string;
  session: string;
  local_approval_id: string;
  kind: TApprovalKind;
  payload: Record<string, unknown>;
  reason: string;
  status: TApprovalStatus;
  decision_source: string;
  decided_by: string | null;
  requested_at: string;
  expires_at: string | null;
  decided_at: string | null;
}
