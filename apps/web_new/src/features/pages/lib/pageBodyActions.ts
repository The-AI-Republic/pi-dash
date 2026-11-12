// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-body option actions (parity/specs/pages.md §6.1, PAGE-044). The
// options-menu shell belongs to the header slice (PAGE-055–060); this
// module owns the copy-markdown entry's behaviour and publishes it
// through a registry the shell reads, so neither side imports the other.
// Editors only: the registry is mounted by the editable surface.

import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { pageBodyMarkdown } from "./pageMarkup.js";

export const PAGE_COPY_MARKDOWN_LABEL = "Copy markdown";
export const PAGE_COPY_MARKDOWN_SUCCESS = "Markdown copied to clipboard.";
export const PAGE_COPY_MARKDOWN_FAILURE = "The page body could not be copied.";

/** What the shell needs to render and run the entry. */
export interface PageBodyOptionActions {
  copyMarkdownLabel: string;
  copyMarkdown: () => Promise<void>;
}

let mounted: PageBodyOptionActions | null = null;
const listeners = new Set<() => void>();

function publish(): void {
  for (const listener of listeners) listener();
}

/** Publish the live editor's actions for the options menu. */
export function registerPageBodyActions(actions: PageBodyOptionActions): () => void {
  mounted = actions;
  publish();
  return () => {
    if (mounted === actions) {
      mounted = null;
      publish();
    }
  };
}

/** The mounted actions, or null on a read-only or unloaded page. */
export function getPageBodyActions(): PageBodyOptionActions | null {
  return mounted;
}

/** Observe the mounted actions; React reads it through useSyncExternalStore. */
export function subscribePageBodyActions(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Test-only reset; production unmounts through the returned disposer. */
export function resetPageBodyActions(): void {
  mounted = null;
  publish();
}

/**
 * Copy the body as Markdown. The clipboard write is awaited before the
 * success notice, so a denied write reports the failure instead of a
 * false success (the bug NEWFRONT-373 pinned on the old app).
 */
export async function copyPageMarkdown(readHtml: () => string): Promise<void> {
  try {
    const html = readHtml();
    const markup = pageBodyMarkdown(html);
    await navigator.clipboard.writeText(markup);
    notifySuccess(PAGE_COPY_MARKDOWN_SUCCESS);
  } catch (error) {
    notifyError(PAGE_COPY_MARKDOWN_FAILURE, error instanceof Error ? error.message : undefined);
  }
}

/** The actions object one editable surface registers. */
export function pageBodyActionsFor(readHtml: () => string): PageBodyOptionActions {
  return {
    copyMarkdownLabel: PAGE_COPY_MARKDOWN_LABEL,
    copyMarkdown: () => copyPageMarkdown(readHtml),
  };
}
