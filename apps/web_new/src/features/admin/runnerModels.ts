// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner model catalog page pure helpers (NEWPIDASH-169): the agent
// sections in picker order, the allowed effort values, the table copy,
// the reorder maths, and the move-button boundaries. The agent list
// stays in code (the catalog never adds agents).
import { isApiError, type RunnerModelRow } from "@pidash/api-client";

/** Agents in section order, with their display names. */
export const RUNNER_MODEL_AGENTS: readonly { agent: string; label: string }[] = [
  { agent: "claude-code", label: "Claude Code" },
  { agent: "codex", label: "Codex" },
  { agent: "cursor-agent", label: "Cursor" },
  { agent: "open-claw", label: "OpenClaw" },
  { agent: "grok", label: "Grok" },
  { agent: "muse-code", label: "Muse Code" },
];

/** Effort values offered by Codex models (the allowed set). */
export const RUNNER_MODEL_EFFORTS = ["low", "medium", "high", "xhigh"] as const;

/** Effort checkbox labels. */
export const RUNNER_MODEL_EFFORT_LABELS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
};

/** Page heading. */
export const RUNNER_MODELS_HEADING = "Runner models";

/** Page description. */
export const RUNNER_MODELS_DESCRIPTION = "LLM models the add-runner picker offers, one section per agent.";

/** The note under the description: deleting a model is safe. */
export const RUNNER_MODELS_DELETE_NOTE =
  "Deleting or disabling a model only removes it from the add-runner picker. Runners already using it keep their model.";

/** Table headers, left to right. */
export const RUNNER_MODELS_HEADERS: readonly string[] = [
  "Model",
  "Label",
  "Efforts",
  "Enabled",
  "Preselected",
  "Order",
  "Actions",
];

/** Empty-section message row. */
export const RUNNER_MODELS_EMPTY_TEXT = "No models for this agent.";

/** Repeat effort text for a row (em dash when none). */
export const RUNNER_MODELS_NO_EFFORTS_TEXT = "—";

/** Inline form errors for validation codes the server answers. */
export function formErrorMessage(error: unknown): string {
  if (isApiError(error)) {
    const code =
      typeof error.body === "object" && error.body !== null && "error" in error.body
        ? String((error.body as { error: unknown }).error)
        : "";
    switch (code) {
      case "unknown_agent":
        return "Unknown agent.";
      case "empty_model":
        return "The model name must not be empty.";
      case "empty_label":
        return "The label must not be empty.";
      case "invalid_effort":
        return "Efforts must be low, medium, high, or xhigh.";
      case "duplicate_effort":
        return "Each effort may appear once.";
      case "duplicate_model":
        return "This agent already has a model with that name.";
      case "not_found":
        return "That model no longer exists.";
    }
  }
  return "The save failed. Check the fields and try again.";
}

/** One agent's rows in picker order (sort, then model). */
export function rowsForAgent(rows: RunnerModelRow[], agent: string): RunnerModelRow[] {
  return rows
    .filter((row) => row.agent === agent)
    .sort((a, b) => a.sort_order - b.sort_order || a.model.localeCompare(b.model));
}

/** Effort summary for one cell. */
export function formatEfforts(row: RunnerModelRow): string {
  if (row.reasoning_efforts.length === 0) return RUNNER_MODELS_NO_EFFORTS_TEXT;
  return row.reasoning_efforts.map((effort) => RUNNER_MODEL_EFFORT_LABELS[effort] ?? effort).join(", ");
}

/** Next sort_order an appended row should take (after the last row). */
export function nextSortOrder(rows: RunnerModelRow[]): number {
  if (rows.length === 0) return 0;
  return Math.max(...rows.map((row) => row.sort_order)) + 1;
}

/**
 * Sort orders to swap two adjacent rows: the moved row takes its
 * neighbour's order and vice versa. Returns null when the move leaves
 * the list.
 */
export function swapSortOrders(
  rows: RunnerModelRow[],
  index: number,
  direction: "up" | "down"
): [number, number] | null {
  const neighbour = direction === "up" ? index - 1 : index + 1;
  if (neighbour < 0 || neighbour >= rows.length) return null;
  const current = rows[index];
  const other = rows[neighbour];
  if (current === undefined || other === undefined) return null;
  return [current.sort_order, other.sort_order];
}
