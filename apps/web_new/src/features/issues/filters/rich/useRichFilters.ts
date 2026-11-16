// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter persistence (parity/specs/issues.md §9, ISS-095–099).
// The stored `rich_filters` user-properties key is the personal filter
// expression. Writes send the complete expression object, apply
// optimistically, refetch the entity's lists under the mutation loader,
// and roll back with a toast on failure. They never read or write the URL.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PrefsPatch } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError } from "../../../../shared/shell/index.js";
import { issueKeys } from "../../api/keys.js";
import { useOptionalIssuesLoader } from "../../loaders/loaderContext.js";
import {
  applyPrefsPatch,
  entityListsKeyPart,
  mergePrefsResponse,
  writeEntityPrefs,
  type IssuesEntity,
  type PrefsRecord,
} from "../../prefs/entities.js";
import { entityPrefsKeyPart, prefsQueryOptions } from "../../prefs/prefsQuery.js";
import { parseRichExpression, serializeRichExpression, type RichCondition, type RichExpression } from "./expression.js";

const FILTERS_FAILED_TITLE = "Couldn't save filters";
const FILTERS_FAILED_BODY = "Your filters are unchanged — check your connection and try again.";

/** Parse the stored expression off a preference record. */
export function selectRichConditions(record: PrefsRecord): RichCondition[] {
  return parseRichExpression(record.rich_filters);
}

/** Complete-key patch for the rich expression; other keys ride untouched. */
export function buildRichFiltersPatch(record: PrefsRecord, expression: RichExpression): PrefsPatch {
  return { rich_filters: expression };
}

/** One entity's stored rich-filter expression; disabled for temporary rows. */
export function useRichFilters(workspaceSlug: string, entity: IssuesEntity, enabled = true) {
  const query = useQuery({ ...prefsQueryOptions(getAppClient(), workspaceSlug, entity), enabled });
  return {
    ...query,
    conditions: query.data === undefined ? [] : selectRichConditions(query.data),
  };
}

/**
 * Persist the edited condition list as the entity's rich expression.
 * Saved-view rows pass `enabled: false`: their edits live in the view
 * record and only "Update view" patches them.
 */
export function useUpdateRichFilters(workspaceSlug: string, entity: IssuesEntity, enabled = true) {
  const queryClient = useQueryClient();
  const loader = useOptionalIssuesLoader();
  const [failureCount, setFailureCount] = React.useState(0);
  const prefsKey = issueKeys.prefs(workspaceSlug, ...entityPrefsKeyPart(entity));
  const listsKey = issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity));
  const mutation = useMutation<PrefsRecord, Error, readonly RichCondition[]>({
    onError: () => {
      setFailureCount((count) => count + 1);
    },
    mutationFn: async (conditions) => {
      const record = await queryClient.fetchQuery(prefsQueryOptions(getAppClient(), workspaceSlug, entity));
      const expression = serializeRichExpression(conditions);
      const patch = buildRichFiltersPatch(record, expression);
      // Key-scoped updater writes (NEWFRONT-1013): the optimistic, success
      // and rollback writes each touch only the rich_filters key, so an
      // interleaved display or collapse write survives them.
      queryClient.setQueryData<PrefsRecord>(prefsKey, (current) => applyPrefsPatch(current, record, patch));
      try {
        const write = async (): Promise<PrefsRecord> => {
          const next = await writeEntityPrefs(getAppClient(), workspaceSlug, entity, patch);
          queryClient.setQueryData<PrefsRecord>(prefsKey, (current) =>
            mergePrefsResponse(current, next, ["rich_filters"])
          );
          await queryClient.invalidateQueries({ queryKey: listsKey });
          return next;
        };
        return loader === null ? await write() : await loader.runWithMutationLoader(write);
      } catch (error) {
        queryClient.setQueryData<PrefsRecord>(prefsKey, (current) =>
          mergePrefsResponse(current, record, ["rich_filters"])
        );
        notifyError(FILTERS_FAILED_TITLE, FILTERS_FAILED_BODY);
        throw error;
      }
    },
  });
  return {
    isSaving: mutation.isPending,
    /** Increments after each failed write so the builder can drop its draft. */
    failureCount,
    updateRichFilters: enabled ? (conditions: readonly RichCondition[]) => mutation.mutate(conditions) : () => {},
  };
}
