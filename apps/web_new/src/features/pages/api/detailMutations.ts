// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail record writes owned by the shell (parity/specs/pages.md §5.1).
// The title saves over the record endpoint, immediately and outside the
// body path; the cached row and collection adopt the new name so the
// header, switcher and list never disagree with the server.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updatePage, type PageDetail, type PageRow } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { pageKeys } from "./keys.js";

export const PAGE_RENAME_FAILED_TITLE = "Page title could not be saved.";

function renameInList(rows: readonly PageRow[] | undefined, pageId: string, name: string): PageRow[] | undefined {
  return rows?.map((row) => (row.id === pageId ? { ...row, name } : row));
}

/** Persist a page name and patch every cached copy of that page. */
export function usePageRename(workspaceSlug: string, projectId: string, pageId: string) {
  const queryClient = useQueryClient();
  const detailKey = pageKeys.detail(workspaceSlug, projectId, pageId);
  const listKey = pageKeys.list(workspaceSlug, projectId);
  return useMutation<{ name: string }, Error, string>({
    mutationFn: async (name) => {
      await updatePage(getAppClient(), workspaceSlug, projectId, pageId, { name });
      return { name };
    },
    onSuccess: ({ name }) => {
      queryClient.setQueryData<PageDetail>(detailKey, (page) => (page === undefined ? page : { ...page, name }));
      queryClient.setQueryData<PageRow[]>(listKey, (rows) => renameInList(rows, pageId, name));
    },
    onError: () => {
      notifyError(PAGE_RENAME_FAILED_TITLE);
    },
  });
}
