// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail record writes owned by this slice (parity/specs/pages.md
// §5.1–5.2, §8.1). The title and the mark both save over the record
// endpoint — immediately, outside the body path — and both patch every
// cached copy of the page so the header, switcher and list never disagree
// with the server. The mark write sends only `logo_props`; `color` is an
// unrelated column and clearing is an empty object, never null (the old
// build's null PATCH answered 400, NEWFRONT-355). Title saves go through
// a newest-wins queue: one request per page on the wire, the newest name
// behind it, so a burst of keystrokes cannot leave an older PATCH
// arriving last (NEWFRONT-358).

import { updatePage, type PageDetail, type PageMark, type PageRow } from "@pidash/api-client";
import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";

import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { createPageRenameChannels, type PageRenameChannels } from "../lib/pageRenameQueue.js";
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

/** One rename write: the name and the page record it belongs to. */
interface RenameWrite {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  name: string;
}

/** The settled answer to one asked-for rename. */
interface PageRenameResult {
  write: RenameWrite;
  name: string;
  /** A newer name replaced this one: it must not touch caches or notices. */
  superseded: boolean;
}

/**
 * Persist a page name and patch every cached copy of that page. Names are
 * serialised per page: a name asked for while another save is in flight
 * replaces the waiting one and is sent as soon as the wire is free, so
 * the last keystroke is the last write (NEWFRONT-358). A superseded save
 * answers quietly — no cache write and no notice, since its name is stale
 * and a newer save is on its way.
 *
 * The channels live on the query client, addressed by page, because the
 * detail route keeps its screen across `$pageId` changes and remounts it
 * when the operator comes back: a name waiting on the page being left
 * must not be dropped by a rename on the page being opened, and a page
 * opened again must not start a second channel beside the first. Every
 * answered write patches the page it names, never the page the screen
 * shows now. The page identity travels with the mutation's variables,
 * not with the render, because the mutation runs after the screen may
 * already have moved on.
 */
export function usePageRename(workspaceSlug: string, projectId: string, pageId: string) {
  const queryClient = useQueryClient();
  const channels = renameChannelsFor(queryClient);
  const mutation = useMutation<PageRenameResult, unknown, RenameWrite>({
    mutationFn: (write) =>
      new Promise<PageRenameResult>((resolve, reject) => {
        channels.enqueue(write, (outcome) => {
          if (outcome.superseded) resolve({ write, name: write.name, superseded: true });
          else if (outcome.failed) reject(outcome.error);
          else resolve({ write, name: write.name, superseded: false });
        });
      }),
    // Patch from the write's own identifiers: a rename that settles after
    // the screen opened another page still belongs to the page it renamed.
    onSuccess: ({ write, name, superseded }) => {
      if (superseded) return;
      queryClient.setQueryData<PageDetail>(
        pageKeys.detail(write.workspaceSlug, write.projectId, write.pageId),
        (page) => (page === undefined ? page : { ...page, name })
      );
      queryClient.setQueryData<PageRow[]>(pageKeys.list(write.workspaceSlug, write.projectId), (rows) =>
        renameInList(rows, write.pageId, name)
      );
    },
    onError: () => {
      notifyError(PAGE_RENAME_FAILED_TITLE);
    },
  });
  const { mutate: runRename } = mutation;
  // The commit the title calls: the page named here travels with the
  // write, so a later render of this screen cannot retarget it.
  const commit = React.useCallback(
    (name: string) => {
      runRename({ workspaceSlug, projectId, pageId, name });
    },
    [runRename, workspaceSlug, projectId, pageId]
  );
  return { ...mutation, mutate: commit };
}

/** One rename channel per page, shared by every screen on one cache. */
const channelsByClient = new WeakMap<QueryClient, PageRenameChannels<RenameWrite>>();

function renameChannelsFor(queryClient: QueryClient): PageRenameChannels<RenameWrite> {
  let channels = channelsByClient.get(queryClient);
  if (channels === undefined) {
    channels = createPageRenameChannels<RenameWrite>(
      (write) => `${write.workspaceSlug}\u0000${write.projectId}\u0000${write.pageId}`,
      async (write) => {
        await updatePage(getAppClient(), write.workspaceSlug, write.projectId, write.pageId, { name: write.name });
      }
    );
    channelsByClient.set(queryClient, channels);
  }
  return channels;
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
