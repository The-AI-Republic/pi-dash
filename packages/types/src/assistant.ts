// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export type TAssistantMessageRole = "user" | "assistant" | "tool_call" | "tool_result" | "error";
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
