// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives tab model (parity/specs/archives.md §1, ARCH-001–003). Pure:
// the fixed tab catalogue, project-flag gating, addresses and per-tab
// browser titles. Components and routes build on these; nothing here
// touches React, the router, or the network.

/** Archives tab keys, in strip order. */
export type ArchivesTabKey = "issues" | "cycles" | "modules";

/** Project flags that gate individual archives tabs. */
export interface ArchivesTabFlags {
  cycle_view: boolean;
  module_view: boolean;
}

/** One archives tab before its address is attached. */
export interface ArchivesTabDef {
  key: ArchivesTabKey;
  /** Strip label and breadcrumb terminal, exactly as asserted. */
  label: string;
  /** Lower-case section named by the browser title. */
  section: string;
  /** Flag that must be on, or null for tabs that always render. */
  flag: keyof ArchivesTabFlags | null;
}

const ISSUES_DEF: ArchivesTabDef = { key: "issues", label: "Work items", section: "work items", flag: null };
const CYCLES_DEF: ArchivesTabDef = { key: "cycles", label: "Cycles", section: "cycles", flag: "cycle_view" };
const MODULES_DEF: ArchivesTabDef = { key: "modules", label: "Modules", section: "modules", flag: "module_view" };

/** Fixed catalogue in display order (ARCH-001). */
export const ARCHIVES_TAB_DEFS: readonly ArchivesTabDef[] = [ISSUES_DEF, CYCLES_DEF, MODULES_DEF];

const DEF_BY_KEY: Record<ArchivesTabKey, ArchivesTabDef> = {
  issues: ISSUES_DEF,
  cycles: CYCLES_DEF,
  modules: MODULES_DEF,
};

/** One tab with its address attached. */
export interface ArchivesTab extends ArchivesTabDef {
  href: string;
}

export function archivesTabHref(workspaceSlug: string, projectId: string, tab: ArchivesTabKey): string {
  return `/${workspaceSlug}/projects/${projectId}/archives/${tab}`;
}

/**
 * Tabs the strip renders, in fixed order (ARCH-001). Flag-hidden tabs
 * never render; a reload re-reads the flags from the project row.
 */
export function selectArchivesTabs(args: {
  workspaceSlug: string;
  projectId: string;
  flags: ArchivesTabFlags;
}): ArchivesTab[] {
  return ARCHIVES_TAB_DEFS.filter((def) => def.flag === null || args.flags[def.flag]).map((def) => ({
    ...def,
    href: archivesTabHref(args.workspaceSlug, args.projectId, def.key),
  }));
}

/** Catalogue entry for one tab key. */
export function archivesTabDef(tab: ArchivesTabKey): ArchivesTabDef {
  return DEF_BY_KEY[tab];
}

/**
 * Browser title while the tab shows (ARCH-003):
 * `<project> - Archived <section>`.
 */
export function archivesTabTitle(projectName: string, tab: ArchivesTabKey): string {
  return `${projectName} - Archived ${DEF_BY_KEY[tab].section}`;
}

/** Title while the project row resolves; replaced once the name lands. */
export const ARCHIVES_HEAD_FALLBACK = "Archives";

/** Route title: the per-tab title, or the fallback while loading. */
export function archivesRouteTitle(projectName: string | undefined, tab: ArchivesTabKey): string {
  return projectName === undefined ? ARCHIVES_HEAD_FALLBACK : archivesTabTitle(projectName, tab);
}
