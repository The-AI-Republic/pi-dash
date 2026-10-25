// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail record writes owned by this slice (parity/specs/pages.md
// §5.1–5.2, §8.1). The title and the mark both save over the record
// endpoint — immediately, outside the body path — and both patch every
// cached copy of the page so the header, switcher and list never disagree
// with the server. The mark write sends only `logo_props`; `color` is an
// unrelated column and clearing is an empty object, never null (the old
// build's null PATCH answered 400, NEWFRONT-355).

import { updatePage, type PageDetail, type PageMark, type PageRow } from "@pidash/api-client";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { pageKeys } from "./keys.js";

export const PAGE_RENAME_FAILED_TITLE = "Page title could not be saved.";
export const PAGE_MARK_FAILED_TITLE = "Page mark could not be saved.";

/** The mark record a cleared page carries: no kind, no glyph. */
export const PAGE_MARK_CLEARED: Record<string, unknown> = {};

function renameInList(rows: readonly PageRow[] | undefined, pageId: string, name: string): PageRow[] | undefined {
  return rows?.map((row) => (row.id === pageId ? { ...row, name } : row));
}

function markInList(
  rows: readonly PageRow[] | undefined,
  pageId: string,
  logoProps: Record<string, unknown>
): PageRow[] | undefined {
  return rows?.map((row) => (row.id === pageId ? { ...row, logo_props: logoProps as PageMark } : row));
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

/**
 * Persist a page mark and patch every cached copy of that page. An empty
 * record is the clear gesture and is sent as an empty `logo_props`, which
 * the API accepts and which leaves the row with its fallback glyph.
 */
export function usePageMark(workspaceSlug: string, projectId: string, pageId: string) {
  const queryClient = useQueryClient();
  const detailKey = pageKeys.detail(workspaceSlug, projectId, pageId);
  const listKey = pageKeys.list(workspaceSlug, projectId);
  return useMutation<{ logoProps: Record<string, unknown> }, Error, Record<string, unknown>>({
    mutationFn: async (draft) => {
      const logoProps = { ...draft };
      await updatePage(getAppClient(), workspaceSlug, projectId, pageId, { logo_props: logoProps as PageMark });
      return { logoProps };
    },
    onSuccess: ({ logoProps }) => {
      queryClient.setQueryData<PageDetail>(detailKey, (page) =>
        page === undefined ? page : { ...page, logo_props: logoProps as PageMark }
      );
      queryClient.setQueryData<PageRow[]>(listKey, (rows) => markInList(rows, pageId, logoProps));
    },
    onError: () => {
      notifyError(PAGE_MARK_FAILED_TITLE);
    },
  });
}
