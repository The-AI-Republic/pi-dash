// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module's quick menu (parity/specs/modules.md §4.5–§4.6, MOD-017,
// MOD-018). The live list and the archived screen mount this same
// component on rows and cards; the entries assemble from the menu model
// per role and archive state. Edit and Delete open the shared dialogs,
// the link actions call the shared helpers, Archive reports to the
// archive opener slice G wires (MOD-026), a landed delete reports to
// the host's onDeleted, and Restore runs straight from the menu with
// its own notices. A press outside the dialog
// context still renders the read-only entries; the entries that need
// the dialogs simply have nowhere to open.

import { useQueryClient } from "@tanstack/react-query";
import { IconButton, Menu, type MenuEntry as KitMenuEntry } from "@pidash/kit";
import { restoreModule, type ModuleRow } from "@pidash/api-client";
import { Ellipsis } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../../api/invalidate.js";
import { MODULE_FAILURE_TITLE, moduleFailureBody } from "../../lib/moduleForm.js";
import { copyModuleLink, openModuleInNewTab } from "../../lib/moduleShare.js";
import { useModuleDialogs } from "../dialogs/ModuleDialogProvider.js";
import { moduleMenuEntries, type ModuleMenuEntryId, type ModuleMenuEntryPlan } from "./moduleMenuModel.js";

/** Pinned restore notices the oracle reads (spec §7.6, MOD-017). */
export const MODULE_RESTORE_SUCCESS_TITLE = "Restore success";
export const MODULE_RESTORE_SUCCESS_BODY = "Your module can be found in project modules.";

export interface ModuleRowMenuProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** True for project admins and members; guests keep the link actions. */
  canEdit: boolean;
  /** Archived modules offer restore instead of edit/archive/delete. */
  archived?: boolean | undefined;
  /**
   * Archive entry handler. Slice G (MOD-026) wires its archive
   * confirmation here; until then the gated entry has nowhere to open.
   */
  onArchive?: ((module: ModuleRow) => void) | undefined;
  /**
   * Runs after a row/card delete lands. The list screen wires its peek
   * cleanup here: deleting the peeked module closes the panel (spec §5).
   */
  onDeleted?: ((module: Pick<ModuleRow, "id" | "name">) => void) | undefined;
  /**
   * Keep the trigger visible instead of revealing it on row hover
   * (cards and narrow rows always show their actions).
   */
  alwaysVisible?: boolean | undefined;
}

export function ModuleRowMenu({
  workspaceSlug,
  projectId,
  module,
  canEdit,
  archived = false,
  onArchive,
  onDeleted,
  alwaysVisible = false,
}: ModuleRowMenuProps): React.ReactElement {
  const queryClient = useQueryClient();
  const dialogs = useModuleDialogs();

  const restore = async (): Promise<void> => {
    try {
      await restoreModule(getAppClient(), workspaceSlug, projectId, module.id);
      await invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, module.id);
      notifySuccess(MODULE_RESTORE_SUCCESS_TITLE, MODULE_RESTORE_SUCCESS_BODY);
    } catch (error) {
      notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(error));
    }
  };

  const select = (entry: ModuleMenuEntryPlan): void => {
    const id: ModuleMenuEntryId = entry.id;
    if (id === "edit") dialogs?.openEdit(module);
    else if (id === "open") void openModuleInNewTab(workspaceSlug, projectId, module.id);
    else if (id === "copy") void copyModuleLink(workspaceSlug, projectId, module.id);
    else if (id === "archive") onArchive?.(module);
    else if (id === "restore") void restore();
    else dialogs?.openDelete(module, { onDeleted });
  };

  const items: KitMenuEntry[] = moduleMenuEntries({
    status: module.status,
    canEdit,
    archived,
  }).map((entry) => ({
    label: entry.title,
    onSelect: () => {
      select(entry);
    },
    disabled: entry.disabled,
    ...(entry.hint === null ? {} : { description: entry.hint }),
  }));

  return (
    <div data-main-menu="true" className="flex items-center">
      <Menu
        trigger={
          <IconButton
            label={`Module actions for ${module.name}`}
            size="small"
            data-testid="module-row-menu-trigger"
            className={
              alwaysVisible
                ? "shrink-0"
                : "shrink-0 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
            }
          >
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={items}
      />
    </div>
  );
}
