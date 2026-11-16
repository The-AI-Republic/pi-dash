// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle query keys (parity/specs/cycles.md §18, CYC-045). Every key
// starts with the workspace scope so a workspace switch or sign-out
// clears one subtree. The detail tuple intentionally matches the
// NEWFRONT-859 detail read so the shell, the peek and the detail page
// share one cache entry per cycle instead of forking it.

import type { CycleAnalyticsType } from "@pidash/api-client";

import { workspaceScope } from "../../../core/query/client.js";

export const cycleKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "cycles"] as const,
  /** Project live list (spec §23: no cycle_view param; bare array). */
  list: (workspaceSlug: string, projectId: string) => [...cycleKeys.all(workspaceSlug), "list", projectId] as const,
  /** Hero's current-cycle read (the only cycle_view=current caller). */
  hero: (workspaceSlug: string, projectId: string) => [...cycleKeys.all(workspaceSlug), "hero", projectId] as const,
  /** First issues page behind the hero's priority tab (CYC-041). */
  heroIssues: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...cycleKeys.all(workspaceSlug), "hero-issues", projectId, cycleId] as const,
  /** One live cycle (detail page and peek body share this read). */
  detail: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...cycleKeys.all(workspaceSlug), "detail", projectId, cycleId] as const,
  /** Project archived list (CYC-045: never refetches on window focus). */
  archived: (workspaceSlug: string, projectId: string) =>
    [...cycleKeys.all(workspaceSlug), "archived", projectId] as const,
  /** One archived cycle for the archived peek. */
  archivedDetail: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...cycleKeys.archived(workspaceSlug, projectId), cycleId] as const,
  /** Progress snapshot backing the ring (CYC-038, CYC-041). */
  progress: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...cycleKeys.all(workspaceSlug), "progress", projectId, cycleId] as const,
  /** Detailed (cloud-variant) progress series. */
  detailedProgress: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...cycleKeys.all(workspaceSlug), "detailed-progress", projectId, cycleId] as const,
  /** Breakdown distributions per analytics type (CYC-038, CYC-041). */
  analytics: (workspaceSlug: string, projectId: string, cycleId: string, type: CycleAnalyticsType) =>
    [...cycleKeys.all(workspaceSlug), "analytics", projectId, cycleId, type] as const,
  /** Workspace-wide rows for switchers and pickers. */
  workspaceCycles: (workspaceSlug: string) => [...cycleKeys.all(workspaceSlug), "workspace-cycles"] as const,
  /** Cursor-paged workspace actives (the only paged read in the area). */
  workspaceActives: (workspaceSlug: string, cursor: string | null) =>
    [...cycleKeys.all(workspaceSlug), "workspace-actives", cursor] as const,
};
