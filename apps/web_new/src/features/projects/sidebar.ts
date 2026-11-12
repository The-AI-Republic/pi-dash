// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar project model (parity/specs/shell.md §4, SHELL-046–055). Pure:
// joined ordering, the listed cap, drop math, subnav entries, row links
// and route guards. Components and the route build on these; nothing
// here touches React, the router, or the network.

import type { Project } from "@pidash/api-client";

import { ROLE_MEMBER } from "../../core/session/permissions.js";
import { selectDefaultTarget, selectProjectTabs, tabHref, type ProjectTab, type ProjectTabFlags } from "./tabs.js";

/** Step the reorder write jumps past its new neighbor (SHELL-051). */
export const PROJECT_ORDER_STEP = 10000;

/** Rows that belong in the sidebar: joined and unarchived (SHELL-048). */
export function isSidebarProject(project: Pick<Project, "member_role" | "archived_at">): boolean {
  return project.member_role !== null && project.archived_at === null;
}

/**
 * Joined order: ascending stored position, unpositioned rows first
 * (a fresh membership has no position yet). Stable: ties keep the
 * collection order.
 */
export function compareSidebarOrder(a: Pick<Project, "sort_order">, b: Pick<Project, "sort_order">): number {
  if (a.sort_order === null && b.sort_order === null) return 0;
  if (a.sort_order === null) return -1;
  if (b.sort_order === null) return 1;
  return a.sort_order - b.sort_order;
}

/** Joined, unarchived projects in sidebar order (SHELL-048). */
export function selectSidebarProjects(projects: readonly Project[]): Project[] {
  return projects.filter(isSidebarProject).sort(compareSidebarOrder);
}

/** Visible slice of the ordered rows; a null cap shows all (SHELL-048). */
export function sliceVisibleProjects(projects: readonly Project[], cap: number | null): Project[] {
  if (cap === null) return [...projects];
  return projects.slice(0, Math.max(0, cap));
}

/**
 * Ids with the dragged row moved to a slot, shifting the rows after
 * it down. Slots clamp into the list; unknown dragged ids resolve
 * back to the current order.
 */
export function moveProjectToIndex(current: readonly string[], draggedId: string, index: number): string[] {
  if (!current.includes(draggedId)) return [...current];
  const next = current.filter((id) => id !== draggedId);
  next.splice(Math.max(0, Math.min(index, next.length)), 0, draggedId);
  return next;
}

/** One listed row's vertical span for drop resolution. */
export interface SidebarDropRow {
  id: string;
  top: number;
  bottom: number;
}

/** Drop position: onto a row's top half, or past the last row. */
export interface SidebarDropTarget {
  id: string;
  after: boolean;
}

/**
 * Drop position under the pointer (SHELL-051): the first row whose
 * middle the pointer has not reached yet, or past the last row when
 * it cleared them all. Drops over the dragged row itself resolve to
 * no target so the indicator never lights its own row.
 */
export function resolveDropTarget(
  rows: readonly SidebarDropRow[],
  pointerY: number,
  sourceId: string
): SidebarDropTarget | null {
  if (rows.length === 0) return null;
  for (const row of rows) {
    if (pointerY < (row.top + row.bottom) / 2) {
      return row.id === sourceId ? null : { id: row.id, after: false };
    }
  }
  const last = rows[rows.length - 1];
  if (last === undefined || last.id === sourceId) return null;
  return { id: last.id, after: true };
}

/**
 * The single reorder write for a drop, or null when the drop changes
 * nothing (SHELL-051). The target resolves against the full joined
 * order, so capped-out rows keep their relative positions.
 */
export function resolveReorder(
  ordered: readonly Pick<Project, "id" | "sort_order">[],
  sourceId: string,
  target: SidebarDropTarget | null
): { projectId: string; sort_order: number } | null {
  if (target === null || target.id === sourceId) return null;
  const ids = ordered.map((row) => row.id);
  const rest = ids.filter((id) => id !== sourceId);
  const slot = rest.indexOf(target.id);
  const next = moveProjectToIndex(ids, sourceId, target.after ? slot + 1 : slot);
  return projectOrderPatch(ordered, next, sourceId);
}

/**
 * Stored position landing the dragged row at its new slot with one
 * write (SHELL-051): past the edge neighbor at either end, midway
 * between the new neighbors in the middle. Missing positions read as
 * zero, matching the collection order's unpositioned rows.
 */
export function sortOrderForMove(
  ordered: readonly Pick<Project, "id" | "sort_order">[],
  draggedId: string,
  order: readonly string[]
): number | null {
  const slot = order.indexOf(draggedId);
  if (slot < 0 || order.length !== ordered.length) return null;
  const positions = new Map(ordered.map((row) => [row.id, row.sort_order ?? 0]));
  const rest = order.filter((id) => id !== draggedId);
  if (slot === 0) return (positions.get(rest[0] ?? "") ?? 0) - PROJECT_ORDER_STEP;
  if (slot === order.length - 1) return (positions.get(rest[rest.length - 1] ?? "") ?? 0) + PROJECT_ORDER_STEP;
  return ((positions.get(rest[slot - 1] ?? "") ?? 0) + (positions.get(rest[slot] ?? "") ?? 0)) / 2;
}

