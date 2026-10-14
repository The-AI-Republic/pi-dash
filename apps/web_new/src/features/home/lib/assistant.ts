// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home assistant card helpers (SHELL-007; written from parity/specs/shell.md
// §2 and the §10 seam: the shell builds the card shell, prompt chips, the
// thread list read and the setup reminder, and links threads into the
// assistant view; the assistant area owns everything past the link).
import type { LlmConfig } from "@pidash/api-client";

export type AssistantCardState = "hidden" | "setup" | "ready";

/**
 * Whether the assistant can run: a model name and key are both set. The
 * blank default (empty name, no key) reads back as unconfigured rather
 * than missing (assistant.ts contract).
 */
export function isAssistantConfigured(
  config: Pick<LlmConfig, "model_name" | "has_api_key"> | null | undefined
): boolean {
  if (config === null || config === undefined) return false;
  return config.model_name.trim() !== "" && config.has_api_key;
}

/**
 * The card state: guests never see the card, members without a model see
 * the setup reminder, and configured members see prompts plus threads.
 */
export function selectAssistantCardState(canView: boolean, configured: boolean): AssistantCardState {
  if (!canView) return "hidden";
  return configured ? "ready" : "setup";
}

/** The assistant landing: where prompt chips and the setup reminder lead. */
export function assistantLandingHref(workspaceSlug: string): string {
  return `/${workspaceSlug}/assistant`;
}

/** One thread in the assistant view, by thread id. */
export function assistantThreadHref(workspaceSlug: string, threadId: string): string {
  return `/${workspaceSlug}/assistant/${threadId}`;
}

/**
 * Suggested prompts for the ready card. Fresh copy for the new app; the
 * assistant area owns what follows a pick, so chips lead to its landing.
 */
export const ASSISTANT_SUGGESTIONS: readonly string[] = [
  "Summarize my open work",
  "Draft this week's status update",
  "What should I pick up next?",
  "Help me plan the week",
];
