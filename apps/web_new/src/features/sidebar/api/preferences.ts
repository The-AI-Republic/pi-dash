// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar preferences (parity/specs/shell.md §6, SHELL-070). Pins and sort
// order per workspace row; the personalization dialog edits the personal
// catalog and the sidebar renders it. Writes apply optimistically, roll
// back on failure with an error toast, and re-read after settling: the
// bulk endpoint answers a bare acknowledgement.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getSidebarPreferences,
  updateSidebarPreferences,
  type ApiClient,
  type SidebarPreferencePatch,
  type SidebarPreferences,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { sidebarKeys } from "./keys.js";

/** One personal row the dialog offers, with its sidebar address. */
export interface PersonalCatalogEntry {
  key: string;
  label: string;
  /** Workspace-relative path segment, e.g. "drafts". */
  path: string;
}

/**
 * Personal entries the dialog offers (SHELL-070 target: Drafts only).
 * Your work is no longer pinnable or reorderable: its stored key is
 * ignored, with no migration.
 */
export const PERSONAL_CATALOG: PersonalCatalogEntry[] = [{ key: "drafts", label: "Drafts", path: "drafts" }];

/** One resolved personal entry: catalog row over stored pin/order. */
export interface PersonalEntryState {
  key: string;
  label: string;
  path: string;
  enabled: boolean;
  sortOrder: number;
}

/** Display order: ascending stored position. */
export function comparePersonalOrder(a: PersonalEntryState, b: PersonalEntryState): number {
  return a.sortOrder - b.sortOrder;
}

/**
 * Resolve the personal list over stored preferences, ordered for
 * display. Catalog rows missing from storage default to visible;
 * stored keys outside the catalog are ignored without migration.
 */
export function selectPersonalEntries(preferences: SidebarPreferences | undefined): PersonalEntryState[] {
  return PERSONAL_CATALOG.map((entry, index) => ({
    key: entry.key,
    label: entry.label,
    path: entry.path,
    enabled: preferences?.[entry.key]?.is_pinned ?? true,
    sortOrder: preferences?.[entry.key]?.sort_order ?? index,
  })).sort(comparePersonalOrder);
}

/** One visible personal row as the sidebar renders it. */
export interface PersonalSidebarEntry {
  key: string;
  label: string;
  href: string;
}

/** Visible personal entries with sidebar addresses, in display order. */
export function selectVisiblePersonalEntries(
  entries: PersonalEntryState[],
  workspaceSlug: string
): PersonalSidebarEntry[] {
  return entries
    .filter((entry) => entry.enabled)
    .map((entry) => ({ key: entry.key, label: entry.label, href: `/${workspaceSlug}/${entry.path}/` }));
}

/** Personal group view-model: the route resolves it, the dialog renders it. */
export interface PersonalGroupState {
  status: "pending" | "error" | "ready";
  entries: PersonalEntryState[];
}

/** Fold a preferences query into the dialog's personal view-model. */
export function selectPersonalGroup(
  status: "pending" | "error" | "success",
  preferences: SidebarPreferences | undefined
): PersonalGroupState {
  if (status === "pending") return { status: "pending", entries: [] };
  if (status === "error") return { status: "error", entries: [] };
  return { status: "ready", entries: selectPersonalEntries(preferences) };
}

/**
 * Move the dragged key into the target's slot, shifting the span
 * between. Drops onto the dragged key itself, or involving keys
 * outside the list, resolve back to the current order.
 */
export function moveEntryOrder(current: string[], dragged: string, target: string): string[] {
  if (dragged === target || !current.includes(dragged) || !current.includes(target)) return [...current];
  const next = current.filter((key) => key !== dragged);
  next.splice(current.indexOf(target), 0, dragged);
  return next;
}

/**
 * Order triples persisting a drop, or null when the drop changes
 * nothing. Positions renumber from zero across the personal group.
 */
export function reorderPatches(current: string[], dragged: string, target: string): SidebarPreferencePatch[] | null {
  // Lengths match by construction (filter removes one, splice adds it
  // back), so an element-wise compare decides the no-op.
  const next = moveEntryOrder(current, dragged, target);
  const same = next.every((key, index) => key === current[index]);
  if (same) return null;
  return next.map((key, index) => ({ key, sort_order: index }));
}

/**
 * Fold patch triples over the cached map. Keys missing from the cache
 * (or partial triples) fill from the same defaults the selector uses;
 * unknown keys pass through for the server to skip.
 */
export function applySidebarPatches(
  previous: SidebarPreferences | undefined,
  items: SidebarPreferencePatch[]
): SidebarPreferences | undefined {
  if (previous === undefined) return undefined;
  const next: SidebarPreferences = { ...previous };
  for (const item of items) {
    const current = next[item.key];
    next[item.key] = {
      is_pinned: item.is_pinned ?? current?.is_pinned ?? true,
      sort_order: item.sort_order ?? current?.sort_order ?? 0,
    };
  }
  return next;
}

/** The caller's sidebar preferences for one workspace. */
export function sidebarPreferencesQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: sidebarKeys.preferences(workspaceSlug),
    queryFn: () => getSidebarPreferences(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The caller's sidebar preferences for one workspace. */
export function useSidebarPreferences(workspaceSlug: string) {
  return useQuery(sidebarPreferencesQueryOptions(getAppClient(), workspaceSlug));
}

const SIDEBAR_PREFS_FAILED_TITLE = "Couldn't save sidebar preferences";
const SIDEBAR_PREFS_FAILED_BODY = "Your sidebar is unchanged — check your connection and try again.";

/** Persist pin/order triples with optimistic apply and rollback. */
export function useUpdateSidebarPreferences(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const key = sidebarKeys.preferences(workspaceSlug);
  return useMutation<unknown, Error, SidebarPreferencePatch[]>({
    mutationFn: async (items) => {
      const rollback = applyOptimisticUpdate<SidebarPreferences>(queryClient, key, (previous) =>
        applySidebarPatches(previous, items)
      );
      try {
        return await updateSidebarPreferences(getAppClient(), workspaceSlug, items);
      } catch (error) {
        rollback();
        notifyError(SIDEBAR_PREFS_FAILED_TITLE, SIDEBAR_PREFS_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: key });
    },
  });
}
