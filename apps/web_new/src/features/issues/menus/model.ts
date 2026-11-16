// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and layout quick actions (parity/specs/issues.md §7, ISS-060–067).
// One pure model builds the entries every surface shares: the project row
// menu, its cycle and module variants, the archived and global variants,
// the detail and peek headers, and the whole-list ellipsis. The component
// layer turns these descriptors into kit menu entries; keeping the rules
// here lets the tests pin the exact order, gating and wording without
// mounting a popup or a router.

import { ARCHIVABLE_STATE_GROUPS } from "../lifecycle/rules.js";

/** Which list or header a menu is opened from. */
export type QuickActionContext = "project" | "cycle" | "module" | "archived" | "global" | "detail";

/** The entry names, shared with the tests and the parity driver. */
export const QUICK_ACTION_LABELS = {
  edit: "Edit",
  copy: "Make a copy",
  openInNewTab: "Open in new tab",
  copyLink: "Copy link",
  move: "Move to project",
  archive: "Archive",
  restore: "Restore",
  delete: "Delete",
  removeFromCycle: "Remove from cycle",
  removeFromModule: "Remove from module",
} as const;

export type QuickActionKey = keyof typeof QUICK_ACTION_LABELS;

/** Why the archive entry cannot be used, shown under it (ISS-061). */
export const ARCHIVE_GATING_NOTE = "Only completed or canceled work items can be archived";

export interface QuickActionEntry {
  key: QuickActionKey;
  label: string;
  disabled: boolean;
  /** Secondary line under the label; the archive note when gated. */
  description?: string;
}

export interface QuickActionInput {
  context: QuickActionContext;
  /**
   * Project member standing that owns every write entry. Without it the
   * write entries disappear rather than disable, so a guest or a read-only
   * archived project sees only open-in-new-tab and copy-link.
   */
  canWrite: boolean;
  /** The record sits in a completed or cancelled state (ISS-061). */
  archivable: boolean;
  /** The record carries an archive stamp, so it offers Restore, not Archive. */
  archived: boolean;
}

/** Order each context uses. Write entries are filtered by standing. */
const CONTEXT_ORDER: Record<QuickActionContext, readonly QuickActionKey[]> = {
  project: ["edit", "copy", "openInNewTab", "copyLink", "move", "archive", "delete"],
  global: ["edit", "copy", "openInNewTab", "copyLink", "move", "archive", "delete"],
  cycle: ["edit", "copy", "openInNewTab", "copyLink", "removeFromCycle", "archive", "delete"],
  module: ["edit", "copy", "openInNewTab", "copyLink", "removeFromModule", "archive", "delete"],
  // ISS-064: archived rows restore or delete; no edit, copy, move or
  // archive. Delete is intended (the oracle's omission is bug:NEWFRONT-165).
  archived: ["restore", "openInNewTab", "copyLink", "delete"],
  // ISS-066: the detail and peek headers carry the destructive set; the
  // header already has its own copy-link control and title editing.
  detail: ["copy", "openInNewTab", "move", "archive", "restore", "delete"],
};

/** The two entries every standing gets. */
const READ_ONLY_KEYS: ReadonlySet<QuickActionKey> = new Set(["openInNewTab", "copyLink"]);

function offersKey(input: QuickActionInput, key: QuickActionKey): boolean {
  if (READ_ONLY_KEYS.has(key)) return true;
  if (!input.canWrite) return false;
  // A stamped record restores instead of archiving again; a live record
  // archives instead of restoring.
  if (key === "archive") return !input.archived;
  // The archived list only holds stamped rows, so its restore entry does
  // not wait for the per-row flag the header menus read.
  if (key === "restore" && input.context === "archived") return true;
  if (key === "restore") return input.archived;
  return true;
}

/**
 * The entries one surface offers, in order. Archive stays visible on an
 * unfinished record, disabled with the note, so members learn why the
 * action is unavailable instead of hunting for it.
 */
export function quickActionEntries(input: QuickActionInput): QuickActionEntry[] {
  const entries: QuickActionEntry[] = [];
  for (const key of CONTEXT_ORDER[input.context]) {
    if (!offersKey(input, key)) continue;
    const entry: QuickActionEntry = { key, label: QUICK_ACTION_LABELS[key], disabled: false };
    if (key === "archive" && !input.archivable) {
      entry.disabled = true;
      entry.description = ARCHIVE_GATING_NOTE;
    }
    entries.push(entry);
  }
  return entries;
}

/** Whether a state group may be archived (ISS-061). */
export function canArchiveStateGroup(group: string | null | undefined): boolean {
  return group != null && ARCHIVABLE_STATE_GROUPS.includes(group);
}

/** The label a row menu is announced by, e.g. "Actions for WEB-12". */
export function rowMenuLabel(issueKey: string): string {
  return `Actions for ${issueKey}`;
}

/** One cloud-supplied addition to the whole-list ellipsis. */
export interface ListMenuExtra {
  key: string;
  label: string;
}

/**
 * The whole-list ellipsis (ISS-067): opening the page elsewhere first,
 * then its link. Edition extras append after these, never before them.
 */
export function listMenuLabels(extra: readonly ListMenuExtra[] = []): ListMenuExtra[] {
  return [
    { key: "openInNewTab", label: QUICK_ACTION_LABELS.openInNewTab },
    { key: "copyLink", label: QUICK_ACTION_LABELS.copyLink },
    ...extra,
  ];
}
