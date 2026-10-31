// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab reads and writes (parity/specs/project-settings-states.md,
// PSET-028–035). One cached list per project; writes patch it
// optimistically where the inventory says Intended (update, move,
// promotion) and restore it with a notice when the server refuses.
// Creates and deletes never paint before the server answers.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createState,
  deleteState,
  listStates,
  markStateDefault,
  updateProject,
  updateState,
  type ApiClient,
  type ProjectDetail,
  type State,
  type StateCreateInput,
  type StateList,
  type StateUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";
import { snapshotQueries, rollbackSnapshots, type QuerySnapshot } from "../../core/query/optimistic.js";

/** Snapshots captured by an optimistic mutation, or none for a skipped hook. */
export function snapshotsOf(context: { snapshots: QuerySnapshot[] } | undefined): QuerySnapshot[] {
  return context?.snapshots ?? [];
}
import { projectQueryOptions } from "../auth/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { compareStateSequence, isWorkflowGroup, type WorkflowGroup } from "./groups.js";
import {
  isDuplicateStateError,
  isOccupiedStateError,
  MEMBER_EDITING_FAILED_TITLE,
  MEMBER_EDITING_OFF_TITLE,
  MEMBER_EDITING_ON_TITLE,
  STATE_CREATED_TITLE,
  STATE_CREATE_FAILED_TITLE,
  STATE_DEFAULT_FAILED_TITLE,
  STATE_DEFAULT_TITLE,
  STATE_DELETE_FAILED_TITLE,
  STATE_DELETE_OCCUPIED_MESSAGE,
  STATE_DELETE_OCCUPIED_TITLE,
  STATE_DELETED_TITLE,
  STATE_DUPLICATE_CREATE_MESSAGE,
  STATE_DUPLICATE_RENAME_MESSAGE,
  STATE_UPDATE_FAILED_TITLE,
  STATE_UPDATED_TITLE,
  stateFailureDetail,
} from "./notices.js";

/** Query keys for the states cache; every key sits under the workspace. */
export const stateKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "project-states"] as const,
  list: (workspaceSlug: string, projectId: string) => [...stateKeys.all(workspaceSlug), "list", projectId] as const,
};

/** One project's workflow states; reference data with shared freshness. */
export function statesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: stateKeys.list(workspaceSlug, projectId),
    queryFn: () => listStates(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** The project's state list for the tab. */
export function useProjectStates(workspaceSlug: string, projectId: string) {
  return useQuery(statesQueryOptions(getAppClient(), workspaceSlug, projectId));
}

// --- pure cache transitions (exported for tests) -------------------------

/** Replace one row in place, preserving list order. */
export function applyStatePatch(
  previous: StateList | undefined,
  stateId: string,
  patch: Partial<State>
): StateList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === stateId ? { ...row, ...patch } : row));
}

/** Append a created row to the cache. */
export function appendState(previous: StateList | undefined, created: State): StateList {
  return previous === undefined ? [created] : [...previous, created];
}

/** Remove one row from the cache. */
export function removeState(previous: StateList | undefined, stateId: string): StateList | undefined {
  return previous?.filter((row) => row.id !== stateId);
}

/**
 * Make one row the project default and clear every other flag — the
 * client-side half of the exclusive server rule (PSET-033).
 */
export function applyDefaultFlag(previous: StateList | undefined, stateId: string): StateList | undefined {
  return previous?.map((row) => ({ ...row, default: row.id === stateId }));
}

/** Move a row to another group at a new ordering value. */
export function applyStateMove(
  previous: StateList | undefined,
  move: { stateId: string; group: WorkflowGroup; sequence: number }
): StateList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) =>
    row.id === move.stateId ? { ...row, group: move.group, sequence: move.sequence } : row
  );
}

/**
 * A freshly created row as the server will answer it, for callers that
 * need the shape of a row before the write lands. Kept pure so the
 * preview math (share, ordering) is testable.
 */
export function sequenceAfterGroup(states: readonly State[], group: WorkflowGroup): number {
  const rows = states
    .filter((row) => row.group === group)
    .slice()
    .sort(compareStateSequence);
  const last = rows[rows.length - 1];
  return last === undefined ? 0 : last.sequence + 1;
}

