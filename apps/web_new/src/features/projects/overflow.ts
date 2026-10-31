// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overflow slide-over model (parity/specs/shell.md §4, SHELL-052).
// Pure: the capped-out rule and the panel's name-or-code search. The
// panel lists the joined collection once the cap leaves rows out of
// the main sidebar; search narrows by project name or short code. The
// component builds on this; nothing here touches React or the router.

import type { Project } from "@pidash/api-client";

/**
 * True once the cap leaves joined rows out of the main sidebar: a null
 * cap shows everything, so the toggle never renders uncapped.
 */
export function hasOverflow(joinedCount: number, cap: number | null): boolean {
  return cap !== null && joinedCount > cap;
}

/**
 * Panel rows matching the query (SHELL-052): names match without case,
 * short codes match as typed. A blank query keeps every row.
 */
export function filterOverflowProjects(projects: readonly Project[], query: string): Project[] {
  if (query.trim() === "") return [...projects];
  const nameNeedle = query.toLowerCase();
  return projects.filter(
    (project) => project.name.toLowerCase().includes(nameNeedle) || project.identifier.includes(query)
  );
}
