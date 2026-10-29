// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collapsed list/board sections (parity/specs/issues.md §2, ISS-008).
// One collapsed-key set lives in the entity's stored `filters.
// kanban_filters.group_by` array, so collapsing on the list hides the
// same column on the board and survives a reload. The helpers are pure;
// the hook writes through the shared preference record.

import { useMutation, useQueryClient } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import {
  applyPrefsPatch,
  mergePrefsResponse,
  writeEntityPrefs,
  type IssuesEntity,
  type PrefsRecord,
} from "../prefs/entities.js";
import { entityPrefsKeyPart, prefsQueryOptions } from "../prefs/prefsQuery.js";

/** One entity's collapsed group keys (list and board share them). */
export interface CollapsedGroups {
  group_by: string[];
  sub_group_by: string[];
}

/** Collapsed keys off a stored filter record; malformed entries drop. */
export function readCollapsedGroups(filters: Record<string, unknown>): CollapsedGroups {
  const raw = filters["kanban_filters"];
  const record = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const strings = (value: unknown): string[] =>
    Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : [];
  return {
    group_by: strings(record["group_by"]),
    sub_group_by: strings(record["sub_group_by"]),
  };
}

/** Next set with `key` toggled in one axis. */
export function toggleCollapsed(current: CollapsedGroups, axis: keyof CollapsedGroups, key: string): CollapsedGroups {
  const list = current[axis];
  return {
    ...current,
    [axis]: list.includes(key) ? list.filter((entry) => entry !== key) : [...list, key],
  };
}

/** Whether a section key is collapsed. */
export function isCollapsed(current: CollapsedGroups, axis: keyof CollapsedGroups, key: string): boolean {
  return current[axis].includes(key);
}

/** Next filter object carrying the complete collapsed set. */
export function withCollapsedFilters(
  filters: Record<string, unknown>,
  collapsed: CollapsedGroups
): Record<string, unknown> {
  return { ...filters, kanban_filters: { ...collapsed } };
}

const COLLAPSE_FAILED_TITLE = "Couldn't save the collapsed section";
const COLLAPSE_FAILED_BODY = "The section stays open — check your connection and try again.";

/** Persist one axis' collapsed set optimistically, rolling back on failure. */
export function useToggleCollapsedGroup(workspaceSlug: string, entity: IssuesEntity) {
  const queryClient = useQueryClient();
  const prefsKey = issueKeys.prefs(workspaceSlug, ...entityPrefsKeyPart(entity));
  const mutation = useMutation<PrefsRecord, Error, { axis: keyof CollapsedGroups; key: string }>({
    mutationFn: async ({ axis, key }) => {
      const record = await queryClient.fetchQuery(prefsQueryOptions(getAppClient(), workspaceSlug, entity));
      const next = toggleCollapsed(readCollapsedGroups(record.filters), axis, key);
      const filters = withCollapsedFilters(record.filters, next);
      // Key-scoped updater writes (NEWFRONT-1013): the optimistic, success
      // and rollback writes each touch only the filters key, so an
      // interleaved display or rich-filter write survives them.
      queryClient.setQueryData<PrefsRecord>(prefsKey, (current) => applyPrefsPatch(current, record, { filters }));
      try {
        const response = await writeEntityPrefs(getAppClient(), workspaceSlug, entity, { filters });
        queryClient.setQueryData<PrefsRecord>(prefsKey, (current) =>
          mergePrefsResponse(current, response, ["filters"])
        );
        return response;
      } catch (error) {
        queryClient.setQueryData<PrefsRecord>(prefsKey, (current) => mergePrefsResponse(current, record, ["filters"]));
        notifyError(COLLAPSE_FAILED_TITLE, COLLAPSE_FAILED_BODY);
        throw error;
      }
    },
  });
  return {
    isSaving: mutation.isPending,
    toggleCollapsedGroup: (axis: keyof CollapsedGroups, key: string) => mutation.mutate({ axis, key }),
  };
}