// --- mutations ------------------------------------------------------------

export interface CreateStateVariables {
  workspaceSlug: string;
  projectId: string;
  group: WorkflowGroup;
  input: Omit<StateCreateInput, "group">;
}

/**
 * Create a state inside a group (PSET-029). The row appears only once the
 * server has accepted it; a duplicate or triage refusal keeps the calling
 * form open, so the form owns its error text and this hook only reports
 * the notice. Success closes the form through `onSettled` invalidation.
 */
export function useCreateState() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (variables: CreateStateVariables) => {
      if (!isWorkflowGroup(variables.group)) throw new Error("Unknown workflow group");
      const result = await createState(getAppClient(), variables.workspaceSlug, variables.projectId, {
        ...variables.input,
        group: variables.group,
      } satisfies StateCreateInput);
      notifySuccess(STATE_CREATED_TITLE);
      return result.state;
    },
    onError: (error) => {
      if (isDuplicateStateError(error)) {
        notifyError(STATE_CREATE_FAILED_TITLE, STATE_DUPLICATE_CREATE_MESSAGE);
        return;
      }
      notifyError(STATE_CREATE_FAILED_TITLE, stateFailureDetail(error));
    },
    onSettled: (_data, _error, variables) => {
      void queryClient.invalidateQueries({ queryKey: stateKeys.list(variables.workspaceSlug, variables.projectId) });
    },
  });
}

export interface UpdateStateVariables {
  workspaceSlug: string;
  projectId: string;
  stateId: string;
  patch: Partial<State>;
  /** Fields to PATCH; defaults to the patch itself. */
  input?: StateUpdateInput;
}

/**
 * Rename, re-describe or recolor a state (PSET-030). The row changes
 * optimistically; a refused save restores it and notifies. Duplicate
 * renames get the specific message (Intended, bug NEWFRONT-402) and the
 * form stays open because the caller closes it only on success.
 */
export function useUpdateState() {
  const queryClient = useQueryClient();
  return useMutation<State, Error, UpdateStateVariables, { snapshots: QuerySnapshot[] }>({
    mutationFn: async (variables) => {
      const state = await updateState(
        getAppClient(),
        variables.workspaceSlug,
        variables.projectId,
        variables.stateId,
        variables.input ?? (variables.patch as StateUpdateInput)
      );
      return state;
    },
    onMutate: async (variables) => {
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      await queryClient.cancelQueries({ queryKey: key });
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<StateList | undefined>(key, (previous) =>
        applyStatePatch(previous, variables.stateId, variables.patch)
      );
      return { snapshots };
    },
    onError: (error, variables, context) => {
      rollbackSnapshots(queryClient, snapshotsOf(context));
      if (isDuplicateStateError(error)) {
        notifyError(STATE_UPDATE_FAILED_TITLE, STATE_DUPLICATE_RENAME_MESSAGE);
        return;
      }
      notifyError(STATE_UPDATE_FAILED_TITLE, stateFailureDetail(error));
    },
    onSuccess: (state, variables) => {
      notifySuccess(STATE_UPDATED_TITLE);
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => applyStatePatch(previous, state.id, state));
    },
  });
}

export interface MoveStateVariables {
  workspaceSlug: string;
  projectId: string;
  move: { stateId: string; group: WorkflowGroup; sequence: number };
}

/**
 * Drag a state to a new slot (PSET-031). The placement paints at once;
 * a refused save restores the old place and notifies (Intended, bug
 * NEWFRONT-403).
 */
export function useMoveState() {
  const queryClient = useQueryClient();
  return useMutation<State, Error, MoveStateVariables, { snapshots: QuerySnapshot[] }>({
    mutationFn: async (variables) =>
      updateState(getAppClient(), variables.workspaceSlug, variables.projectId, variables.move.stateId, {
        group: variables.move.group,
        sequence: variables.move.sequence,
      }),
    onMutate: async (variables) => {
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      await queryClient.cancelQueries({ queryKey: key });
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => applyStateMove(previous, variables.move));
      return { snapshots };
    },
    onError: (error, _variables, context) => {
      rollbackSnapshots(queryClient, snapshotsOf(context));
      notifyError(STATE_UPDATE_FAILED_TITLE, stateFailureDetail(error));
    },
    onSuccess: (state, variables) => {
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => applyStatePatch(previous, state.id, state));
    },
  });
}

