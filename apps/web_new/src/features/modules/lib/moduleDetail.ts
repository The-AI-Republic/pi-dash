// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail screen model (parity/specs/modules.md §8.1, MOD-029).
// Pure: the tab title, the switcher pick target, the gone-away
// predicate and the pinned work-item badge tooltip the detail frame
// renders.

import { isApiError } from "@pidash/api-client";

import { moduleDetailHref } from "./moduleList.js";

/** Browser tab title: the project name followed by the module name. */
export function moduleDetailTitle(projectName: string, moduleName: string): string {
  return `${projectName} - ${moduleName}`;
}

/** Project home for a reader: the project's issues screen. */
export function projectIssuesHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/issues`;
}

/**
 * Detail address a switcher pick navigates to. Null stays put —
 * clearing is impossible (the menu hides its clear row) and
 * re-picking the current module is a no-op.
 */
export function switchModuleTarget(
  workspaceSlug: string,
  projectId: string,
  currentModuleId: string,
  pickedModuleId: string | null
): string | null {
  if (pickedModuleId === null || pickedModuleId === currentModuleId) return null;
  return moduleDetailHref(workspaceSlug, projectId, pickedModuleId);
}

/** Gone-away: an archived record, or a 404 on the live detail read. */
export function isModuleGoneAway(error: unknown, archivedAt: string | null | undefined): boolean {
  if (archivedAt !== null && archivedAt !== undefined) return true;
  return isApiError(error) && error.status === 404;
}

/**
 * Pinned badge tooltip. The singular keeps the plural verb with the
 * singular noun, matching the oracle's pinned shape.
 */
export function moduleBadgeTooltip(total: number): string {
  if (total === 1) return "There are 1 work item in this module";
  return `There are ${total} work items in this module`;
}
