// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog seam (parity/specs/exports.md §0.3, EXP-016): the Pages
// detail owns the options-menu entry and the live editor facts; the exports
// area owns the dialog. The dialog registers here and the detail renders the
// registered component only while the export is open. Mirrors the
// workspace-settings exports-guide seam.
import * as React from "react";

/** The live page facts the dialog renders from, supplied by the Pages detail. */
export interface PageExportHost {
  pageTitle: string;
  projectId: string;
  workspaceSlug: string;
  /** Current editor body as HTML (pictures resolve from here for documents). */
  getHtml(): string;
  /** Current editor body as Markdown (the plain-text source). */
  getMarkdown(): string;
}

export type PageExportDialogComponent = (props: {
  host: PageExportHost;
  open: boolean;
  onClose(): void;
}) => React.ReactNode;

let dialog: PageExportDialogComponent | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

/**
 * Register the exports area's dialog. Returns an unmount cleanup that
 * removes only this registration, so a stale owner cannot clear a newer
 * dialog.
 */
export function registerPageExportDialog(component: PageExportDialogComponent): () => void {
  dialog = component;
  emit();
  return () => {
    if (dialog !== component) return;
    dialog = null;
    emit();
  };
}

export function getPageExportDialog(): PageExportDialogComponent | null {
  return dialog;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The registered dialog, re-rendering when the exports area mounts it. */
export function usePageExportDialog(): PageExportDialogComponent | null {
  return React.useSyncExternalStore(subscribe, getPageExportDialog, getPageExportDialog);
}

/** Clears the registration. Tests only; production unregisters on unmount. */
export function resetPageExportDialog(): void {
  dialog = null;
  emit();
}
