// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure rules for the install flows (parity/specs/agents.md §2, AGT-006,
// AGT-008, AGT-009): which definitions a project may install, how a set
// of per-target writes partitions, and the first failure worth quoting.

import type { Scheduler } from "@pidash/api-client";

/** Enabled definitions this project has not installed yet, list order kept. */
export function eligibleDefinitions(
  schedulers: readonly Scheduler[],
  installedSchedulerIds: readonly string[]
): Scheduler[] {
  const installed = new Set(installedSchedulerIds);
  return schedulers.filter((scheduler) => scheduler.is_enabled && !installed.has(scheduler.id));
}

export interface InstallOutcome {
  targetId: string;
  ok: boolean;
  /** The failure message quoted in the notice, when the write failed. */
  message: string;
}

export interface InstallPartition {
  succeeded: string[];
  failed: InstallOutcome[];
}

/** Split settled per-target writes into successes and failures (in order). */
export function partitionInstallOutcomes(
  targetIds: readonly string[],
  results: readonly PromiseSettledResult<unknown>[]
): InstallPartition {
  const succeeded: string[] = [];
  const failed: InstallOutcome[] = [];
  targetIds.forEach((targetId, index) => {
    const result = results[index];
    if (result !== undefined && result.status === "fulfilled") {
      succeeded.push(targetId);
      return;
    }
    const reason: unknown = result === undefined ? null : result.reason;
    failed.push({ targetId, ok: false, message: failureText(reason) });
  });
  return { succeeded, failed };
}

function failureText(reason: unknown): string {
  if (typeof reason === "object" && reason !== null) {
    const candidate = reason as { message?: unknown; fields?: Record<string, unknown> };
    const fields = candidate.fields;
    if (fields !== undefined) {
      for (const value of Object.values(fields)) {
        if (Array.isArray(value) && typeof value[0] === "string" && value[0] !== "") return value[0];
      }
    }
    if (typeof candidate.message === "string" && candidate.message.trim() !== "") return candidate.message;
  }
  if (typeof reason === "string" && reason !== "") return reason;
  return "The install was refused.";
}

/** Every failure message joined, in target order; empty when none failed. */
export function failureSummary(partition: InstallPartition): string {
  return partition.failed.map((failure) => failure.message).join("; ");
}

/** Notice title for the success half of a bulk install. */
export function bulkSuccessTitle(count: number): string {
  return count === 1 ? "Installed on 1 project" : `Installed on ${count} projects`;
}

/** Notice title for the failure half of a bulk install. */
export function bulkFailureTitle(count: number): string {
  return count === 1 ? "1 project failed" : `${count} projects failed`;
}

/** Selection summary the bulk picker trigger shows. */
export function bulkSelectionSummary(count: number): string {
  return count === 0 ? "Select projects" : count === 1 ? "1 project selected" : `${count} projects selected`;
}

/** Case-insensitive project search across the name and identifier. */
export function filterProjects<T extends { name: string; identifier?: string }>(
  projects: readonly T[],
  query: string
): T[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...projects];
  return projects.filter((project) => `${project.name} ${project.identifier ?? ""}`.toLowerCase().includes(needle));
}

/**
 * Bulk install path state. Already-installed targets are locked and count
 * as checked; a fresh selection starts empty.
 */
export interface BulkSelection {
  /** Explicitly picked ids (installed ones are never added here). */
  picked: string[];
  /** Installed ids detected when the dialog opened. */
  installed: string[];
}

export function toggleBulkTarget(selection: BulkSelection, projectId: string): BulkSelection {
  if (selection.installed.includes(projectId)) return selection;
  const picked = selection.picked.includes(projectId)
    ? selection.picked.filter((id) => id !== projectId)
    : [...selection.picked, projectId];
  return { ...selection, picked };
}

/** Select every unlocked target, or clear when all of them are picked. */
export function toggleAllBulkTargets(selection: BulkSelection, projectIds: readonly string[]): BulkSelection {
  const unlocked = projectIds.filter((id) => !selection.installed.includes(id));
  const allPicked = unlocked.length > 0 && unlocked.every((id) => selection.picked.includes(id));
  return { ...selection, picked: allPicked ? [] : unlocked };
}
