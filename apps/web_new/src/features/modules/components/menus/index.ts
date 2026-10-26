// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module quick-menu exports (MOD-017/018). The live list, the board,
// the archived screen and the detail header all mount entries from
// this one model, never a second menu.

export { moduleArchiveMenuEntry, type ModuleArchiveEntryInput } from "./moduleArchiveEntry.js";
export { moduleDetailMenuEntries } from "./moduleDetailMenuEntries.js";
export {
  MODULE_ARCHIVE_GATE_HINT,
  MODULE_MENU_ARCHIVE,
  MODULE_MENU_COPY,
  MODULE_MENU_OPEN,
  MODULE_MENU_RESTORE,
  moduleCanArchive,
  moduleMenuEntries,
  type ModuleMenuAssembly,
  type ModuleMenuEntryId,
  type ModuleMenuEntryPlan,
} from "./moduleMenuModel.js";
export {
  MODULE_RESTORE_SUCCESS_BODY,
  MODULE_RESTORE_SUCCESS_TITLE,
  ModuleRowMenu,
  type ModuleRowMenuProps,
} from "./ModuleRowMenu.js";
