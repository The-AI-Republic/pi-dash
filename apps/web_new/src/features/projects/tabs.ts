// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project tab model (parity/specs/shell.md §7, SHELL-072–079). Pure:
// definitions, role and feature gating, active resolution, default and
// switcher targets, overflow splits, quick-action entries, preference
// parsing. Components and drivers build on these; nothing here touches
// React, the router, or the network.

import { ROLE_ADMIN, ROLE_MEMBER } from "../../core/session/permissions.js";

/** Storage key of the tracking tab; the fallback for every default lookup. */
export const DEFAULT_TAB_KEY = "work_items";

/**
 * Storage key the highlight rule matches for epics. No rendered tab
 * carries it, so an epic detail page highlights nothing (SHELL-076).
 */
export const EPIC_TAB_KEY = "epics";

/** Project feature flags that gate individual tabs. */
export interface ProjectTabFlags {
  cycle_view: boolean;
  module_view: boolean;
  issue_views_view: boolean;
  page_view: boolean;
  inbox_view: boolean;
}

/** One tab of the strip, before the address is attached. */
export interface ProjectTabDef {
  key: string;
  name: string;
  segment: string;
  order: number;
  /** Flag that must be on, or null for tabs that always render. */
  flag: keyof ProjectTabFlags | null;
  /** When true, guests never see the tab. */
  memberPlus: boolean;
}

/** Fixed catalogue in display order (SHELL-075). */
export const PROJECT_TAB_DEFS: readonly ProjectTabDef[] = [
  { key: "work_items", name: "Work Items", segment: "issues", order: 1, flag: null, memberPlus: false },
  { key: "cycles", name: "Cycles", segment: "cycles", order: 2, flag: "cycle_view", memberPlus: true },
  { key: "modules", name: "Modules", segment: "modules", order: 3, flag: "module_view", memberPlus: true },
  { key: "views", name: "Views", segment: "views", order: 4, flag: "issue_views_view", memberPlus: false },
  { key: "pages", name: "Pages", segment: "pages", order: 5, flag: "page_view", memberPlus: false },
  { key: "intake", name: "Intake", segment: "intake", order: 6, flag: "inbox_view", memberPlus: false },
  { key: "schedulers", name: "Schedulers", segment: "schedulers", order: 7, flag: null, memberPlus: false },
];

/** One tab with its address attached. */
export interface ProjectTab extends ProjectTabDef {
  href: string;
}

export function tabHref(workspaceSlug: string, projectId: string, segment: string): string {
  return `/${workspaceSlug}/projects/${projectId}/${segment}`;
}

/**
 * Tabs the caller may see, in fixed order (SHELL-075). Flag-hidden or
 * role-lacking tabs never render. A null role reads as guest level.
 */
export function selectProjectTabs(args: {
  workspaceSlug: string;
  projectId: string;
  flags: ProjectTabFlags;
  role: number | null;
}): ProjectTab[] {
  const memberPlus = args.role !== null && args.role >= ROLE_MEMBER;
  return PROJECT_TAB_DEFS.filter((def) => {
    if (def.flag !== null && !args.flags[def.flag]) return false;
    if (def.memberPlus && !memberPlus) return false;
    return true;
  })
    .slice()
    .sort((a, b) => a.order - b.order)
    .map((def) => ({ ...def, href: tabHref(args.workspaceSlug, args.projectId, def.segment) }));
}

/** A work item in view, for the detail-page highlight rule. */
export interface StripWorkItem {
  id: string;
  projectId: string;
  isEpic: boolean;
}

/**
 * Key of the highlighted tab, or null (SHELL-076). Nested routes keep
 * the parent highlighted through prefix matching; a plain work item of
 * this project highlights tracking; an epic matches the epic key, which
 * no rendered tab carries, so nothing highlights.
 */
export function resolveActiveTabKey(args: {
  tabs: ProjectTab[];
  pathname: string;
  projectId: string;
  workItem: StripWorkItem | null;
}): string | null {
  for (const tab of args.tabs) {
    if (args.workItem !== null && args.workItem.projectId === args.projectId) {
      if (tab.key === DEFAULT_TAB_KEY && !args.workItem.isEpic) return tab.key;
      if (tab.key === EPIC_TAB_KEY && args.workItem.isEpic) return tab.key;
    }
    if (args.pathname === tab.href || args.pathname.startsWith(`${tab.href}/`)) return tab.key;
  }
  return null;
}

/**
 * Landing tab for bare addresses (SHELL-077): the stored default when it
 * names an available tab, else tracking. Unknown stored keys fall back.
 */
export function selectDefaultTarget(tabs: ProjectTab[], defaultKey: string): ProjectTab | null {
  if (tabs.length === 0) return null;
  return tabs.find((tab) => tab.key === defaultKey) ?? tabs.find((tab) => tab.key === DEFAULT_TAB_KEY) ?? null;
}

/**
 * Switcher landing (SHELL-072): the picked project opened at the current
 * project's validated default tab, so a switch keeps the member's tab.
 * Unknown keys fall back to tracking.
 */
export function selectSwitcherTarget(args: {
  currentTabs: ProjectTab[];
  currentDefaultKey: string;
  workspaceSlug: string;
  projectId: string;
}): string {
  const target = selectDefaultTarget(args.currentTabs, args.currentDefaultKey);
  const segment = target?.segment ?? "issues";
  return tabHref(args.workspaceSlug, args.projectId, segment);
}

