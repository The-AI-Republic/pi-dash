// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Display preferences data layer (parity/specs/issues.md §1, ISS-002).
// Layout, grouping, sort order, empty-group visibility, calendar options
// and visible properties persist per user per entity on the
// user-properties endpoints. Reads happen once per mount and never on
// window focus; writes send complete key objects (PATCH replaces each
// sent key wholesale), apply optimistically, roll back with a toast on
// failure, and refetch the entity's lists under the mutation loader.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PrefsPatch } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { parseLayoutKind, type LayoutKind } from "../layouts/kinds.js";
import { useIssuesLoader } from "../loaders/loaderContext.js";
import {
  applyPrefsPatch,
  entityListsKeyPart,
  mergePrefsResponse,
  writeEntityPrefs,
  type IssuesEntity,
  type PrefsRecord,
} from "./entities.js";
import { entityPrefsKeyPart, prefsQueryOptions } from "./prefsQuery.js";

/** Calendar options inside display_filters.calendar. */
export interface DisplayCalendarPrefs {
  mode: "month" | "week";
  showWeekends: boolean;
}

/** Parsed per-entity display preferences (ISS-002, ISS-079). */
export interface DisplayPrefs {
  layout: LayoutKind;
  groupBy: string | null;
  subGroupBy: string | null;
  orderBy: string;
  showEmptyGroups: boolean;
  showSubIssues: boolean;
  calendar: DisplayCalendarPrefs;
  /** Visible property columns by key; absent keys read as shown. */
  properties: Record<string, boolean>;
}

const DEFAULT_ORDER_BY = "sort_order";

function parseGroupKey(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function parseOrderBy(value: unknown): string {
  return typeof value === "string" && value.length > 0 ? value : DEFAULT_ORDER_BY;
}

function parseCalendar(value: unknown): DisplayCalendarPrefs {
  const record = typeof value === "object" && value !== null ? (value as Record<string, unknown>) : null;
  return {
    mode: record?.["layout"] === "week" ? "week" : "month",
    // The old calendar starts with weekends hidden when the preference
    // key is absent; only an explicit true opts back into seven columns.
    showWeekends: record?.["show_weekends"] === true,
  };
}

function parseProperties(value: unknown): Record<string, boolean> {
  if (typeof value !== "object" || value === null) return {};
  const properties: Record<string, boolean> = {};
  for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
    if (typeof entry === "boolean") properties[key] = entry;
  }
  return properties;
}

/**
 * Parse the stored filter keys into display preferences. Empty groups
 * default to hidden because the ISS-002 oracle restores grouping without a
 * show_empty_groups key and sees only the populated group. Sub-issues are
 * the opposite: ISS-079 defines the switch as on by default, and list reads
 * need that default to include child rows when a record predates the key.
 * Fresh server records seed both keys, so the legacy default only matters
 * for old or reset records.
 */
export function parseDisplayPrefs(
  displayFilters: Record<string, unknown>,
  displayProperties: Record<string, unknown>
): DisplayPrefs {
  return {
    layout: parseLayoutKind(displayFilters["layout"]),
    groupBy: parseGroupKey(displayFilters["group_by"]),
    subGroupBy: parseGroupKey(displayFilters["sub_group_by"]),
    orderBy: parseOrderBy(displayFilters["order_by"]),
    showEmptyGroups: displayFilters["show_empty_groups"] === true,
    showSubIssues: displayFilters["sub_issue"] !== false,
    calendar: parseCalendar(displayFilters["calendar"]),
    properties: parseProperties(displayProperties),
  };
}

/** Parse one preference record's display keys. */
export function selectDisplayPrefs(record: PrefsRecord): DisplayPrefs {
  return parseDisplayPrefs(record.display_filters, record.display_properties);
}

/** One entity's raw preference record; temporary rows disable the read. */
export function useEntityPrefs(workspaceSlug: string, entity: IssuesEntity, enabled = true) {
  return useQuery({ ...prefsQueryOptions(getAppClient(), workspaceSlug, entity), enabled });
}

function mergeCalendar(
  current: DisplayCalendarPrefs,
  partial: Partial<DisplayCalendarPrefs> | undefined
): DisplayCalendarPrefs {
  return { ...current, ...partial };
}

