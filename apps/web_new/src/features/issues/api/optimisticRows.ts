// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optimistic row painting for single-issue property writes
// (parity/specs/issues.md §18, ISS-225). One issue's fields land inside
// any cached shape — a detail object, a bare row list, a flat list
// envelope, or a grouped envelope whose buckets hold rows — before the
// request resolves. When the grouped envelope names a dimension the
// patch changes (state, priority, cycle, project, creator), the row also
// moves to the bucket its new value belongs to, so a grouped list
// re-buckets instead of waiting for the refetch. Shapes the module does
// not recognise pass through untouched.

const GROUP_KEY_FIELD: Record<string, keyof OptimisticPatch> = {
  state_id: "state_id",
  priority: "priority",
  cycle_id: "cycle_id",
  project_id: "project_id",
  created_by: "created_by",
};

/** Fields a paint may carry; anything else is dropped from the bucket move. */
export interface OptimisticPatch {
  name?: unknown;
  description_html?: unknown;
  state_id?: unknown;
  priority?: unknown;
  assignee_ids?: unknown;
  label_ids?: unknown;
  estimate_point?: unknown;
  start_date?: unknown;
  target_date?: unknown;
  cycle_id?: unknown;
  module_ids?: unknown;
  parent_id?: unknown;
  sort_order?: unknown;
  project_id?: unknown;
  created_by?: unknown;
  subscribed?: unknown;
}

const NONE_GROUP_KEY = "None";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function withRowFields(
  row: unknown,
  issueId: string,
  patch: Record<string, unknown>,
  knownFieldsOnly: boolean
): unknown {
  if (!isRecord(row) || row.id !== issueId) return row;
  if (!knownFieldsOnly) return { ...row, ...patch };
  // A list row carries only the fields its read returns; painting a
  // detail-only field onto it would invent data the row never had.
  const allowed = Object.fromEntries(Object.entries(patch).filter(([field]) => field in row));
  return { ...row, ...allowed };
}

function groupKeyOf(value: unknown): string | null {
  if (value === null || value === undefined || value === "") return NONE_GROUP_KEY;
  return typeof value === "string" || typeof value === "number" ? String(value) : null;
}

/**
 * The bucket key a patched row belongs in, or null when the envelope's
 * dimension is not one this patch can resolve. Multi-value dimensions
 * (labels, assignees, modules) legitimately hold a row in several
 * buckets, and workflow-group grouping needs the state table, so those
 * stay where they are and the settle refetch corrects them.
 */
export function optimisticGroupKey(groupedBy: unknown, patch: Record<string, unknown>): string | null {
  if (typeof groupedBy !== "string") return null;
  const field = GROUP_KEY_FIELD[groupedBy];
  if (field === undefined || !(field in patch)) return null;
  return groupKeyOf(patch[field]);
}

interface BucketRow {
  id?: unknown;
}

function moveWithinBuckets(
  buckets: Record<string, unknown>,
  issueId: string,
  patch: Record<string, unknown>,
  groupedBy: unknown,
  knownFieldsOnly: boolean
): Record<string, unknown> | null {
  const target = optimisticGroupKey(groupedBy, patch);
  if (target === null) return null;
  let sourceKey: string | null = null;
  let sourceRows: unknown[] = [];
  for (const [key, bucket] of Object.entries(buckets)) {
    if (!isRecord(bucket) || !Array.isArray(bucket.results)) continue;
    if (bucket.results.some((row) => isRecord(row) && row.id === issueId)) {
      sourceKey = key;
      sourceRows = bucket.results;
      break;
    }
  }
  if (sourceKey === null || sourceKey === target) return null;
  const next: Record<string, unknown> = { ...buckets };
  const remaining: unknown[] = [];
  let moved: BucketRow = {};
  for (const row of sourceRows) {
    if (isRecord(row) && row.id === issueId) moved = row;
    else remaining.push(row);
  }
  const source = buckets[sourceKey] as Record<string, unknown>;
  if (remaining.length === 0) delete next[sourceKey];
  else {
    next[sourceKey] = {
      ...source,
      results: remaining,
      total_results: Math.max(0, Number(source.total_results ?? 1) - 1),
    };
  }
  const previous = isRecord(next[target]) ? (next[target] as Record<string, unknown>) : null;
  const rows = previous !== null && Array.isArray(previous.results) ? previous.results : [];
  next[target] = {
    ...(previous ?? {}),
    results: [...rows, withRowFields(moved, issueId, patch, knownFieldsOnly)],
    total_results: Number(previous?.total_results ?? 0) + 1,
  };
  return next;
}

function withBucketFields(
  bucket: unknown,
  issueId: string,
  patch: Record<string, unknown>,
  knownFieldsOnly: boolean
): unknown {
  if (!isRecord(bucket) || !Array.isArray(bucket.results)) return bucket;
  return { ...bucket, results: bucket.results.map((row) => withRowFields(row, issueId, patch, knownFieldsOnly)) };
}

/** Patch one issue's fields inside any cached shape. */
export function withIssueFields(
  data: unknown,
  issueId: string,
  patch: Record<string, unknown>,
  options: { rebucket?: boolean; knownFieldsOnly?: boolean } = {}
): unknown {
  const knownFieldsOnly = options.knownFieldsOnly === true;
  if (Array.isArray(data)) return data.map((row) => withRowFields(row, issueId, patch, knownFieldsOnly));
  if (!isRecord(data)) return data;
  if (typeof data.id === "string") return withRowFields(data, issueId, patch, knownFieldsOnly);
  if (Array.isArray(data.results)) {
    return { ...data, results: data.results.map((row) => withRowFields(row, issueId, patch, knownFieldsOnly)) };
  }
  if (isRecord(data.results)) {
    if (options.rebucket !== false) {
      const moved = moveWithinBuckets(data.results, issueId, patch, data.grouped_by, knownFieldsOnly);
      if (moved !== null) return { ...data, results: moved };
    }
    const buckets = Object.fromEntries(
      Object.entries(data.results).map(([key, bucket]) => [
        key,
        withBucketFields(bucket, issueId, patch, knownFieldsOnly),
      ])
    );
    // Sub-grouped envelopes hold buckets of buckets; patch the inner rows
    // without moving them between groups.
    const nested = Object.fromEntries(
      Object.entries(buckets).map(([key, bucket]) => {
        if (!isRecord(bucket) || !isRecord(bucket.results) || Array.isArray(bucket.results)) return [key, bucket];
        return [
          key,
          {
            ...bucket,
            results: Object.fromEntries(
              Object.entries(bucket.results).map(([inner, rows]) => [
                inner,
                withBucketFields(rows, issueId, patch, knownFieldsOnly),
              ])
            ),
          },
        ];
      })
    );
    return { ...data, results: nested };
  }
  return data;
}
