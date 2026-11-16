// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board card writes (parity/specs/issues.md §5, ISS-037–039). One drop
// paints the moving card's fields across every cached copy through the
// shared optimistic helper (grouped envelopes re-bucket on the patched
// dimension; the settle refetch corrects the rest), then writes the
// issue PATCH beside the cycle/module bridge the plan carries. A refused
// write rolls the paint back, notifies, and re-reads the entity's lists.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { addIssueToCycle, removeIssueFromCycle, setIssueModules, updateIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { withIssueFields } from "../api/optimisticRows.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import type { BoardMovePlan } from "./drag.js";

/** Failure notice shared by every card drop. */
export const BOARD_MOVE_FAILED_TITLE = "Couldn't save the move";
const BOARD_MOVE_FAILED_BODY = "The card is back where it was — check your connection and try again.";

export interface BoardCardMoveInput {
  issueId: string;
  plan: BoardMovePlan;
  /** Memberships the bridges diff against. */
  currentModuleIds: readonly string[];
}

/** Next module set after the plan's add/remove pair. */
export function nextModuleIds(current: readonly string[], plan: BoardMovePlan): string[] {
  if (plan.modules === null) return [...current];
  const kept = current.filter((id) => !plan.modules?.remove.includes(id));
  return [...kept, ...plan.modules.add.filter((id) => !kept.includes(id))];
}

/** Fields the optimistic paint lands on every cached copy of the card. */
export function movePaint(plan: BoardMovePlan, currentModuleIds: readonly string[]): Record<string, unknown> {
  const paint: Record<string, unknown> = { ...plan.patch };
  if (plan.cycle !== null) paint["cycle_id"] = plan.cycle.add;
  if (plan.modules !== null) paint["module_ids"] = nextModuleIds(currentModuleIds, plan);
  return paint;
}

/** Optimistic card move: the PATCH plus whatever bridge the plan carries. */
export function useBoardCardMove(workspaceSlug: string, projectId: string, entity: IssuesEntity) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, BoardCardMoveInput>({
    mutationFn: async ({ issueId, plan, currentModuleIds }) => {
      const client = getAppClient();
      const paint = movePaint(plan, currentModuleIds);
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(snapshot.queryKey, withIssueFields(snapshot.data, issueId, paint));
      }
      try {
        await updateIssue(client, workspaceSlug, projectId, issueId, plan.patch);
        if (plan.cycle !== null) {
          if (plan.cycle.add !== null) {
            await addIssueToCycle(client, workspaceSlug, projectId, plan.cycle.add, issueId);
          } else if (plan.cycle.remove !== null) {
            await removeIssueFromCycle(client, workspaceSlug, projectId, plan.cycle.remove, issueId);
          }
        }
        if (plan.modules !== null) {
          await setIssueModules(client, workspaceSlug, projectId, issueId, {
            modules: plan.modules.add,
            ...(plan.modules.remove.length === 0 ? {} : { removed_modules: plan.modules.remove }),
          });
        }
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(BOARD_MOVE_FAILED_TITLE, BOARD_MOVE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({
        queryKey: issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
      });
    },
  });
  return {
    isMoving: mutation.isPending,
    moveCard: (input: BoardCardMoveInput) => mutation.mutate(input),
  };
}
