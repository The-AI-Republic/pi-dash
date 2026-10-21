// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contextual palette mounting hooks (shell spec, SHELL-090/091). Entity
// screens call one hook with their subject plus feature data; the hook
// loads the picker lists, wires the thin mutations, and registers the
// entries while mounted. Routes assemble plain data from their feature
// hooks (shared/ never imports features/) and render the returned
// delete dialog beside their content.

import type { IssuePatch } from "@pidash/api-client";
import * as React from "react";

import { registerCommands, registerPalettePages } from "../../commands/registry.js";
import { useCommandPalette } from "../../commands/store.js";
import { copyLinkWithToast, workItemPath } from "../shareLink.js";
import { notifyError, notifySuccess } from "../toaster.js";
import {
  selectNamedProjectMembers,
  useCycleModuleMutations,
  usePageMutations,
  usePaletteCycles,
  usePaletteEstimates,
  usePaletteFavorites,
  usePaletteLabels,
  usePaletteModules,
  usePaletteProjectMembers,
  usePaletteStates,
  usePaletteSubscription,
  usePaletteWorkspaceMembers,
  useWorkItemMutations,
  selectFavoriteId,
} from "./api.js";
import { selectCycleContext } from "./cycles.js";
import { DeleteWorkItemDialog } from "./deleteDialog.js";
import { selectModuleContext } from "./modules.js";
import { selectPageCapabilities, selectPageContext } from "./pages.js";
import { workItemNoun, type ProjectStanding } from "./types.js";
import { selectWorkItemContext, type WorkItemPickerData } from "./workItems.js";

async function copyText(text: string, ok: string, fail: string): Promise<void> {
  try {
    if (typeof navigator === "undefined" || !navigator.clipboard) throw new Error("no clipboard");
    await navigator.clipboard.writeText(text);
    notifySuccess(ok);
  } catch {
    notifyError(fail);
  }
}

export interface WorkItemPaletteInput {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Route key shown in the chip (e.g. "PROJ-123"). */
  issueKey: string;
  issueName: string;
  stateId: string | null;
  priority: "urgent" | "high" | "medium" | "low" | "none";
  assigneeIds: string[];
  labelIds: string[];
  estimatePoint: string | number | null | undefined;
  cycleId: string | null;
  moduleIds: string[];
  isEpic: boolean;
  parentId: string | null;
  archived: boolean;
  estimatesEnabled: boolean;
  cycleView: boolean;
  moduleView: boolean;
  meId: string | null;
  standing: ProjectStanding;
  /** Whether this viewer may delete the item (creator or project admin, ISS-222). */
  mayDelete: boolean;
  /** Re-read the mounting screen after a mutation lands. */
  refresh: () => Promise<unknown>;
  /** Leave the detail view after a confirmed delete. */
  onDeleted: () => void;
}

export interface WorkItemPaletteBinding {
  /** Render beside the screen content; opens from the Delete entry. */
  deleteDialog: React.ReactNode;
  /** True while the delete write is in flight (for route-level guards). */
  deleting: boolean;
}