/** Per-member tab preferences: the stored default plus hidden keys. */
export interface TabPreferences {
  defaultTab: string;
  hiddenTabs: string[];
}

/**
 * Read navigation preferences off a project user-properties payload.
 * Missing or malformed branches fall back to tracking plus no hides.
 */
export function parseNavigationPrefs(preferences: unknown): TabPreferences {
  const fallback: TabPreferences = { defaultTab: DEFAULT_TAB_KEY, hiddenTabs: [] };
  if (typeof preferences !== "object" || preferences === null) return fallback;
  const navigation = (preferences as Record<string, unknown>)["navigation"];
  if (typeof navigation !== "object" || navigation === null) return fallback;
  const branch = navigation as Record<string, unknown>;
  const defaultTab = typeof branch["default_tab"] === "string" ? branch["default_tab"] : DEFAULT_TAB_KEY;
  const hidden = Array.isArray(branch["hide_in_more_menu"])
    ? branch["hide_in_more_menu"].filter((key): key is string => typeof key === "string")
    : [];
  return { defaultTab, hiddenTabs: hidden };
}

/** The strip mounts only in tabbed project-list mode. */
export function isTabbedMode(navigationControlPreference: string): boolean {
  return navigationControlPreference === "TABBED";
}

/** Project admin standing for the quick-actions menu (SHELL-073). */
export function isProjectAdmin(role: number | null): boolean {
  return role !== null && role >= ROLE_ADMIN;
}

/** Admin or member standing; guests and outsiders read false. */
export function isProjectAuthorized(role: number | null): boolean {
  return role !== null && role >= ROLE_MEMBER;
}

export type QuickActionId = "publish" | "copy" | "archives" | "settings" | "leave";

/** Quick-action entry in menu order (SHELL-073). */
export interface QuickActionEntry {
  id: QuickActionId;
  label: string;
}

/**
 * Entries the role permits: publish for project admins, archives for
 * admin and member, leave for everyone else, copy and settings for all.
 */
export function selectQuickActions(args: { isAdmin: boolean; isAuthorized: boolean }): QuickActionEntry[] {
  const entries: QuickActionEntry[] = [];
  if (args.isAdmin) entries.push({ id: "publish", label: "Publish project" });
  entries.push({ id: "copy", label: "Copy link" });
  if (args.isAuthorized) entries.push({ id: "archives", label: "Archives" });
  entries.push({ id: "settings", label: "Settings" });
  if (!args.isAuthorized) entries.push({ id: "leave", label: "Leave project" });
  return entries;
}

/**
 * Address the copy entry writes (SHELL-074): the active tab route, or
 * the tracking route when no tab is active.
 */
export function selectCopyPath(args: { activeHref: string | null; workspaceSlug: string; projectId: string }): string {
  return args.activeHref ?? tabHref(args.workspaceSlug, args.projectId, "issues");
}

/**
 * How many leading tabs fit (SHELL-078): each step adds its width plus
 * the gap, reserving the overflow trigger while tabs remain. Falls back
 * to one visible tab when nothing fits but the trigger would.
 */
export function countFittingTabs(args: {
  itemWidths: number[];
  gap: number;
  overflowWidth: number;
  containerWidth: number;
}): number {
  if (args.itemWidths.length === 0) return 0;
  let total = 0;
  let count = 0;
  for (const [index, width] of args.itemWidths.entries()) {
    const step = width + (count > 0 ? args.gap : 0);
    const remaining = args.itemWidths.length - (index + 1);
    const reserve = remaining > 0 ? args.overflowWidth + args.gap : 0;
    if (total + step + reserve > args.containerWidth) break;
    total += step;
    count += 1;
  }
  if (count === 0 && args.containerWidth > args.overflowWidth) return 1;
  return count;
}

/**
 * Visible-plus-overflow split (SHELL-078). The first visibleCount
 * candidates show; when the active tab would pool, it swaps into view
 * with the last visible tab; user-hidden tabs always pool after the
 * space-pooled ones. The trigger shows while anything pools.
 */
export function resolveOverflow(args: {
  candidates: ProjectTab[];
  hidden: ProjectTab[];
  visibleCount: number;
  activeKey: string | null;
}): { visible: ProjectTab[]; overflow: ProjectTab[]; hasOverflow: boolean } {
  let visible = args.candidates.slice(0, args.visibleCount);
  let pooled = args.candidates.slice(args.visibleCount);
  const activeIndex = args.candidates.findIndex((tab) => tab.key === args.activeKey);
  if (args.visibleCount > 0 && activeIndex >= args.visibleCount) {
    const active = args.candidates[activeIndex];
    const replaced = visible[args.visibleCount - 1];
    if (active !== undefined && replaced !== undefined) {
      visible = [...visible.slice(0, args.visibleCount - 1), active];
      pooled = [
        replaced,
        ...args.candidates.slice(args.visibleCount, activeIndex),
        ...args.candidates.slice(activeIndex + 1),
      ];
    }
  }
  const overflow = [...pooled, ...args.hidden];
  return { visible, overflow, hasOverflow: overflow.length > 0 };
}
