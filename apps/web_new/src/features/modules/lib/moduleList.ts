// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules list screen model (parity/specs/modules.md §2.1, §2.2, §3.1,
// §3.4). Pure: addresses, the tab title, the one body-state decision the
// screen renders, the progress and date readings rows and cards show, and
// the name ordering the list's default sort uses. Nothing here touches
// React, the router, the network or the browser.

import type { ModuleRow } from "@pidash/api-client";

/** Crumb and fallback tab title for the modules list. */
export const MODULES_CRUMB = "Modules";

/** Tab title for the modules list: the project name followed by Modules. */
export function modulesTitle(projectName: string): string {
  return `${projectName} - ${MODULES_CRUMB}`;
}

/** Canonical (slash-terminated) modules list address. */
export function modulesListHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/modules/`;
}

/** One module's detail address, the row and card name links to. */
export function moduleDetailHref(workspaceSlug: string, projectId: string, moduleId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/modules/${moduleId}`;
}

/**
 * Manage-features shortcut target (§2.2): the project's modules feature
 * leaf in settings. The bare `…/features` address has no page of its own
 * (bug NEWFRONT-247, shared with the views and cycles gates), so the
 * shortcut goes to the rendered leaf, where the modules switch lives.
 */
export function modulesFeatureHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/features/modules`;
}

/** The one body the modules list renders (BY ROW OWNERSHIP). */
export type ModulesListBodyState = "skeleton" | "gate" | "rows";

export interface ModulesListBodyInput {
  /** The project row's feature flags are still loading. */
  projectPending: boolean;
  /** The project's `module_view` switch is off. */
  gateOff: boolean;
  /** The modules list read is still loading its first page. */
  modulesPending: boolean;
}

/**
 * Body order: the feature gate wins over every list state and never
 * flashes the skeleton first (§2.2), then the skeleton while the list's
 * first read is in flight (§3.1), then rows. The zero-modules and no-match
 * containers belong to slices F (MOD-009) and B (MOD-010); they mount
 * inside the rows slot this slice exposes, so the shell only ever composes
 * gate, skeleton and content.
 */
export function resolveModulesListBody({
  projectPending,
  gateOff,
  modulesPending,
}: ModulesListBodyInput): ModulesListBodyState {
  if (projectPending) return "skeleton";
  if (gateOff) return "gate";
  if (modulesPending) return "skeleton";
  return "rows";
}

/** A module's completion share in [0, 1]; no work items reads as zero. */
export function moduleProgressRatio(
  row: Pick<ModuleRow, "completed_issues" | "cancelled_issues" | "total_issues">
): number {
  if (row.total_issues <= 0) return 0;
  const done = row.completed_issues + row.cancelled_issues;
  return Math.min(1, Math.max(0, done / row.total_issues));
}

/** The integer percentage the list's progress marker shows (floored). */
export function moduleProgressPercent(
  row: Pick<ModuleRow, "completed_issues" | "cancelled_issues" | "total_issues">
): number {
  return Math.floor(moduleProgressRatio(row) * 100);
}

/** Done-over-total, as the board card's count reads it. */
export function moduleIssueCountText(row: Pick<ModuleRow, "completed_issues" | "total_issues">): string {
  return `${row.completed_issues}/${row.total_issues}`;
}

/**
 * Natural, case-insensitive name comparison (§2.5): numeric runs compare
 * by value, so "Module 2" sorts before "Module 10". Ties return 0 so the
 * caller's stable sort keeps the server's own order.
 */
export function compareModuleNames(left: string, right: string): number {
  const chunk = /\d+|\D+/g;
  const leftParts = left.toLowerCase().match(chunk) ?? [];
  const rightParts = right.toLowerCase().match(chunk) ?? [];
  const length = Math.min(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const leftPart = leftParts[index]!;
    const rightPart = rightParts[index]!;
    const leftDigits = /^\d/.test(leftPart);
    const rightDigits = /^\d/.test(rightPart);
    if (leftDigits && rightDigits) {
      const leftValue = Number(leftPart);
      const rightValue = Number(rightPart);
      if (leftValue !== rightValue) return leftValue < rightValue ? -1 : 1;
    } else if (leftPart !== rightPart) {
      return leftPart < rightPart ? -1 : 1;
    }
  }
  if (leftParts.length !== rightParts.length) return leftParts.length < rightParts.length ? -1 : 1;
  return 0;
}

/**
 * The list's default order (§2.5, Name ascending). A stable sort keeps
 * creation order for equal names, which the server already sends.
 */
export function sortModulesByName<T extends { name: string }>(rows: readonly T[]): T[] {
  return [...rows].sort((left, right) => compareModuleNames(left.name, right.name));
}