/** Register the work-item contextual group for a mounted detail screen. */
export function useWorkItemPaletteContext(input: WorkItemPaletteInput): WorkItemPaletteBinding {
  const noun = workItemNoun({ isEpic: input.isEpic, parentId: input.parentId });
  const states = usePaletteStates(input.workspaceSlug, input.projectId);
  const labels = usePaletteLabels(input.workspaceSlug, input.projectId);
  const projectMembers = usePaletteProjectMembers(input.workspaceSlug, input.projectId);
  const workspaceMembers = usePaletteWorkspaceMembers(input.workspaceSlug);
  const estimates = usePaletteEstimates(input.workspaceSlug, input.projectId);
  const cycles = usePaletteCycles(input.workspaceSlug, input.projectId);
  const modules = usePaletteModules(input.workspaceSlug, input.projectId);
  const subscription = usePaletteSubscription(input.workspaceSlug, input.projectId, input.issueId);
  const mutations = useWorkItemMutations({
    workspaceSlug: input.workspaceSlug,
    projectId: input.projectId,
    issueId: input.issueId,
    noun,
    refresh: input.refresh,
  });
  const [deleteOpen, setDeleteOpen] = React.useState(false);

  const members = React.useMemo(
    () => selectNamedProjectMembers(projectMembers.data ?? [], workspaceMembers.data ?? []),
    [projectMembers.data, workspaceMembers.data]
  );
  const pickers = React.useMemo<WorkItemPickerData>(
    () => ({
      states: states.data ?? [],
      labels: labels.data ?? [],
      members,
      estimates: estimates.data ?? [],
      cycles: cycles.data ?? [],
      modules: modules.data ?? [],
      loading:
        states.isPending ||
        labels.isPending ||
        projectMembers.isPending ||
        workspaceMembers.isPending ||
        estimates.isPending ||
        cycles.isPending ||
        modules.isPending,
    }),
    [states, labels, projectMembers, workspaceMembers, estimates, cycles, modules, members]
  );

  const updateFields = React.useCallback((patch: IssuePatch) => mutations.updateFields.mutate(patch), [mutations]);
  const addToCycle = React.useCallback((cycleId: string) => mutations.addToCycle.mutate(cycleId), [mutations]);
  const setModules = React.useCallback(
    (selection: { modules: string[]; removed_modules?: string[] }) => mutations.setModules.mutate(selection),
    [mutations]
  );
  const toggleSubscription = React.useCallback(
    (subscribed: boolean) => mutations.toggleSubscription.mutate(subscribed),
    [mutations]
  );
  const openDelete = React.useCallback(() => setDeleteOpen(true), []);
  const copyId = React.useCallback(
    () => void copyText(input.issueKey, "Copied the work item ID.", "Couldn't copy the work item ID."),
    [input.issueKey]
  );
  const copyTitle = React.useCallback(
    () => void copyText(input.issueName, "Copied the work item title.", "Couldn't copy the work item title."),
    [input.issueName]
  );
  const copyUrl = React.useCallback(
    () =>
      void copyLinkWithToast(
        workItemPath(input.workspaceSlug, input.issueKey),
        { copied: "Copied the work item link.", failed: "Couldn't copy the work item link." },
        { success: notifySuccess, error: notifyError }
      ),
    [input.workspaceSlug, input.issueKey]
  );
  const close = React.useCallback(() => useCommandPalette.getState().closePalette(), []);

  const selection = React.useMemo(
    () =>
      selectWorkItemContext({
        workspaceSlug: input.workspaceSlug,
        projectId: input.projectId,
        issueId: input.issueId,
        issueKey: input.issueKey,
        stateId: input.stateId,
        priority: input.priority,
        assigneeIds: input.assigneeIds,
        labelIds: input.labelIds,
        estimatePoint: input.estimatePoint,
        cycleId: input.cycleId,
        moduleIds: input.moduleIds,
        archived: input.archived,
        estimatesEnabled: input.estimatesEnabled,
        cycleView: input.cycleView,
        moduleView: input.moduleView,
        meId: input.meId,
        subscribed: subscription.data?.subscribed ?? false,
        standing: input.standing,
        mayDelete: input.mayDelete,
        pickers,
        actions: {
          updateFields,
          addToCycle,
          setModules,
          toggleSubscription,
          openDelete,
          copyId,
          copyTitle,
          copyUrl,
          close,
        },
      }),
    [
      input.workspaceSlug,
      input.projectId,
      input.issueId,
      input.issueKey,
      input.stateId,
      input.priority,
      input.assigneeIds,
      input.labelIds,
      input.estimatePoint,
      input.cycleId,
      input.moduleIds,
      input.archived,
      input.estimatesEnabled,
      input.cycleView,
      input.moduleView,
      input.meId,
      subscription.data?.subscribed,
      input.standing,
      input.mayDelete,
      pickers,
      updateFields,
      addToCycle,
      setModules,
      toggleSubscription,
      openDelete,
      copyId,
      copyTitle,
      copyUrl,
      close,
    ]
  );

  React.useEffect(() => registerCommands(selection.commands), [selection.commands]);
  React.useEffect(() => registerPalettePages(selection.pages), [selection.pages]);

  const confirmDelete = React.useCallback(() => {
    mutations.remove.mutate(undefined, {
      onSuccess: () => {
        setDeleteOpen(false);
        input.onDeleted();
      },
    });
  }, [mutations, input]);

  const deleteDialog = (
    <DeleteWorkItemDialog
      open={deleteOpen}
      onOpenChange={setDeleteOpen}
      noun={noun}
      title={input.issueName}
      deleting={mutations.remove.isPending}
      onConfirm={confirmDelete}
    />
  );
  return { deleteDialog, deleting: mutations.remove.isPending };
}

