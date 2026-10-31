// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail header's Archive entry (parity/specs/modules.md §4.5, §7.4;
// MOD-026). The detail page mounts one actions menu; slice G appends this
// entry beside slice H's open/copy entries rather than forking a second
// menu. It is offered to editors on active modules only, and it is
// disabled with the reason until the module's status is finished — the
// same gate the row menu reads from the one shared model.

import type { MenuEntry } from "@pidash/kit";
import type { ModuleRow, ModuleStatus } from "@pidash/api-client";

import { MODULE_MENU_ARCHIVE, moduleCanArchive, MODULE_ARCHIVE_GATE_HINT } from "./moduleMenuModel.js";

export interface ModuleArchiveEntryInput {
  status: ModuleStatus;
  /** True for project admins and members; guests get no editor entries. */
  canEdit: boolean;
  /** Opens the confirmation for this module. */
  onArchive: (module: Pick<ModuleRow, "id" | "name">) => void;
  module: Pick<ModuleRow, "id" | "name">;
}

/** The Archive entry, or none for a guest or an archived module. */
export function moduleArchiveMenuEntry({ status, canEdit, onArchive, module }: ModuleArchiveEntryInput): MenuEntry[] {
  if (!canEdit) return [];
  const archivable = moduleCanArchive(status);
  return [
    {
      label: MODULE_MENU_ARCHIVE,
      onSelect: () => {
        onArchive(module);
      },
      disabled: !archivable,
      ...(archivable ? {} : { description: MODULE_ARCHIVE_GATE_HINT }),
    },
  ];
}
