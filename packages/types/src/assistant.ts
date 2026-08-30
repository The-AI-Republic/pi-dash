// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

// "notice" is a client-synthesized role (never persisted) used to render
// inline, non-fatal notices in the thread — e.g. a tool_servers_skipped event.
export type TAssistantMessageRole = "user" | "assistant" | "tool_call" | "tool_result" | "error" | "notice";
export type TAssistantMessageStatus = "streaming" | "completed" | "failed" | "cancelled";
export type TAssistantProviderKind = "openai_compatible" | "anthropic";

export interface IAssistantThread {
  id: string;
  title: string;
  is_archived: boolean;
  has_active_turn: boolean;
  created_at: string;
  updated_at: string;
}

export interface IAssistantLink {
  type: string;
  workspace_slug: string;
  project_id: string;
  issue_id: string;
  url_path: string;
}

export interface IAssistantMessage {
  id: string;
  role: TAssistantMessageRole;
  content: string;
  status: TAssistantMessageStatus;
  seq: number;
  turn_id: string | null;
  payload: { links?: IAssistantLink[] } & Record<string, unknown>;
  created_at: string;
  completed_at: string | null;
}

/** One MCP tool server that was dropped from a run, with a machine-readable reason. */
export interface IAssistantSkippedServer {
  name: string;
  reason: string;
}

/**
 * Payload of the `tool_servers_skipped` SSE event: the servers a run could not
 * use, so the UI can tell the user a capability was unavailable instead of
 * silently losing it.
 */
export interface IToolServersSkippedPayload {
  servers: IAssistantSkippedServer[];
}

export interface IAssistantEvent {
  id?: number;
  thread: string;
  message: string | null;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface IAssistantSendResponse {
  turn: { id: string; status: string };
  message: IAssistantMessage;
}

export interface IUserLLMConfig {
  provider_kind: TAssistantProviderKind;
  base_url: string;
  model_name: string;
  has_api_key: boolean;
  last_verified_at: string | null;
}

export interface IUserLLMConfigInput {
  provider_kind: TAssistantProviderKind;
  base_url?: string;
  model_name: string;
  api_key?: string;
}