export interface DeleteStateVariables {
  workspaceSlug: string;
  projectId: string;
  stateId: string;
}

/** Delete an empty, non-default state (PSET-032); the row leaves only on success. */
export function useDeleteState() {
  const queryClient = useQueryClient();
  return useMutation<number, Error, DeleteStateVariables>({
    mutationFn: async (variables) =>
      deleteState(getAppClient(), variables.workspaceSlug, variables.projectId, variables.stateId),
    onSuccess: (_status, variables) => {
      notifySuccess(STATE_DELETED_TITLE);
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => removeState(previous, variables.stateId));
    },
    onError: (error) => {
      if (isOccupiedStateError(error)) {
        notifyError(STATE_DELETE_OCCUPIED_TITLE, STATE_DELETE_OCCUPIED_MESSAGE);
        return;
      }
      notifyError(STATE_DELETE_FAILED_TITLE, stateFailureDetail(error));
    },
  });
}

export interface MarkDefaultVariables {
  workspaceSlug: string;
  projectId: string;
  stateId: string;
}

/**
 * Promote a state to the project default (PSET-033). The exclusive flag
 * flips optimistically and restores with a notice on failure (Intended,
 * bug NEWFRONT-403).
 */
export function useMarkStateDefault() {
  const queryClient = useQueryClient();
  return useMutation<number, Error, MarkDefaultVariables, { snapshots: QuerySnapshot[] }>({
    mutationFn: async (variables) =>
      markStateDefault(getAppClient(), variables.workspaceSlug, variables.projectId, variables.stateId),
    onMutate: async (variables) => {
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      await queryClient.cancelQueries({ queryKey: key });
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => applyDefaultFlag(previous, variables.stateId));
      return { snapshots };
    },
    onError: (error, _variables, context) => {
      rollbackSnapshots(queryClient, snapshotsOf(context));
      notifyError(STATE_DEFAULT_FAILED_TITLE, stateFailureDetail(error));
    },
    onSuccess: (_status, variables) => {
      notifySuccess(STATE_DEFAULT_TITLE);
      const key = stateKeys.list(variables.workspaceSlug, variables.projectId);
      queryClient.setQueryData<StateList | undefined>(key, (previous) => applyDefaultFlag(previous, variables.stateId));
    },
  });
}

export interface MemberEditingVariables {
  workspaceSlug: string;
  projectId: string;
  enabled: boolean;
}

/**
 * Flip the members-may-edit-states flag (PSET-034). The switch reads the
 * new value from the project query, which this write seeds on success so
 * the flip is immediate; a refusal notifies and leaves the switch where
 * it was.
 */
export function useSetMemberEditing() {
  const queryClient = useQueryClient();
  return useMutation<boolean, Error, MemberEditingVariables>({
    mutationFn: async (variables) => {
      const client = getAppClient();
      await updateProject(client, variables.workspaceSlug, variables.projectId, {
        members_can_edit_states: variables.enabled,
      });
      return variables.enabled;
    },
    onSuccess: (enabled, variables) => {
      notifySuccess(enabled ? MEMBER_EDITING_ON_TITLE : MEMBER_EDITING_OFF_TITLE);
      seedProjectFlag(queryClient, variables.workspaceSlug, variables.projectId, enabled);
      void queryClient.invalidateQueries({
        queryKey: projectQueryOptions(getAppClient(), variables.workspaceSlug, variables.projectId).queryKey,
      });
    },
    onError: (error) => {
      notifyError(MEMBER_EDITING_FAILED_TITLE, error.message);
    },
  });
}

/**
 * Paint the flag into the cached project row so the toggle and the
 * derived edit rights read back before the refetch lands.
 */
function seedProjectFlag(queryClient: QueryClient, workspaceSlug: string, projectId: string, enabled: boolean): void {
  const key = projectQueryOptions(getAppClient(), workspaceSlug, projectId).queryKey;
  queryClient.setQueryData<ProjectDetail | undefined>(key, (previous) =>
    previous === undefined ? previous : { ...previous, members_can_edit_states: enabled }
  );
}
