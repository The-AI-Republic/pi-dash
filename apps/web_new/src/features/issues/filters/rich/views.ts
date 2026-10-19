// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-view actions on the filter row (parity/specs/issues.md §9,
// ISS-104). Save snapshots the live expression with the entity's display
// settings into the create payload; update is offered only to the owner
// of an unlocked view whose stored expression or display settings differ
// from what the row currently edits. Workspace defaults never update.
// The views area owns full view management; this module only carries the
// row's two entries.

import type { SavedView, SavedViewCreate, SavedViewPatch } from "@pidash/api-client";
import { issueKeys } from "../../api/keys.js";
import { parseDisplayPrefs, type DisplayPrefs } from "../../prefs/displayPrefs.js";
import { normalizeRichExpression, type RichExpression } from "./expression.js";

/** The row's view context: which record it edits, if any. */
export interface ViewContext {
  /** True when the row sits on a saved view rather than a bare entity list. */
  isView: boolean;
  /** Which collection the record lives in; defaults to the project. */
  viewScope: "project" | "workspace";
  viewId: string | null;
  /** The view's owner id, or null while unresolved. */
  ownerId: string | null;
  /** A locked view accepts no writes. */
  locked: boolean;
  /** Workspace default views are not updatable. */
  isDefault: boolean;
  /** Stored expression the view carries, for the dirty check. */
  storedExpression: Record<string, unknown>;
  /** Stored display_filters/display_properties the view carries. */
  storedDisplayFilters: Record<string, unknown>;
  storedDisplayProperties: Record<string, unknown>;
}

/** Everything a save/update snapshot needs from the live row. */
export interface ViewSnapshot {
  expression: Record<string, unknown>;
  displayFilters: Record<string, unknown>;
  displayProperties: Record<string, unknown>;
}

