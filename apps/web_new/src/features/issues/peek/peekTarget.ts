// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek target resolution (parity/specs/issues.md §14, ISS-165/167). The
// peek param names either a list key ("PROJ-123", kept for links written
// before opens switched to ids) or a raw issue id (what layout clicks and
// deep links carry). Rows the opener already holds resolve both without a
// fetch; anything else that is not key-shaped reads as an id under the
// peeked (or route) project, while an unmatched key needs the lite lookup
// before the detail read.

import type { IssueListItem } from "@pidash/api-client";

/** A peeked issue pinned to the project its detail reads from. */
export interface PeekTarget {
  issueId: string;
  projectId: string;
  /** List key when the opener knew it; derived after load otherwise. */
  issueKey: string | null;
}

/** List-key shape ("PROJ-123"); ids never match it. */
export function isIssueKey(value: string): boolean {
  return /^[A-Za-z0-9]+-\d+$/.test(value);
}

export interface PeekRows {
  rows: readonly Pick<IssueListItem, "id" | "project_id" | "sequence_id">[];
  /** Code the opener keyed its rows with. */
  projectIdentifier: string;
  /** Project of the mounting route. */
  routeProjectId: string;
  /** Cross-project override from the URL, when present. */
  peekProjectId?: string | null | undefined;
}

/**
 * Resolve the peeked value to a detail read. Returns null only for a
 * key no row holds, which the caller resolves through the lite read;
 * every other value answers inline.
 */
export function resolvePeekTarget(peeked: string, scope: PeekRows): PeekTarget | null {
  const trimmed = peeked.trim();
  if (trimmed === "") return null;
  // Layout clicks hand over an id; a held row pins the peek to that
  // row's own project, which may differ from the route's.
  const held = scope.rows.find((entry) => entry.id === trimmed);
  if (held !== undefined) return { issueId: held.id, projectId: held.project_id, issueKey: null };
  const row = scope.rows.find((entry) => `${scope.projectIdentifier}-${entry.sequence_id}` === trimmed);
  if (row !== undefined) return { issueId: row.id, projectId: row.project_id, issueKey: trimmed };
  if (!isIssueKey(trimmed)) {
    return { issueId: trimmed, projectId: scope.peekProjectId ?? scope.routeProjectId, issueKey: null };
  }
  return null;
}
