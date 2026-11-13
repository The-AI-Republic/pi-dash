// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-project page move rules the client can decide on its own
// (PAGE-017, PDASHOSS01-347). The server stays the authority: it re-checks
// every rule below and answers a refusal with a plain-language reason, which
// the notice shows verbatim. These helpers only keep the affordance and the
// picker honest so the common refusals never need a round trip.

import { isApiError, type PageRow, type ProjectList } from "@pidash/api-client";

import { ROLE_MEMBER } from "../../../core/session/permissions.js";
import { pageParentId } from "./pageNested.js";

export interface PageProjectMoveGate {
  /** The viewer is an active admin of the page's current project. */
  admin: boolean;
  /** A locked page cannot move until someone unlocks it. */
  locked: boolean;
}

/** Whether the move entry is offered at all: admins only, never on a locked page. */
export function canMovePageToProject(gate: PageProjectMoveGate): boolean {
  return gate.admin && !gate.locked;
}

/**
 * Projects the page may move into: every other live project in the
 * workspace where the viewer holds member or admin standing. Guests and
 * non-members cannot write there; archived projects accept nothing.
 */
export function pageMoveProjectIds(projects: ProjectList, currentProjectId: string): string[] {
  return projects
    .filter(
      (project) =>
        project.id !== currentProjectId &&
        project.archived_at === null &&
        project.member_role !== null &&
        project.member_role >= ROLE_MEMBER
    )
    .map((project) => project.id);
}

/**
 * The server's own reason for a refused move, or undefined when the failure
 * carried none (transport errors, an empty body). The move route answers
 * `{"error": "..."}`; the locked refusal also carries a machine code, which
 * the client folds into its message, so the body's text wins over it.
 */
export function pageMoveErrorMessage(error: unknown): string | undefined {
  if (!isApiError(error) || error.status === 0) return undefined;
  const body = error.body;
  if (body !== null && typeof body === "object") {
    const reason = (body as { error?: unknown }).error;
    if (typeof reason === "string" && reason.trim() !== "") return reason;
    const detail = (body as { detail?: unknown }).detail;
    if (typeof detail === "string" && detail.trim() !== "") return detail;
  }
  return undefined;
}

/**
 * Ids of the moved page and every descendant the caller has read, at any
 * depth. The server moves the whole live subtree, so every one of them
 * leaves the source project's caches.
 */
export function movedSubtreeIds(
  rows: Iterable<Pick<PageRow, "id" | "parent" | "parent_id">>,
  pageId: string
): Set<string> {
  const byParent = new Map<string, string[]>();
  for (const row of rows) {
    const parent = pageParentId(row);
    if (parent === null) continue;
    const siblings = byParent.get(parent);
    if (siblings === undefined) byParent.set(parent, [row.id]);
    else if (!siblings.includes(row.id)) siblings.push(row.id);
  }
  const found = new Set<string>([pageId]);
  const queue = [pageId];
  while (queue.length > 0) {
    const current = queue.pop() as string;
    for (const child of byParent.get(current) ?? []) {
      if (found.has(child)) continue;
      found.add(child);
      queue.push(child);
    }
  }
  return found;
}
