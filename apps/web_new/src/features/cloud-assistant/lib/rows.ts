// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connection row model (parity/specs/cloud.md §7, CLOUD-032–034, 036). Turns
// the server's connection list into what the tab renders: a kind-appropriate
// title, the markers each row carries, which control edits its model, and
// whether the row can be selected at all. Pure on purpose — every branch is
// unit-testable without a DOM or a server.

import {
  resolveOpenhubAvailable,
  type AssistantConnectionKind,
  type AssistantConnectionList,
  type AssistantConnectionListItem,
} from "@pidash/api-client";

import {
  IN_USE_MARKER,
  INCLUDED_MARKER,
  NOT_CONNECTED_MARKER,
  PLAN_CONNECTED_MARKER,
  PLAN_SELECTED_MARKER,
} from "./copy.js";

/**
 * Managed-lane model list used when the server sends none (CLOUD-032). The
 * choices mirror the models the product's own runner catalog offers, so a
 * pick saved here is one the assistants can actually run.
 */
export const BUILTIN_CODEX_MODELS = ["gpt-6-astra", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"] as const;

/** Short hints for the built-in managed models, shown beside each choice. */
export const BUILTIN_CODEX_MODEL_DESCRIPTIONS: Readonly<Record<string, string>> = {
  "gpt-6-astra": "Most capable model",
  "gpt-5.6-sol": "Deep reasoning for hard problems",
  "gpt-5.6-terra": "Everyday default",
  "gpt-5.6-luna": "Quickest answers",
  "gpt-5.5": "Older generation",
};

/** Hints for the hosted-lane models the product names by default. */
export const OPENHUB_MODEL_DESCRIPTIONS: Readonly<Record<string, string>> = {
  "moonshotai/kimi-k3": "Balanced hosted model",
  "deepseek/deepseek-v4-pro": "Deepest reasoning, costs more",
  "deepseek/deepseek-v4-flash": "Lowest cost",
};

/**
 * Free-text suggestions for self-supplied keys: one entry per provider
 * family the product documents, so the field hints at the slug format
 * without pretending to know the user's catalog.
 */
export const BYOK_MODEL_SUGGESTIONS = [
  "gpt-5.5",
  "claude-sonnet-4-6",
  "deepseek-chat",
  "qwen-2.5-72b-instruct",
] as const;

/** Marker text for the active row. */
export const ACTIVE_MARKER = IN_USE_MARKER;

export type ModelControlKind = "select" | "input" | "hidden";

export interface ModelChoice {
  /** Value sent as the model name. */
  value: string;
  /** What the picker shows; the provider's own label when it sent one. */
  label: string;
}

export interface ConnectionRow {
  id: string;
  kind: AssistantConnectionKind;
  title: string;
  selected: boolean;
  /** False only for the provider-account row while no account backs it. */
  selectable: boolean;
  markers: string[];
  /** Rows whose session predates the hosted lane's permissions. */
  stale: boolean;
  /** True only when the row draws on the ChatGPT plan (guards the manage-usage link). */
  usesChatgptPlan: boolean;
  modelControl: ModelControlKind;
  modelName: string;
  modelChoices: ModelChoice[];
  modelSuggestions: readonly string[];
  verifiedAt: string | null;
  /** The provider account has been disconnected; the row stays. */
  disconnected: boolean;
  /** The row cannot be removed (managed lanes are permanent). */
  removable: boolean;
  /** The row can be disconnected (a connected provider account). */
  disconnectable: boolean;
}

export interface ConnectionModelFlags {
  codexModels: string[];
  codexModelLabels: Record<string, string>;
  openhubModels: string[];
}

/** True when the provider-account row has no account behind it. */
export function isDisconnected(row: AssistantConnectionListItem): boolean {
  return row.kind === "codex" && !row.connected;
}

/**
 * The title a row shows. Managed lanes are named after the product; a
 * self-supplied key is named after where it points, because the wire
 * protocol says nothing a user recognises. An unparseable address falls
 * back to the provider family, never to raw input.
 */
export function connectionTitle(row: Pick<AssistantConnectionListItem, "kind" | "provider_kind" | "base_url">): string {
  if (row.kind === "openhub") return "OpenHub";
  if (row.kind === "codex") return "OpenAI account";
  const host = hostOf(row.base_url);
  if (host !== null) return `BYOK:${host}`;
  return row.provider_kind === "anthropic" ? "BYOK:Anthropic" : "BYOK";
}

function hostOf(raw: string): string | null {
  const value = raw.trim();
  if (value === "") return null;
  try {
    const url = new URL(value);
    return url.host === "" ? null : url.host;
  } catch {
    return null;
  }
}

/**
 * Managed pickers always keep the stored model selectable: a choice saved
 * before the account's catalog changed stays visible (and valid) until the
 * user reconnects. Provider labels win over descriptions.
 */
export function modelChoices(
  models: readonly string[],
  labels: Readonly<Record<string, string>>,
  descriptions: Readonly<Record<string, string>>,
  current: string
): ModelChoice[] {
  const values = current !== "" && !models.includes(current) ? [...models, current] : [...models];
  return values.map((value) => {
    const label = labels[value] ?? descriptions[value];
    return label === undefined ? { value, label: value } : { value, label };
  });
}

/** A hosted row on a lane the session's permissions cannot reach. */
export function isStaleHostedRow(row: AssistantConnectionListItem, openhubAvailable: boolean): boolean {
  return row.kind === "openhub" && !openhubAvailable;
}

export function toConnectionRow(
  row: AssistantConnectionListItem,
  list: AssistantConnectionList,
  flags: ConnectionModelFlags
): ConnectionRow {
  const openhubAvailable = resolveOpenhubAvailable(list);
  const disconnected = isDisconnected(row);
  const stale = isStaleHostedRow(row, openhubAvailable);
  const markers: string[] = [];
  if (disconnected) markers.push(NOT_CONNECTED_MARKER);
  if (row.uses_chatgpt_plan) markers.push(row.is_selected ? PLAN_SELECTED_MARKER : PLAN_CONNECTED_MARKER);
  if (row.kind === "openhub") markers.push(INCLUDED_MARKER);
  if (row.is_selected) markers.push(IN_USE_MARKER);

  const control = modelControlFor(row, disconnected, stale);
  const modelName = row.model_name;
  return {
    id: row.id,
    kind: row.kind,
    title: connectionTitle(row),
    selected: row.is_selected,
    selectable: !disconnected && !stale,
    markers,
    stale,
    usesChatgptPlan: row.uses_chatgpt_plan,
    modelControl: control,
    modelName,
    modelChoices: control === "select" ? choicesFor(row, flags, modelName) : [],
    modelSuggestions: control === "input" ? BYOK_MODEL_SUGGESTIONS : [],
    verifiedAt: row.last_verified_at,
    disconnected,
    removable: row.kind === "byok",
    disconnectable: row.kind === "codex" && !disconnected,
  };
}

/** Stale and disconnected rows keep their model but offer no control. */
function modelControlFor(row: AssistantConnectionListItem, disconnected: boolean, stale: boolean): ModelControlKind {
  if (disconnected || stale) return "hidden";
  return row.kind === "byok" ? "input" : "select";
}

function choicesFor(row: AssistantConnectionListItem, flags: ConnectionModelFlags, current: string): ModelChoice[] {
  if (row.kind === "openhub") {
    return modelChoices(flags.openhubModels, {}, OPENHUB_MODEL_DESCRIPTIONS, current);
  }
  const models = flags.codexModels.length > 0 ? flags.codexModels : [...BUILTIN_CODEX_MODELS];
  return modelChoices(models, flags.codexModelLabels, BUILTIN_CODEX_MODEL_DESCRIPTIONS, current);
}

/** Managed model flags for a list answer, defaulting each field. */
export function connectionModelFlags(list: AssistantConnectionList): ConnectionModelFlags {
  return {
    codexModels: list.codex_model_options,
    codexModelLabels: list.codex_model_labels,
    openhubModels: list.openhub_model_options,
  };
}