export interface CyclePaletteInput {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  cycleName: string;
  archived: boolean;
  favorite: boolean;
  standing: ProjectStanding;
  refresh: () => Promise<unknown>;
}

/** Register the cycle contextual entries for a mounted cycle screen. */
export function useCyclePaletteContext(input: CyclePaletteInput): void {
  const mutations = useCycleModuleMutations({
    workspaceSlug: input.workspaceSlug,
    projectId: input.projectId,
    refresh: input.refresh,
  });
  const favorites = usePaletteFavorites(input.workspaceSlug);

  const toggleFavorite = React.useCallback(() => {
    mutations.toggleFavorite.mutate({
      entityType: "cycle",
      entityId: input.cycleId,
      favoriteId: selectFavoriteId(favorites.data ?? [], "cycle", input.cycleId),
    });
  }, [mutations, favorites.data, input.cycleId]);
  const copyUrl = React.useCallback(
    () =>
      void copyText(
        typeof window === "undefined" ? "" : window.location.href,
        "Copied the cycle link.",
        "Couldn't copy the cycle link."
      ),
    []
  );

  const commands = React.useMemo(
    () =>
      selectCycleContext({
        cycleId: input.cycleId,
        cycleName: input.cycleName,
        archived: input.archived,
        favorite: input.favorite,
        standing: input.standing,
        toggleFavorite,
        copyUrl,
      }),
    [input.cycleId, input.cycleName, input.archived, input.favorite, input.standing, toggleFavorite, copyUrl]
  );
  React.useEffect(() => registerCommands(commands), [commands]);
}

export interface ModulePaletteInput {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  moduleName: string;
  archived: boolean;
  status: string;
  memberIds: string[];
  favorite: boolean;
  standing: ProjectStanding;
  refresh: () => Promise<unknown>;
}