/** The route a saved view opens after creation. */
export function projectViewHref(workspaceSlug: string, projectId: string, viewId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/views/${viewId}`;
}

/** Create payload for "Save view": name plus the live snapshot. */
export function buildViewCreate(
  snapshot: ViewSnapshot,
  input: { name: string; description?: string }
): SavedViewCreate {
  return {
    name: input.name,
    ...(input.description === undefined || input.description === "" ? {} : { description: input.description }),
    rich_filters: snapshot.expression,
    display_filters: snapshot.displayFilters,
    display_properties: snapshot.displayProperties,
  };
}

/** Patch payload for "Update view": the same three snapshot keys. */
export function buildViewPatch(snapshot: ViewSnapshot): SavedViewPatch {
  return {
    rich_filters: snapshot.expression,
    display_filters: snapshot.displayFilters,
    display_properties: snapshot.displayProperties,
  };
}

/** Whether the singular "Save view" entry offers (an unsaved row). */
export function offersSaveView(context: ViewContext): boolean {
  return !context.isView;
}

/** Whether "Save as" offers (a saved view the caller may fork). */
export function offersSaveAs(context: ViewContext): boolean {
  return context.isView && !context.locked;
}

/**
 * Whether "Update view" offers: a saved view, owned by the caller, not
 * locked, not a workspace default, and changed against what it stores.
 */
export function offersUpdateView(context: ViewContext, snapshot: ViewSnapshot, meId: string | null): boolean {
  if (!context.isView || context.viewId === null) return false;
  if (context.locked || context.isDefault) return false;
  if (context.ownerId === null || meId === null || context.ownerId !== meId) return false;
  return isViewDirty(context, snapshot);
}

/** Compare the live snapshot with the view's stored values. */
export function isViewDirty(context: ViewContext, snapshot: ViewSnapshot): boolean {
  return (
    stableJson(context.storedExpression) !== stableJson(snapshot.expression) ||
    stableJson(context.storedDisplayFilters) !== stableJson(snapshot.displayFilters) ||
    stableJson(context.storedDisplayProperties) !== stableJson(snapshot.displayProperties)
  );
}

/** JSON with sorted keys, so key order never reads as a change. */
export function stableJson(value: unknown): string {
  return JSON.stringify(sortKeys(value));
}

function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (typeof value !== "object" || value === null) return value;
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(value as Record<string, unknown>).sort()) {
    out[key] = sortKeys((value as Record<string, unknown>)[key]);
  }
  return out;
}

/** Build the row context for a list rendered from a saved view. */
export function viewContextFromRecord(
  view: SavedView,
  meId: string | null,
  options: { isDefault: boolean }
): ViewContext {
  return {
    isView: true,
    viewId: view.id,
    ownerId: view.owned_by,
    locked: view.is_locked,
    isDefault: options.isDefault,
    viewScope: view.project === null ? "workspace" : "project",
    // One-leaf `and` wrappers and CSV multi values mean what the builder
    // writes, so the stored side is normalized before comparing.
    storedExpression: normalizeRichExpression(view.rich_filters),
    storedDisplayFilters: view.display_filters,
    storedDisplayProperties: view.display_properties,
  };
}

/** Build the row context for a bare entity list (no saved view). */
export function entityViewContext(): ViewContext {
  return {
    isView: false,
    viewId: null,
    ownerId: null,
    locked: false,
    isDefault: false,
    viewScope: "project",
    storedExpression: {},
    storedDisplayFilters: {},
    storedDisplayProperties: {},
  };
}

/** Saved-view query keys: workspace-scoped so sign-out clears them. */
export const savedViewKeys = {
  project: (workspaceSlug: string, projectId: string, viewId: string) =>
    [...issueKeys.all(workspaceSlug), "saved-view", projectId, viewId] as const,
  workspace: (workspaceSlug: string, viewId: string) =>
    [...issueKeys.all(workspaceSlug), "saved-view", "workspace", viewId] as const,
};

/**
 * A view record's stored filter expression, applied to the list query and
 * the builder. The record is written by the same view contract, so the
 * shape is the builder's own.
 */
export function viewExpression(view: SavedView): RichExpression {
  return view.rich_filters;
}

/**
 * The display keys a saved view would store for these settings. Only keys
 * the record already carries, or whose value actually differs from what
 * the record parses back to, are written: the stored object is patched in
 * place, so a view nobody touched snapshots back to exactly what it
 * stores and never reads as changed. A key the record lacks is written
 * only when the row showed something other than the record's default.
 */
export function viewDisplaySnapshot(
  view: Pick<SavedView, "display_filters" | "display_properties">,
  prefs: DisplayPrefs
): Pick<ViewSnapshot, "displayFilters" | "displayProperties"> {
  const stored = parseDisplayPrefs(view.display_filters, view.display_properties);
  const filters: Record<string, unknown> = { ...view.display_filters };
  const properties: Record<string, unknown> = { ...view.display_properties };
  function put(target: Record<string, unknown>, key: string, next: unknown, current: unknown): void {
    if (key in target || stableJson(next) !== stableJson(current)) target[key] = next;
  }
  put(filters, "layout", prefs.layout, stored.layout);
  put(filters, "group_by", prefs.groupBy, stored.groupBy);
  put(filters, "sub_group_by", prefs.subGroupBy, stored.subGroupBy);
  put(filters, "order_by", prefs.orderBy, stored.orderBy);
  put(filters, "show_empty_groups", prefs.showEmptyGroups, stored.showEmptyGroups);
  put(filters, "sub_issue", prefs.showSubIssues, stored.showSubIssues);
  const storedCalendar =
    typeof view.display_filters["calendar"] === "object" && view.display_filters["calendar"] !== null
      ? (view.display_filters["calendar"] as Record<string, unknown>)
      : {};
  // A calendar object carries two independent keys; write each only when
  // the record has it or the switch moved away from its parsed default.
  if (
    "calendar" in filters ||
    prefs.calendar.mode !== stored.calendar.mode ||
    prefs.calendar.showWeekends !== stored.calendar.showWeekends
  ) {
    const calendar: Record<string, unknown> = { ...storedCalendar };
    put(calendar, "layout", prefs.calendar.mode, stored.calendar.mode);
    put(calendar, "show_weekends", prefs.calendar.showWeekends, stored.calendar.showWeekends);
    filters["calendar"] = calendar;
  }
  for (const [key, value] of Object.entries(prefs.properties)) {
    put(properties, key, value, stored.properties[key]);
  }
  return { displayFilters: filters, displayProperties: properties };
}
