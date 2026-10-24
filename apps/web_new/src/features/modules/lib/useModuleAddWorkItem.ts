// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module detail page's add entries (parity/specs/modules.md §8.3,
// MOD-031). The header button, the empty card's two actions and the
// filtered-empty card all read one controller, so a page can never mount a
// second creation dialog or a second picker beside the shared ones. The
// controller knows the standing rule; the page decides nothing about who
// may add.

import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { canAddWorkItemsToModule } from "./moduleAddWorkItem.js";
import { useModuleAddExisting, type ModuleAddExisting } from "./useModuleAddExisting.js";
import { moduleKeys } from "../api/keys.js";

export interface UseModuleAddWorkItemInput {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  /** The project row's membership role; a guest reads false standing. */
  memberRole: number | null;
  /** True while the project is archived. */
  projectArchived: boolean;
  /** The module's stored status. */
  moduleStatus: string | undefined;
}

export interface ModuleAddWorkItemController extends ModuleAddExisting {
  /** Whether any add entry exists on this page. */
  canAdd: boolean;
  createOpen: boolean;
  openCreate: () => void;
  setCreateOpen: (open: boolean) => void;
  pickerOpen: boolean;
  openPicker: () => void;
  setPickerOpen: (open: boolean) => void;
  /** A create landed: close the form and refresh the module's own counts. */
  handleCreated: () => void;
}

export function useModuleAddWorkItem({
  workspaceSlug,
  projectId,
  moduleId,
  memberRole,
  projectArchived,
  moduleStatus,
}: UseModuleAddWorkItemInput): ModuleAddWorkItemController {
  const queryClient = useQueryClient();
  const canAdd = canAddWorkItemsToModule({ memberRole, archived: projectArchived, moduleStatus });
  const [createOpen, setCreateOpenState] = React.useState(false);
  const [pickerOpen, setPickerOpenState] = React.useState(false);
  const addExisting = useModuleAddExisting({ workspaceSlug, projectId, moduleId });
  const setCreateOpen = React.useCallback(
    (open: boolean) => {
      setCreateOpenState(canAdd && open);
    },
    [canAdd]
  );
  const setPickerOpen = React.useCallback(
    (open: boolean) => {
      setPickerOpenState(canAdd && open);
    },
    [canAdd]
  );
  const openCreate = React.useCallback(() => setCreateOpenState(canAdd), [canAdd]);
  const openPicker = React.useCallback(() => setPickerOpenState(canAdd), [canAdd]);
  const handleCreated = React.useCallback(() => {
    setCreateOpenState(false);
    void queryClient.invalidateQueries({ queryKey: moduleKeys.detail(workspaceSlug, projectId, moduleId) });
  }, [moduleId, projectId, queryClient, workspaceSlug]);
  return {
    ...addExisting,
    canAdd,
    createOpen,
    openCreate,
    setCreateOpen,
    pickerOpen,
    openPicker,
    setPickerOpen,
    handleCreated,
  };
}
