/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pure helpers backing {@link InstallSchedulerOnProjectsModal}. Kept free of
 * React/MobX so the selection and failure-handling logic can be unit-tested
 * without a DOM.
 */

/** Shape of the error body `SchedulerService.createBinding` rejects with (DRF serializer errors). */
type BindingErrorShape = {
  error?: string;
  rrule?: string[];
  dtstart?: string[];
  tzid?: string[];
  scheduler?: string[];
  pod?: string[];
} | null;

/**
 * Pull a human-readable detail out of a rejected `createBinding` reason,
 * mirroring the field precedence used by the single-project install modal.
 * Returns null when nothing usable is present so callers can fall back to a
 * generic message.
 */
export function extractBindingError(reason: unknown): string | null {
  const err = reason as BindingErrorShape;
  return (
    err?.error ?? err?.rrule?.[0] ?? err?.dtstart?.[0] ?? err?.tzid?.[0] ?? err?.scheduler?.[0] ?? err?.pod?.[0] ?? null
  );
}

export type InstallPartition = {
  succeededIds: string[];
  failedIds: string[];
  /** First surfaced backend detail across the failures, or null if none. */
  firstError: string | null;
};

/**
 * Split a `Promise.allSettled` result set (aligned positionally with
 * `targetIds`) into succeeded vs failed project ids, capturing the first
 * backend error detail so the failure toast can say *why*, not just *which*.
 */
export function partitionInstallResults(
  targetIds: readonly string[],
  results: readonly PromiseSettledResult<unknown>[]
): InstallPartition {
  const succeededIds: string[] = [];
  const failedIds: string[] = [];
  let firstError: string | null = null;

  results.forEach((res, i) => {
    const pid = targetIds[i];
    if (res.status === "fulfilled") {
      succeededIds.push(pid);
    } else {
      failedIds.push(pid);
      if (firstError === null) firstError = extractBindingError(res.reason);
    }
  });

  return { succeededIds, failedIds, firstError };
}

/** Project fields the picker filters on. */
type FilterableProject = {
  name: string;
  identifier?: string | null;
};

/**
 * Case-insensitive filter over project name and identifier. An empty/whitespace
 * query returns the input list unchanged.
 */
export function filterProjects<T extends FilterableProject>(projects: readonly T[], query: string): T[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...projects];
  return projects.filter((p) => p.name.toLowerCase().includes(q) || (p.identifier ?? "").toLowerCase().includes(q));
}