/** Register the module contextual entries for a mounted module screen. */
export function useModulePaletteContext(input: ModulePaletteInput): void {
  const mutations = useCycleModuleMutations({
    workspaceSlug: input.workspaceSlug,
    projectId: input.projectId,
    refresh: input.refresh,
  });
  const projectMembers = usePaletteProjectMembers(input.workspaceSlug, input.projectId);
  const workspaceMembers = usePaletteWorkspaceMembers(input.workspaceSlug);
  const favorites = usePaletteFavorites(input.workspaceSlug);

  const members = React.useMemo(
    () => selectNamedProjectMembers(projectMembers.data ?? [], workspaceMembers.data ?? []),
    [projectMembers.data, workspaceMembers.data]
  );
  const toggleMember = React.useCallback(
    (userId: string, member: boolean) => {
      mutations.updateModuleFields.mutate({
        moduleId: input.moduleId,
        patch: {
          member_ids: member ? input.memberIds.filter((id) => id !== userId) : [...input.memberIds, userId],
        },
      });
    },
    [mutations, input.moduleId, input.memberIds]
  );
  const setStatus = React.useCallback(
    (status: string) => mutations.updateModuleFields.mutate({ moduleId: input.moduleId, patch: { status } }),
    [mutations, input.moduleId]
  );
  const toggleFavorite = React.useCallback(() => {
    mutations.toggleFavorite.mutate({
      entityType: "module",
      entityId: input.moduleId,
      favoriteId: selectFavoriteId(favorites.data ?? [], "module", input.moduleId),
    });
  }, [mutations, favorites.data, input.moduleId]);
  const copyUrl = React.useCallback(
    () =>
      void copyText(
        typeof window === "undefined" ? "" : window.location.href,
        "Copied the module link.",
        "Couldn't copy the module link."
      ),
    []
  );
  const close = React.useCallback(() => useCommandPalette.getState().closePalette(), []);

  const selection = React.useMemo(
    () =>
      selectModuleContext({
        moduleId: input.moduleId,
        moduleName: input.moduleName,
        archived: input.archived,
        status: input.status,
        memberIds: input.memberIds,
        favorite: input.favorite,
        standing: input.standing,
        members,
        membersLoading: projectMembers.isPending || workspaceMembers.isPending,
        toggleMember,
        setStatus,
        toggleFavorite,
        copyUrl,
        close,
      }),
    [
      input.moduleId,
      input.moduleName,
      input.archived,
      input.status,
      input.memberIds,
      input.favorite,
      input.standing,
      members,
      projectMembers.isPending,
      workspaceMembers.isPending,
      toggleMember,
      setStatus,
      toggleFavorite,
      copyUrl,
      close,
    ]
  );
  React.useEffect(() => registerCommands(selection.commands), [selection.commands]);
  React.useEffect(() => registerPalettePages(selection.pages), [selection.pages]);
}

export interface PagePaletteInput {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  pageName: string;
  locked: boolean;
  access: number;
  archived: boolean;
  favorite: boolean;
  isOwner: boolean;
  highestProjectRole: number | null;
  refresh: () => Promise<unknown>;
}

/** Register the page contextual entries for a mounted page screen. */
export function usePagePaletteContext(input: PagePaletteInput): void {
  const mutations = usePageMutations({
    workspaceSlug: input.workspaceSlug,
    projectId: input.projectId,
    refresh: input.refresh,
  });

  const setLocked = React.useCallback(
    (locked: boolean) => mutations.setLocked.mutate({ pageId: input.pageId, locked }),
    [mutations, input.pageId]
  );
  const setAccess = React.useCallback(
    (access: number) => mutations.setAccess.mutate({ pageId: input.pageId, access }),
    [mutations, input.pageId]
  );
  const setArchived = React.useCallback(
    (archived: boolean) => mutations.setArchived.mutate({ pageId: input.pageId, archived }),
    [mutations, input.pageId]
  );
  const toggleFavorite = React.useCallback(
    () => mutations.toggleFavorite.mutate({ pageId: input.pageId, favorite: !input.favorite }),
    [mutations, input.pageId, input.favorite]
  );
  const copyUrl = React.useCallback(
    () =>
      void copyText(
        typeof window === "undefined" ? "" : window.location.href,
        "Copied the page link.",
        "Couldn't copy the page link."
      ),
    []
  );

  const commands = React.useMemo(
    () =>
      selectPageContext({
        pageId: input.pageId,
        pageName: input.pageName,
        locked: input.locked,
        access: input.access,
        archived: input.archived,
        favorite: input.favorite,
        capabilities: selectPageCapabilities({ isOwner: input.isOwner, highestProjectRole: input.highestProjectRole }),
        setLocked,
        setAccess,
        setArchived,
        toggleFavorite,
        copyUrl,
      }),
    [
      input.pageId,
      input.pageName,
      input.locked,
      input.access,
      input.archived,
      input.favorite,
      input.isOwner,
      input.highestProjectRole,
      setLocked,
      setAccess,
      setArchived,
      toggleFavorite,
      copyUrl,
    ]
  );
  React.useEffect(() => registerCommands(commands), [commands]);
}
