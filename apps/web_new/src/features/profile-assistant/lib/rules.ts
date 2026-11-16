// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Form rules for the AI-assistant tab (parity/specs/profile.md §10,
// PROF-021..023). These decide when the save, test and add controls
// enable, how a fresh config seeds the form, how a refused save is
// worded, and how the voice section reacts to the instance kill switch.

import type { LlmConfig, McpServer, SttConfig } from "@pidash/api-client";

import {
  DEFAULT_PROVIDER_KIND,
  LAST_VERIFIED_NEVER,
  LAST_VERIFIED_PREFIX,
  NAMED_PROVIDER_KIND,
  SAVE_FAILED_FALLBACK,
  SAVED_KEY_PLACEHOLDER,
  SERVER_PREFIX_PREFIX,
  UNKNOWN_ERROR_FALLBACK,
} from "./copy.js";

/** The provider a stored config names, defaulting when the read is blank. */
export function providerOf(config: Pick<LlmConfig, "provider_kind"> | undefined): string {
  const kind = config?.provider_kind ?? "";
  return kind === "" ? DEFAULT_PROVIDER_KIND : kind;
}

/** A named provider dials a fixed host, so the endpoint field is hidden. */
export function endpointVisible(provider: string): boolean {
  return provider !== NAMED_PROVIDER_KIND;
}

/** The endpoint a save posts for the current provider. */
export function endpointForSave(provider: string, baseUrl: string): string | undefined {
  return endpointVisible(provider) ? baseUrl.trim() : undefined;
}

/**
 * The language-model save needs a model and a credential to land: a typed
 * key, or the one already stored (an omitted key keeps it). It never
 * creates a config without a secret.
 */
export function llmCanSave(state: { model: string; key: string; hasKey: boolean }): boolean {
  return state.model.trim() !== "" && (state.key.trim() !== "" || state.hasKey);
}

/** The voice save mirrors that gate over its endpoint and model. */
export function sttCanSave(state: { baseUrl: string; model: string; key: string; hasKey: boolean }): boolean {
  return state.baseUrl.trim() !== "" && state.model.trim() !== "" && (state.key.trim() !== "" || state.hasKey);
}

/** The key a save posts: the typed value, or nothing to keep the stored one. */
export function keyForSave(key: string): string | undefined {
  const typed = key.trim();
  return typed === "" ? undefined : typed;
}

/** Test dials the stored key, so it waits for one. */
export function canTest(config: { has_api_key: boolean } | undefined): boolean {
  return config?.has_api_key === true;
}

/** A tool-server add needs a name and an endpoint. */
export function canAddServer(state: { name: string; url: string }): boolean {
  return state.name.trim() !== "" && state.url.trim() !== "";
}

/** The placeholder an empty key field shows: saved keys are replaced, not shown. */
export function keyPlaceholder(hasKey: boolean, emptyPlaceholder: string): string {
  return hasKey ? SAVED_KEY_PLACEHOLDER : emptyPlaceholder;
}

/** A verification stamp line, or null when the config was never verified. */
export function lastVerifiedLine(value: string | null | undefined): string | null {
  if (value === null || value === undefined || value === "") return null;
  const when = new Date(value);
  const stamp = Number.isNaN(when.getTime()) ? LAST_VERIFIED_NEVER : when.toLocaleString();
  return `${LAST_VERIFIED_PREFIX} ${stamp}`;
}

/**
 * The message a refused save shows: the server's detail or error code when
 * the body carries one, else the generic line. Reads both the normalized
 * client error body and a raw transport-shaped body.
 */
export function saveFailureMessage(error: unknown): string {
  const body = (error as { body?: unknown } | null)?.body;
  if (body !== null && typeof body === "object") {
    const record = body as Record<string, unknown>;
    for (const key of ["detail", "error"]) {
      const value = record[key];
      if (typeof value === "string" && value.trim() !== "") return value;
    }
  }
  return SAVE_FAILED_FALLBACK;
}

/** The same server message, or the caller's own fallback when the body has none. */
export function serverFailureMessage(error: unknown, fallback: string): string {
  const message = saveFailureMessage(error);
  return message === SAVE_FAILED_FALLBACK ? fallback : message;
}

/** The code a failed connection test reports, or the generic fallback. */
export function testFailureCode(result: { error_code?: string | undefined }): string {
  const code = result.error_code ?? "";
  return code.trim() === "" ? UNKNOWN_ERROR_FALLBACK : code;
}

/**
 * The voice section's shape from the stored config and the read's switch
 * state. A backend without the switch (`enabled` absent) is treated as on;
 * with the switch off the section disappears unless a key is already saved,
 * and then only the remove control remains.
 */
export type VoiceSection = { kind: "hidden" } | { kind: "removal-only" } | { kind: "editable" };

export function voiceSection(config: Pick<SttConfig, "has_api_key" | "enabled"> | undefined): VoiceSection {
  const configurable = config?.enabled !== false;
  if (configurable) return { kind: "editable" };
  return config.has_api_key ? { kind: "removal-only" } : { kind: "hidden" };
}

/** The tool prefix a row claims while enabled. */
export function serverPrefix(server: Pick<McpServer, "effective_tool_prefix" | "tool_prefix">): string | null {
  const prefix = server.effective_tool_prefix ?? server.tool_prefix;
  return prefix === "" ? null : `${prefix}_*`;
}

/** The prefix sentence beside an enabled row. */
export function serverPrefixLine(server: Pick<McpServer, "effective_tool_prefix" | "tool_prefix">): string | null {
  const prefix = serverPrefix(server);
  return prefix === null ? null : `${SERVER_PREFIX_PREFIX} ${prefix}`;
}
