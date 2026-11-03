// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contextual palette data (shell spec, SHELL-090/091). Thin reads and
// writes behind the entity pickers and actions: reference lists, the
// subscription row, the favorites lookup, and every mutation the
// entries offer. Reads live under palette-scoped keys (the states,
// labels, members, estimates, cycles and modules features absorb them
// when they land); mutations refresh through a caller callback because
// the owning routes' keys live in features/, which shared/ cannot
// import. Full endpoint ownership stays in the owning areas.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  addIssueToCycle,
  addPageFavorite,
  archivePage,
  createFavorite,
  deleteFavorite,
  deleteIssue,
  getSubscriptionStatus,
  isApiError,
  listCycles,
  listFavorites,
  listLabels,
  listMembers,
  listModules,
  listProjectEstimates,
  listStates,
  listWorkspaceMembers,
  lockPage,
  removePageFavorite,
  restorePage,
  setIssueModules,
  setPageAccess,
  subscribeIssue,
  unlockPage,
  unsubscribeIssue,
  updateIssue,
  updateModule,
  type ApiClient,
  type CycleList,
  type EstimateList,
  type FavoriteList,
  type IssuePatch,
  type LabelList,
  type ModuleList,
  type ModulePatch,
  type ProjectMemberList,
  type StateList,
  type WorkspaceMemberList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { notifyError, notifySuccess } from "../toaster.js";

export const contextualKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "palette"] as const,
  states: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "states", projectId] as const,
  labels: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "labels", projectId] as const,
  projectMembers: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "project-members", projectId] as const,
  workspaceMembers: (workspaceSlug: string) => [...contextualKeys.all(workspaceSlug), "workspace-members"] as const,
  estimates: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "estimates", projectId] as const,
  cycles: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "cycles", projectId] as const,
  modules: (workspaceSlug: string, projectId: string) =>
    [...contextualKeys.all(workspaceSlug), "modules", projectId] as const,
  subscription: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...contextualKeys.all(workspaceSlug), "subscription", projectId, issueId] as const,
  favorites: (workspaceSlug: string) => [...contextualKeys.all(workspaceSlug), "favorites"] as const,
};

function referenceOptions<T>(key: readonly unknown[], fetch: () => Promise<T>) {
  return queryOptions({
    queryKey: key,
    queryFn: fetch,
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export function paletteStatesQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<StateList>(contextualKeys.states(slug, projectId), () => listStates(client, slug, projectId));
}

export function paletteLabelsQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<LabelList>(contextualKeys.labels(slug, projectId), () => listLabels(client, slug, projectId));
}

export function paletteProjectMembersQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<ProjectMemberList>(contextualKeys.projectMembers(slug, projectId), () =>
    listMembers(client, slug, projectId)
  );
}

export function paletteWorkspaceMembersQueryOptions(client: ApiClient, slug: string) {
  return referenceOptions<WorkspaceMemberList>(contextualKeys.workspaceMembers(slug), () =>
    listWorkspaceMembers(client, slug)
  );
}

export function paletteEstimatesQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<EstimateList>(contextualKeys.estimates(slug, projectId), () =>
    listProjectEstimates(client, slug, projectId)
  );
}

export function paletteCyclesQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<CycleList>(contextualKeys.cycles(slug, projectId), () => listCycles(client, slug, projectId));
}

export function paletteModulesQueryOptions(client: ApiClient, slug: string, projectId: string) {
  return referenceOptions<ModuleList>(contextualKeys.modules(slug, projectId), () =>
    listModules(client, slug, projectId)
  );
}

export function paletteSubscriptionQueryOptions(client: ApiClient, slug: string, projectId: string, issueId: string) {
  return referenceOptions(contextualKeys.subscription(slug, projectId, issueId), () =>
    getSubscriptionStatus(client, slug, projectId, issueId)
  );
}

