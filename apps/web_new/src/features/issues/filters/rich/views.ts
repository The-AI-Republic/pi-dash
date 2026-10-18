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
    storedExpression: view.rich_filters,
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