/**
 * The single reorder write for a drop, or null when the drop changes
 * nothing. The caller patches the dragged project's stored position
 * and re-reads the collection; neighbors keep their positions.
 */
export function projectOrderPatch(
  ordered: readonly Pick<Project, "id" | "sort_order">[],
  order: readonly string[],
  draggedId: string
): { projectId: string; sort_order: number } | null {
  const current = ordered.map((row) => row.id);
  if (order.length !== current.length || order.every((id, index) => id === current[index])) return null;
  const sortOrder = sortOrderForMove(ordered, draggedId, order);
  if (sortOrder === null) return null;
  return { projectId: draggedId, sort_order: sortOrder };
}

/** One inline sub-navigation entry with its address attached. */
export interface SidebarSubnavEntry {
  key: string;
  name: string;
  href: string;
  /** Pending-intake badge; null hides it (SHELL-055 shape, read here). */
  badge: number | null;
}

/**
 * Inline sub-navigation entries for one project row (SHELL-050/055):
 * work tracking, cycles, modules, views, pages, intake with its
 * pending-count badge, schedulers and the workers panel, in fixed
 * order, each already flag- and role-gated (SHELL-055: guests keep
 * tracking, views, pages, intake and schedulers; cycles, modules and
 * workers are admin and member only). Addresses carry a trailing
 * slash so the parity driver tells entries apart from the bare row
 * link. Link targets belong to their own areas; the shell proves
 * hrefs plus gating only.
 */
export function selectSubnavEntries(args: {
  workspaceSlug: string;
  projectId: string;
  flags: ProjectTabFlags;
  role: number | null;
  intakeCount: number;
}): SidebarSubnavEntry[] {
  const tabs = selectProjectTabs(args);
  const entries: SidebarSubnavEntry[] = tabs.map((tab) => ({
    key: tab.key,
    name: tab.name,
    href: `${tab.href}/`,
    badge: tab.key === "intake" && args.intakeCount > 0 ? args.intakeCount : null,
  }));
  // Workers panel: member-plus, always rendered (SHELL-055). It has no
  // tab-strip counterpart, so it is appended here instead of the tab
  // catalogue.
  if (args.role !== null && args.role >= ROLE_MEMBER) {
    entries.push({
      key: "runners",
      name: "AI Workers",
      href: `/${args.workspaceSlug}/projects/${args.projectId}/runners/`,
      badge: null,
    });
  }
  return entries;
}

/**
 * Row link for one project (SHELL-050): the stored default tab when it
 * names an available tab, else tracking. Unknown stored keys fall
 * back; the caller reads the stored key from the warmed member store
 * and the available tabs from the same selector as the strip.
 */
export function resolveProjectRowHref(args: {
  tabs: ProjectTab[];
  defaultTab: string;
  workspaceSlug: string;
  projectId: string;
}): string {
  if (args.tabs.length === 0) return tabHref(args.workspaceSlug, args.projectId, "issues");
  return selectDefaultTarget(args.tabs, args.defaultTab)?.href ?? tabHref(args.workspaceSlug, args.projectId, "issues");
}

/**
 * Routes whose content lives under the projects group (SHELL-048):
 * project pages, drafts and workspace views. Landing on one expands
 * the group so the active row is never hidden behind it.
 */
export function isProjectsGroupRoute(pathname: string): boolean {
  return pathname.includes("projects") || pathname.includes("/drafts") || pathname.includes("/workspace-views");
}

/** Active rule for the workspace aggregate row (SHELL-048). */
export function isAggregateRowActive(pathname: string, workspaceSlug: string): boolean {
  return pathname.startsWith(`/${workspaceSlug}/workspace-views`);
}

/**
 * Active rule for one sub-navigation entry (SHELL-050): the current
 * address at or under the entry's address, trailing slashes ignored
 * on both sides so row links and entries agree.
 */
export function isSubnavEntryActive(pathname: string, href: string): boolean {
  const strip = (value: string): string => (value.endsWith("/") && value.length > 1 ? value.slice(0, -1) : value);
  const current = strip(pathname);
  const entry = strip(href);
  return current === entry || current.startsWith(`${entry}/`);
}

/** Smallest surface the scroll reveal needs: a subset of Element. */
export interface ScrollRevealTarget {
  scrollIntoView?: ((options?: ScrollIntoViewOptions) => void) | undefined;
}

/**
 * Reveal the active project row on mount (SHELL-050): without layout
 * support (tests, SSR) it stays a no-op instead of throwing.
 */
export function revealRow(row: ScrollRevealTarget | null, active: boolean): void {
  if (row === null || !active) return;
  if (typeof row.scrollIntoView === "function") row.scrollIntoView({ block: "nearest" });
}