export function paletteFavoritesQueryOptions(client: ApiClient, slug: string) {
  return referenceOptions<FavoriteList>(contextualKeys.favorites(slug), () => listFavorites(client, slug));
}

/** Project member with its workspace display name joined in. */
export interface NamedProjectMember {
  userId: string;
  role: number;
  displayName: string;
}

/** Join bare project member ids to workspace display names. */
export function selectNamedProjectMembers(
  projectMembers: ProjectMemberList,
  workspaceMembers: WorkspaceMemberList
): NamedProjectMember[] {
  const names = new Map(workspaceMembers.map((entry) => [entry.member.id, entry.member.display_name]));
  return projectMembers.map((entry) => ({
    userId: entry.member,
    role: entry.role,
    displayName: names.get(entry.member) ?? "Unknown member",
  }));
}

/**
 * The favorite row id for an entity, if starred. Cycle and module
 * favorites live in user-favorites; pages favorite separately.
 */
export function selectFavoriteId(
  favorites: FavoriteList,
  entityType: "cycle" | "module",
  entityId: string
): string | null {
  return favorites.find((row) => row.entity_type === entityType && row.entity_identifier === entityId)?.id ?? null;
}

/** Callbacks the mounting route wires to its own queries and navigation. */
export interface ContextualRefresh {
  /** Re-read the mounting screen after a mutation lands. */
  refresh: () => Promise<unknown>;
}

