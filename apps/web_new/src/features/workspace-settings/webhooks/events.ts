// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook event selection (parity/specs/workspace-settings.md §19/§21,
// WSET-020/022). One of two modes — everything, or picked events — over
// the five delivery flags the API stores. All-events is not a stored
// flag: a row is in that mode exactly when every flag is on, so the
// detail form derives the mode from the row instead of trusting extra
// state.

import type { WorkspaceWebhook } from "@pidash/api-client";

export type WebhookEventFlag = "project" | "cycle" | "module" | "issue" | "issue_comment";
export type WebhookEventMode = "all" | "individual";
export type WebhookEventFlags = Record<WebhookEventFlag, boolean>;

export interface WebhookEventOption {
  flag: WebhookEventFlag;
  /** Toggle label in the create form and the detail form. */
  label: string;
  /** One line describing what deliveries the flag admits. */
  description: string;
}

/** Display order of the five individual toggles (§19). */
export const WEBHOOK_EVENT_OPTIONS: readonly WebhookEventOption[] = [
  { flag: "project", label: "Projects", description: "Project creation, updates and removal." },
  { flag: "cycle", label: "Cycles", description: "Cycle creation, updates and completion." },
  { flag: "issue", label: "Work items", description: "Work item creation, updates and state moves." },
  { flag: "module", label: "Modules", description: "Module creation, updates and membership changes." },
  {
    flag: "issue_comment",
    label: "Work item comments",
    description: "Comments added to work items.",
  },
];

export const ALL_EVENTS: WebhookEventFlags = {
  project: true,
  cycle: true,
  module: true,
  issue: true,
  issue_comment: true,
};

export const NO_EVENTS: WebhookEventFlags = {
  project: false,
  cycle: false,
  module: false,
  issue: false,
  issue_comment: false,
};

/** Fresh form state: everything selected, individual toggles all on (§19). */
export function initialEventSelection(): { mode: WebhookEventMode; flags: WebhookEventFlags } {
  return { mode: "all", flags: { ...ALL_EVENTS } };
}

/** The detail form's selection for a stored row (§21). */
export function eventSelectionFor(row: WorkspaceWebhook): { mode: WebhookEventMode; flags: WebhookEventFlags } {
  const flags: WebhookEventFlags = {
    project: row.project,
    cycle: row.cycle,
    module: row.module,
    issue: row.issue,
    issue_comment: row.issue_comment,
  };
  return { mode: usesAllEvents(flags) ? "all" : "individual", flags };
}

export function usesAllEvents(flags: WebhookEventFlags): boolean {
  return WEBHOOK_EVENT_OPTIONS.every((option) => flags[option.flag]);
}

/**
 * Switching to all-events turns every flag on, because the mode has no
 * separate storage. Switching back to individual keeps the last picks
 * unless there were none to keep (a stored all-on row), where every
 * toggle starts on (the create default).
 */
export function selectMode(selection: { mode: WebhookEventMode; flags: WebhookEventFlags }, mode: WebhookEventMode) {
  if (mode === selection.mode) return selection;
  if (mode === "all") return { mode, flags: { ...ALL_EVENTS } };
  const anyOn = WEBHOOK_EVENT_OPTIONS.some((option) => selection.flags[option.flag]);
  return { mode, flags: anyOn ? { ...selection.flags } : { ...ALL_EVENTS } };
}

export function toggleEvent(flags: WebhookEventFlags, flag: WebhookEventFlag): WebhookEventFlags {
  return { ...flags, [flag]: !flags[flag] };
}

/** The five flags a submit sends, whichever mode is showing (§19). */
export function submittedFlags(selection: { mode: WebhookEventMode; flags: WebhookEventFlags }): WebhookEventFlags {
  return selection.mode === "all" ? { ...ALL_EVENTS } : { ...selection.flags };
}

/** Trim what the person typed; an address is required (§19). */
export function normalizeWebhookUrl(raw: string): string {
  return raw.trim();
}

export const WEBHOOK_URL_REQUIRED = "URL is required";
