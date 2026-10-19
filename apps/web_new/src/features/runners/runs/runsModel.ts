// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs presentation helpers (NEWFRONT-545). Pure functions keep the
// view code small and give the diagnostics, executor labels, transcript
// narration and page parsing one tested definition each.

import type { BadgeTone } from "@pidash/kit";

export { parseRunsPage, runsHref, runsPageSearch } from "./runsPage.js";
import type { AgentRun, RunErrorDiagnostic, RunEvent } from "@pidash/api-client";

/** Footer text; only shown while the scope has runs. */
export function pageLabel(page: number, totalPages: number): string {
  return `Page ${page} of ${totalPages}`;
}

/** Executor badge copy; every non-cloud executor is the local runner. */
export function executorLabel(kind: AgentRun["executor_kind"]): string {
  return kind === "cloud_agent" ? "Cloud executor" : "Local runner";
}

export function executorTone(kind: AgentRun["executor_kind"]): BadgeTone {
  return kind === "cloud_agent" ? "accent" : "neutral";
}

/** The raw error text of a run, kept for the failure block. */
export function hasFailure(run: Pick<AgentRun, "error">): boolean {
  return run.error !== "";
}

const ERROR_KIND_LABELS: Readonly<Record<string, string>> = {
  agent_authentication: "Sign-in failed",
  agent_model_access: "Model unavailable",
  runner_registration: "Runner enrollment",
  runner_lifecycle: "Runner state",
  agent_stalled: "Agent made no progress",
  unknown: "Unclassified",
};

/** Human label for a diagnostic kind; unknown tokens read verbatim. */
export function errorKindLabel(kind: string): string {
  return ERROR_KIND_LABELS[kind] ?? kind;
}

const ERROR_SOURCE_TONES: Readonly<Record<RunErrorDiagnostic["source"], BadgeTone>> = {
  agent: "warning",
  pidash_runner: "accent",
  pidash_cloud: "accent",
  unknown: "neutral",
};

export function errorSourceTone(source: RunErrorDiagnostic["source"]): BadgeTone {
  return ERROR_SOURCE_TONES[source];
}

/**
 * Narrative text carried by an agent message event. Only non-blank text
 * expands inline; every other event stays a metadata row (RUN-018).
 */
export function eventNarrative(event: Pick<RunEvent, "kind" | "payload">): string | null {
  if (event.kind !== "agent/message") return null;
  const text = event.payload["text"];
  return typeof text === "string" && text.trim() !== "" ? text : null;
}

/** The cloud agent's plain-language summary above the raw result payload. */
export function resultSummary(run: Pick<AgentRun, "executor_kind" | "done_payload">): string | null {
  if (run.executor_kind !== "cloud_agent" || run.done_payload === null) return null;
  const summary = run.done_payload["summary"];
  return typeof summary === "string" && summary.trim() !== "" ? summary : null;
}

/** Result payload serialized for the raw block. */
export function resultJson(payload: Record<string, unknown>): string {
  return JSON.stringify(payload, null, 2);
}

/** Started time as a locale string; a run that never started shows a dash. */
export function startedLabel(run: Pick<AgentRun, "started_at" | "created_at">): string {
  const value = run.started_at ?? run.created_at;
  return new Date(value).toLocaleString();
}

/** Clock time for one event row. */
export function eventTime(iso: string): string {
  return new Date(iso).toLocaleTimeString();
}
