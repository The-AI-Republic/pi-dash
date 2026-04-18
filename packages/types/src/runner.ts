/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export type TRunnerStatus = "online" | "offline" | "busy" | "revoked";

export interface IRunner {
  id: string;
  name: string;
  status: TRunnerStatus;
  os: string;
  arch: string;
  runner_version: string;
  protocol_version: number;
  capabilities: string[];
  last_heartbeat_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface IRunnerRegistration {
  id: string;
  label: string;
  expires_at: string;
  consumed_at: string | null;
  created_at: string;
}

export interface IRegistrationTokenResult {
  registration: IRunnerRegistration;
  /** Plaintext code shown ONCE — copy it immediately. */
  token: string;
}

export type TAgentRunStatus =
  | "queued"
  | "assigned"
  | "running"
  | "awaiting_approval"
  | "awaiting_reauth"
  | "completed"
  | "failed"
  | "cancelled";

export const AGENT_RUN_TERMINAL_STATUSES: readonly TAgentRunStatus[] = ["completed", "failed", "cancelled"] as const;

export interface IAgentRunEvent {
  id: number;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface IAgentRun {
  id: string;
  status: TAgentRunStatus;
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
