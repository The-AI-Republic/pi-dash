// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module link writes for the sidebar's links section
// (parity/specs/modules.md §9.3, MOD-035). Each write goes through the
// module-links endpoints and then rewrites the cached detail record's
// `link_module` in place: creates append, updates replace by id, deletes
// filter by id. The detail is deliberately NOT invalidated afterwards:
// the oracle's row legs pin the client append order (titled before the
// later plain link), while a refetch would return the server's
// newest-first order and flip the rows. The success notices are the
// pinned link wording; a refusal toasts the server sentence when it
// carries one and leaves the cache untouched.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createModuleLink,
  deleteModuleLink,
  updateModuleLink,
  type ApiClient,
  type ModuleDetail,
  type ModuleLink,
  type ModuleLinkInput,
} from "@pidash/api-client";

import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { moduleKeys } from "../../api/keys.js";
import {
  MODULE_LINK_CREATE_FAILED_TITLE,
  MODULE_LINK_CREATED,
  MODULE_LINK_DELETE_FAILED_TITLE,
  MODULE_LINK_DELETED,
  MODULE_LINK_UPDATE_FAILED_TITLE,
  MODULE_LINK_UPDATED,
} from "./moduleLinksModel.js";
import { moduleSaveFailureBody } from "./useModuleSidebarWrite.js";

export interface ModuleLinksWriteArgs {
  client: ApiClient;
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
}

export interface ModuleLinkUpdate {
  linkId: string;
  input: ModuleLinkInput;
}

/**
 * Rewrite one detail record's links, leaving an absent record absent
 * (the section renders from its record prop, so the cache may be empty
 * while the section is mounted).
 */
function rewriteModuleLinksCache(
  queryClient: QueryClient,
  key: readonly unknown[],
  rewrite: (current: ModuleLink[]) => ModuleLink[]
): void {
  queryClient.setQueryData<ModuleDetail | undefined>(key, (previous) =>
    previous === undefined ? previous : { ...previous, link_module: rewrite(previous.link_module) }
  );
}

/** Create, update and delete mutations behind the links section. */
export function useModuleLinksWrite({ client, workspaceSlug, projectId, moduleId }: ModuleLinksWriteArgs) {
  const queryClient = useQueryClient();
  const key = moduleKeys.detail(workspaceSlug, projectId, moduleId);

  const createLink = useMutation<ModuleLink, unknown, ModuleLinkInput>({
    mutationFn: (input) => createModuleLink(client, workspaceSlug, projectId, moduleId, input),
    onSuccess: (created) => {
      rewriteModuleLinksCache(queryClient, key, (current) => [...current, created]);
      notifySuccess(MODULE_LINK_CREATED);
    },
    onError: (error) => notifyError(MODULE_LINK_CREATE_FAILED_TITLE, moduleSaveFailureBody(error)),
  });

  const updateLink = useMutation<ModuleLink, unknown, ModuleLinkUpdate>({
    mutationFn: ({ linkId, input }) => updateModuleLink(client, workspaceSlug, projectId, moduleId, linkId, input),
    onSuccess: (updated) => {
      rewriteModuleLinksCache(queryClient, key, (current) =>
        current.map((link) => (link.id === updated.id ? updated : link))
      );
      notifySuccess(MODULE_LINK_UPDATED);
    },
    onError: (error) => notifyError(MODULE_LINK_UPDATE_FAILED_TITLE, moduleSaveFailureBody(error)),
  });

  const deleteLink = useMutation<void, unknown, string>({
    mutationFn: (linkId) => deleteModuleLink(client, workspaceSlug, projectId, moduleId, linkId),
    onSuccess: (_removed, linkId) => {
      rewriteModuleLinksCache(queryClient, key, (current) => current.filter((link) => link.id !== linkId));
      notifySuccess(MODULE_LINK_DELETED);
    },
    onError: (error) => notifyError(MODULE_LINK_DELETE_FAILED_TITLE, moduleSaveFailureBody(error)),
  });

  return { createLink, updateLink, deleteLink };
}
