// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles screen model (parity/specs/cycles.md §1 and §5, CYC-010–013).
// Pure: addresses, tab title, the one body-state decision the screen
// renders, and the gone-away decider. Components build on this; nothing
// here touches React, the router, or the network.

import { isApiError } from "@pidash/api-client";

/** Crumb and fallback tab title for the cycles list. */
export const CYCLES_CRUMB = "Cycles";

/** Tab title for the cycles list: the project name followed by Cycles. */
export function cyclesTitle(projectName: string): string {
  return `${projectName} - ${CYCLES_CRUMB}`;
}

/**
 * Project cycles list address, trailing-slash form: the gone-away
 * back action lands here, and the CYC-013 oracle pins that exact path
 * (the router preserves the slash). Other surfaces pin their own form.
 */
export function cyclesListHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/cycles/`;
}

/**
 * Canonical cycle detail address, without a trailing slash: row links,
 * the peek's full-detail link, and the copy/open helpers all use this
 * one spelling, so a shared address always reloads the same cycle.
 */
export function cycleDetailPath(workspaceSlug: string, projectId: string, cycleId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/cycles/${cycleId}`;
}

/**
 * Cycles feature-settings leaf. The shortcut targets the rendered leaf,
 * never the bare features index, which has no page (bug NEWFRONT-247,
 * shared with the views/modules gates).
 */
export function cyclesFeatureHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/features/cycles`;
}

/**
 * Project work-items address: the detail trail's project crumb lands
 * here (CYC-034).
 */
export function projectIssuesHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/issues`;
}

/**
 * Detail address a switcher pick navigates to (CYC-033): null stays
 * put — clearing is impossible (the menu hides its clear row) and
 * re-picking the current cycle is a no-op.
 */
export function switchCycleTarget(
  workspaceSlug: string,
  projectId: string,
  currentCycleId: string,
  pickedCycleId: string | null
): string | null {
  if (pickedCycleId === null || pickedCycleId === currentCycleId) return null;
  return cycleDetailPath(workspaceSlug, projectId, pickedCycleId);
}

/** The one body the cycles screen renders. */
export type CyclesBodyState = "skeleton" | "error" | "gate" | "zero" | "list";

export interface CyclesBodyInput {
  projectPending: boolean;
  projectError: boolean;
  /** The project's cycle_view switch is off. */
  gateOff: boolean;
  cyclesPending: boolean;
  cyclesError: boolean;
  cycleCount: number;
}

/**
 * Body order: the project read first (its gate takes precedence over
 * every list state), then the list read, then zero versus rows. A list
 * refetch after mutations keeps its rows: only a pending read with no
 * data yet shows the skeleton.
 */
export function resolveCyclesBody({
  projectPending,
  projectError,
  gateOff,
  cyclesPending,
  cyclesError,
  cycleCount,
}: CyclesBodyInput): CyclesBodyState {
  if (projectPending) return "skeleton";
  if (projectError) return "error";
  if (gateOff) return "gate";
  if (cyclesPending) return "skeleton";
  if (cyclesError) return "error";
  if (cycleCount === 0) return "zero";
  return "list";
}

/**
 * Whether a cycle detail read renders the gone-away view (CYC-013): a
 * missing, deleted or foreign id 404s, and an archived cycle read
 * through a live address carries its stamp. Every other failure keeps
 * the page's own behavior.
 */
export function isCycleGoneAway(error: unknown, archivedAt: string | null | undefined): boolean {
  if (archivedAt !== null && archivedAt !== undefined) return true;
  return isApiError(error) && error.status === 404;
}