/** One display settings object with a partial change applied. */
export function mergeDisplayPrefs(current: DisplayPrefs, partial: Partial<DisplayPrefs>): DisplayPrefs {
  return { ...current, ...partial, calendar: mergeCalendar(current.calendar, partial.calendar) };
}

/**
 * The display transition for a layout switch (ISS-243). A switch to the
 * board seeds state grouping when none is set and drops a sub-grouping
 * that would equal the grouping, so the board opens in state columns
 * instead of one flat column; every other switch only moves the layout.
 */
export function switchLayoutPrefs(current: DisplayPrefs, kind: LayoutKind): Partial<DisplayPrefs> {
  if (kind !== "kanban") return { layout: kind };
  if (current.groupBy === null) return { layout: kind, groupBy: "state", subGroupBy: null };
  return {
    layout: kind,
    groupBy: current.groupBy,
    subGroupBy: current.subGroupBy === current.groupBy ? null : current.subGroupBy,
  };
}

/**
 * Complete display_filters/display_properties objects for a partial
 * update. Unknown stored keys ride along untouched inside each object;
 * the filters/rich_filters top-level keys are never sent, so the server
 * keeps them as they are.
 */
export function buildPrefsPatch(record: PrefsRecord, partial: Partial<DisplayPrefs>): PrefsPatch {
  const current = selectDisplayPrefs(record);
  const next = mergeDisplayPrefs(current, partial);
  const storedCalendar =
    typeof record.display_filters["calendar"] === "object" && record.display_filters["calendar"] !== null
      ? (record.display_filters["calendar"] as Record<string, unknown>)
      : {};
  return {
    display_filters: {
      ...record.display_filters,
      layout: next.layout,
      group_by: next.groupBy,
      sub_group_by: next.subGroupBy,
      order_by: next.orderBy,
      show_empty_groups: next.showEmptyGroups,
      sub_issue: next.showSubIssues,
      calendar: { ...storedCalendar, layout: next.calendar.mode, show_weekends: next.calendar.showWeekends },
    },
    display_properties: { ...record.display_properties, ...next.properties },
  };
}

const PREFS_FAILED_TITLE = "Couldn't save display preferences";
const PREFS_FAILED_BODY = "Your issues view is unchanged — check your connection and try again.";

/** Persist display preferences with optimistic apply and rollback. */
export function useUpdateDisplayPrefs(workspaceSlug: string, entity: IssuesEntity) {
  const queryClient = useQueryClient();
  const loader = useIssuesLoader();
  const prefsKey = issueKeys.prefs(workspaceSlug, ...entityPrefsKeyPart(entity));
  const listsKey = issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity));
  const mutation = useMutation<PrefsRecord, Error, Partial<DisplayPrefs>>({
    mutationFn: async (partial) => {
      // fetchQuery serves the cached record without refetching (the query
      // never goes stale) and reads the server only when nothing is cached,
      // so the patch below always merges over complete key objects.
      const record = await queryClient.fetchQuery(prefsQueryOptions(getAppClient(), workspaceSlug, entity));
      const patch = buildPrefsPatch(record, partial);
      // The fetched record doubles as the rollback snapshot: fetchQuery
      // always caches what it resolves, so the optimistic write below
      // never runs on an empty cache. All three writes are key-scoped
      // updaters (NEWFRONT-1013): they touch only the display keys, so an
      // interleaved collapse or rich-filter write survives them.
      queryClient.setQueryData<PrefsRecord>(prefsKey, (current) => applyPrefsPatch(current, record, patch));
      try {
        return await loader.runWithMutationLoader(async () => {
          const next = await writeEntityPrefs(getAppClient(), workspaceSlug, entity, patch);
          queryClient.setQueryData<PrefsRecord>(prefsKey, (current) =>
            mergePrefsResponse(current, next, ["display_filters", "display_properties"])
          );
          // Awaited: the loader covers the list refetch, not just the PATCH.
          await queryClient.invalidateQueries({ queryKey: listsKey });
          return next;
        });
      } catch (error) {
        queryClient.setQueryData<PrefsRecord>(prefsKey, (current) =>
          mergePrefsResponse(current, record, ["display_filters", "display_properties"])
        );
        notifyError(PREFS_FAILED_TITLE, PREFS_FAILED_BODY);
        throw error;
      }
    },
  });
  return {
    isSaving: mutation.isPending,
    updateDisplayPrefs: (partial: Partial<DisplayPrefs>) => mutation.mutate(partial),
  };
}
