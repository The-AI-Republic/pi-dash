// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module quick-menu assembly (parity/specs/modules.md §4.5, MOD-017).
// Pure: the entry set in spec order for one role and archive state.
// Editors on an active module get edit, the link actions, archive and
// delete; editors on an archived module get the link actions and
// restore; guests keep the link actions in either state. Archive is
// disabled with an explanation until the module's status is finished.
// The row/card menu and the detail header menu assemble from this one
// model, so the entries never drift between surfaces.

import { ARCHIVABLE_MODULE_STATUSES, type ModuleStatus } from "@pidash/api-client";

import { MODULE_MENU_DELETE, MODULE_MENU_EDIT } from "../dialogs/ModuleDetailActions.js";

export const MODULE_MENU_OPEN = "Open in new tab";
export const MODULE_MENU_COPY = "Copy link";
export const MODULE_MENU_ARCHIVE = "Archive";
export const MODULE_MENU_RESTORE = "Restore";

/** Entry identity in display order. */
export type ModuleMenuEntryId = "edit" | "open" | "copy" | "archive" | "restore" | "delete";

/** One assembled entry: its title, gating and gate explanation. */
export interface ModuleMenuEntryPlan {
  id: ModuleMenuEntryId;
  title: string;
  disabled: boolean;
  /** Shown under the title while the entry is disabled; null otherwise. */
  hint: string | null;
}

/** Why an unfinished module cannot be archived yet (§4.5). */
export const MODULE_ARCHIVE_GATE_HINT = "Only completed or cancelled modules can be archived.";

/** Whether the status lets the module archive (§4.5, §7.4). */
export function moduleCanArchive(status: ModuleStatus): boolean {
  return ARCHIVABLE_MODULE_STATUSES.includes(status);
}

export interface ModuleMenuAssembly {
  status: ModuleStatus;
  /** True for project admins and members; guests keep the link actions. */
  canEdit: boolean;
  /** Archived modules offer restore instead of edit/archive/delete. */
  archived: boolean;
}

/** The menu's entries in display order for one role and archive state. */
export function moduleMenuEntries({ status, canEdit, archived }: ModuleMenuAssembly): ModuleMenuEntryPlan[] {
  const open: ModuleMenuEntryPlan = { id: "open", title: MODULE_MENU_OPEN, disabled: false, hint: null };
  const copy: ModuleMenuEntryPlan = { id: "copy", title: MODULE_MENU_COPY, disabled: false, hint: null };
  if (!canEdit) return [open, copy];
  if (archived) {
    return [open, copy, { id: "restore", title: MODULE_MENU_RESTORE, disabled: false, hint: null }];
  }
  const archivable = moduleCanArchive(status);
  return [
    { id: "edit", title: MODULE_MENU_EDIT, disabled: false, hint: null },
    open,
    copy,
    {
      id: "archive",
      title: MODULE_MENU_ARCHIVE,
      disabled: !archivable,
      hint: archivable ? null : MODULE_ARCHIVE_GATE_HINT,
    },
    { id: "delete", title: MODULE_MENU_DELETE, disabled: false, hint: null },
  ];
}