export function usePaletteStates(slug: string, projectId: string) {
  return useQuery(paletteStatesQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteLabels(slug: string, projectId: string) {
  return useQuery(paletteLabelsQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteProjectMembers(slug: string, projectId: string) {
  return useQuery(paletteProjectMembersQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteWorkspaceMembers(slug: string) {
  return useQuery(paletteWorkspaceMembersQueryOptions(getAppClient(), slug));
}

export function usePaletteEstimates(slug: string, projectId: string) {
  return useQuery(paletteEstimatesQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteCycles(slug: string, projectId: string) {
  return useQuery(paletteCyclesQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteModules(slug: string, projectId: string) {
  return useQuery(paletteModulesQueryOptions(getAppClient(), slug, projectId));
}

export function usePaletteSubscription(slug: string, projectId: string, issueId: string) {
  return useQuery(paletteSubscriptionQueryOptions(getAppClient(), slug, projectId, issueId));
}

export function usePaletteFavorites(slug: string) {
  return useQuery(paletteFavoritesQueryOptions(getAppClient(), slug));
}

export interface WorkItemScope extends ContextualRefresh {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  noun: string;
}

/** Field, membership, subscription and delete writes for one work item. */
export function useWorkItemMutations(scope: WorkItemScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const noun = scope.noun.toLowerCase();

  function invalidateSubscription(): Promise<void> {
    return queryClient
      .invalidateQueries({
        queryKey: contextualKeys.subscription(scope.workspaceSlug, scope.projectId, scope.issueId),
      })
      .then(() => undefined);
  }

  const updateFields = useMutation({
    mutationFn: async (patch: IssuePatch) =>
      updateIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId, patch),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError(`Couldn't update the ${noun}.`, error.message),
  });
  const addToCycle = useMutation({
    mutationFn: async (cycleId: string) =>
      addIssueToCycle(client, scope.workspaceSlug, scope.projectId, cycleId, scope.issueId),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError(`Couldn't update the ${noun}.`, error.message),
  });
  const setModules = useMutation({
    mutationFn: async (input: { modules: string[]; removed_modules?: string[] }) =>
      setIssueModules(client, scope.workspaceSlug, scope.projectId, scope.issueId, input),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError(`Couldn't update the ${noun}.`, error.message),
  });
  const toggleSubscription = useMutation({
    mutationFn: async (subscribed: boolean) =>
      subscribed
        ? unsubscribeIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId)
        : subscribeIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId),
    onSuccess: (_row, subscribed) => {
      notifySuccess(
        subscribed ? "Unsubscribed from work item notifications." : "Subscribed to work item notifications."
      );
      void invalidateSubscription().then(() => scope.refresh());
    },
    onError: (error: Error) => notifyError("Couldn't update the subscription.", error.message),
  });
  const remove = useMutation({
    mutationFn: async () => deleteIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId),
    onSuccess: () => notifySuccess(`${scope.noun} deleted.`),
    onError: (error: Error) => {
      if (isApiError(error) && error.status === 403) {
        notifyError(`Only the creator or a project admin can delete this ${noun}.`);
      } else {
        notifyError(`Couldn't delete the ${noun}.`, error.message);
      }
    },
  });
  return { updateFields, addToCycle, setModules, toggleSubscription, remove };
}

export interface EntityScope extends ContextualRefresh {
  workspaceSlug: string;
  projectId: string;
}

/** Favorite, member and status writes for one cycle or module. */
export function useCycleModuleMutations(scope: EntityScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();

  function invalidateFavorites(): Promise<void> {
    return queryClient
      .invalidateQueries({ queryKey: contextualKeys.favorites(scope.workspaceSlug) })
      .then(() => undefined);
  }

  const toggleFavorite = useMutation({
    mutationFn: async (input: {
      entityType: "cycle" | "module";
      entityId: string;
      favoriteId: string | null;
      /** Sent as entity_data.name so the sidebar titles the entry (cycles). */
      entityName?: string | undefined;
    }) =>
      input.favoriteId === null
        ? createFavorite(client, scope.workspaceSlug, {
            entity_type: input.entityType,
            entity_identifier: input.entityId,
            project_id: scope.projectId,
            ...(input.entityName === undefined ? {} : { entity_data: { name: input.entityName } }),
          })
        : deleteFavorite(client, scope.workspaceSlug, input.favoriteId),
    onSuccess: () => {
      void invalidateFavorites().then(() => scope.refresh());
    },
    onError: (error: Error) => notifyError("Couldn't update favorites.", error.message),
  });
  const updateModuleFields = useMutation({
    mutationFn: async (input: { moduleId: string; patch: ModulePatch }) =>
      updateModule(client, scope.workspaceSlug, scope.projectId, input.moduleId, input.patch),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError("Couldn't update the module.", error.message),
  });
  return { toggleFavorite, updateModuleFields };
}

/** Lock, visibility, archive and favorite writes for one page. */
export function usePageMutations(scope: EntityScope) {
  const client = getAppClient();

  const setLocked = useMutation({
    mutationFn: async (input: { pageId: string; locked: boolean }) =>
      input.locked
        ? lockPage(client, scope.workspaceSlug, scope.projectId, input.pageId)
        : unlockPage(client, scope.workspaceSlug, scope.projectId, input.pageId),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError("Couldn't update the page lock.", error.message),
  });
  const setAccess = useMutation({
    mutationFn: async (input: { pageId: string; access: number }) =>
      setPageAccess(client, scope.workspaceSlug, scope.projectId, input.pageId, input.access),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError("Couldn't change page access.", error.message),
  });
  const setArchived = useMutation({
    mutationFn: async (input: { pageId: string; archived: boolean }) =>
      input.archived
        ? archivePage(client, scope.workspaceSlug, scope.projectId, input.pageId)
        : restorePage(client, scope.workspaceSlug, scope.projectId, input.pageId),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError("Couldn't update the page archive.", error.message),
  });
  const toggleFavorite = useMutation({
    mutationFn: async (input: { pageId: string; favorite: boolean }) =>
      input.favorite
        ? addPageFavorite(client, scope.workspaceSlug, scope.projectId, input.pageId)
        : removePageFavorite(client, scope.workspaceSlug, scope.projectId, input.pageId),
    onSuccess: () => void scope.refresh(),
    onError: (error: Error) => notifyError("Couldn't update favorites.", error.message),
  });
  return { setLocked, setAccess, setArchived, toggleFavorite };
}
