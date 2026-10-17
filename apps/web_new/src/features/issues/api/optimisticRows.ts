// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Generic optimistic row patch for single-issue property writes. One
// issue's fields patched inside any cached shape: a bare row list, a
// detail object, a flat list envelope, or a grouped envelope whose
// buckets each carry rows. Unknown shapes pass through untouched.

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function withRowFields(row: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  if (!isRecord(row) || row.id !== issueId) return row;
  return { ...row, ...patch };
}

function withBucketFields(bucket: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  if (!isRecord(bucket) || !Array.isArray(bucket.results)) return bucket;
  return { ...bucket, results: bucket.results.map((row) => withRowFields(row, issueId, patch)) };
}

/** Patch one issue's fields inside any cached list or detail shape. */
export function withIssueFields(data: unknown, issueId: string, patch: Record<string, unknown>): unknown {
  if (Array.isArray(data)) return data.map((row) => withRowFields(row, issueId, patch));
  if (!isRecord(data)) return data;
  if (typeof data.id === "string") return withRowFields(data, issueId, patch);
  if (Array.isArray(data.results)) {
    return { ...data, results: data.results.map((row) => withRowFields(row, issueId, patch)) };
  }
  if (isRecord(data.results)) {
    const buckets = Object.fromEntries(
      Object.entries(data.results).map(([key, bucket]) => [key, withBucketFields(bucket, issueId, patch)])
    );
    return { ...data, results: buckets };
  }
  return data;
}
