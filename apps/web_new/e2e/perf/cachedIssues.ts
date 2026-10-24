// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Readiness probe for the desktop warm-start perf spec (NEWFRONT-886).
//
// The measured relaunch blocks the issues API, so the spec may only reload
// once the persisted query cache actually holds the issue list. Bytes under
// the storage key are not that condition: the persistor writes one second
// after the last cache change and dehydrates settled queries only, so on a
// slow first load a quiet moment can leave a payload carrying the bootstrap
// queries and no issues entry. Reloading on that payload paints the
// blocked-API error state and times the setup, not the warm start.
//
// This module answers the question the measurement asks - how many
// issue-list rows does the stored payload carry for this project - as a pure
// function over the stored string. The spec reads the string inside the
// page (Playwright serializes an evaluate callback by source) and parses it
// here in Node, so the gate and its unit test share one definition.
//
// The key layout mirrors the issues query factory: the workspace scope, the
// `issues` subtree, then `list`, the project id and the view shape.

const WORKSPACE_SCOPE_INDEX = 0;
const ISSUES_SEGMENT_INDEX = 2;
const LIST_SEGMENT_INDEX = 3;
const PROJECT_ID_INDEX = 4;
const MIN_KEY_LENGTH = 5;

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Entries the restore path hydrates. An errored query that still carries
 * data is settled on purpose and paints its previously fetched rows.
 */
function isSettled(status: unknown): boolean {
  return status === "success" || status === "error";
}

/** Rows one dehydrated cache entry contributes, or 0 when it is not this project's list. */
function rowsInEntry(entry: unknown, projectId: string): number {
  if (!isRecord(entry)) return 0;
  const key = entry["queryKey"];
  if (!Array.isArray(key) || key.length < MIN_KEY_LENGTH) return 0;
  if (key[WORKSPACE_SCOPE_INDEX] !== "ws") return 0;
  if (key[ISSUES_SEGMENT_INDEX] !== "issues" || key[LIST_SEGMENT_INDEX] !== "list") return 0;
  if (key[PROJECT_ID_INDEX] !== projectId) return 0;
  const state = entry["state"];
  if (!isRecord(state) || !isSettled(state["status"])) return 0;
  const data = state["data"];
  if (!isRecord(data)) return 0;
  const results = data["results"];
  return Array.isArray(results) ? results.length : 0;
}

/**
 * Issue-list rows in a stored query-cache payload, taking the fullest
 * matching entry (a list can be cached under several view shapes). Anything
 * unreadable counts as zero so the caller keeps polling instead of
 * reloading against a payload it cannot paint.
 */
export function cachedIssueListRows(stored: unknown, projectId: string): number {
  if (typeof stored !== "string" || stored.length === 0) return 0;
  let payload: unknown;
  try {
    payload = JSON.parse(stored);
  } catch {
    return 0;
  }
  if (!isRecord(payload)) return 0;
  const state = payload["state"];
  if (!isRecord(state)) return 0;
  const queries = state["queries"];
  if (!Array.isArray(queries)) return 0;
  let rows = 0;
  for (const entry of queries) {
    const count = rowsInEntry(entry, projectId);
    if (count > rows) rows = count;
  }
  return rows;
}
