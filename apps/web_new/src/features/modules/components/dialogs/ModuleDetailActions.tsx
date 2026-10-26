// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module actions menu on the detail page (parity/specs/modules.md §7.2,
// §7.4, §7.5, MOD-024/025/026). Edit opens the shared update dialog
// prefilled from the module; Archive opens slice G's confirmation for a
// finished module; Delete opens the confirmation and, once the delete
// lands, returns the user to the modules list — the page they were on no
// longer exists. The menu is one overflow trigger marked for the shell
// (`data-main-menu`); later slices extend it through `extraEntries` rather
// than forking a second menu, and `archiveModule` splices the Archive entry
// in beside those read-only entries so the opener runs inside this
// component's own dialog provider. Edit, Archive and Delete are editor
// entries — a guest offers only the read-only entries another slice
// supplies, and with none of those the header shows no menu at all
// (§4.5). The component brings its own dialog provider, so a detail host
// only has to mount the menu.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import type { ModuleStatus } from "@pidash/api-client";
import { moduleDeleteLanding } from "../../lib/moduleForm.js";
import { moduleArchiveMenuEntry } from "../menus/moduleArchiveEntry.js";
import { moduleProjectFactsQueryOptions, moduleProjectIdentifier } from "../../api/projectFacts.js";
import { moduleDetailQueryOptions } from "../sidebar/moduleSidebarQueries.js";
import { ModuleDialogProvider, useModuleDialogs } from "./ModuleDialogProvider.js";

export const MODULE_MENU_EDIT = "Edit";
export const MODULE_MENU_DELETE = "Delete";

export interface ModuleDetailActionsProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  /** True for project admins and members; guests get no Edit or Delete. */
  canEdit: boolean;
  /** Entries other slices own (copy link, open in new tab…). */
  extraEntries?: readonly MenuEntry[] | undefined;
  /**
   * The module this page shows. Present, editors get the Archive entry on
   * a finished module (slice G, MOD-026); absent, the menu offers none.
   */
  archiveModule?: { id: string; name: string; status: ModuleStatus } | undefined;
  /** Menu trigger's accessible label. */
  label?: string | undefined;
}

export function ModuleDetailActions(props: ModuleDetailActionsProps): React.ReactElement {
  const project = useQuery(moduleProjectFactsQueryOptions(getAppClient(), props.workspaceSlug, props.projectId));
  // A guest with nothing read-only to offer gets no trigger instead of an
  // empty menu; the read stays unconditional so the hook order is fixed.
  if (!props.canEdit && (props.extraEntries ?? []).length === 0) return <></>;
  return (
    <ModuleDialogProvider
      workspaceSlug={props.workspaceSlug}
      projectId={props.projectId}
      currentProject={{
        id: props.projectId,
        name: project.data?.name ?? "",
        identifier: moduleProjectIdentifier(project.data),
      }}
    >
      <ModuleActionsMenu {...props} />
    </ModuleDialogProvider>
  );
}

function ModuleActionsMenu({
  workspaceSlug,
  projectId,
  moduleId,
  canEdit,
  extraEntries = [],
  archiveModule,
  label = "Module actions",
}: ModuleDetailActionsProps): React.ReactElement {
  const navigate = useNavigate();
  const dialogs = useModuleDialogs();
  const detail = useQuery(moduleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, moduleId));
  const record = detail.data;

  const items: MenuEntry[] = [...extraEntries];
  if (record !== undefined && canEdit) {
    items.unshift({
      label: MODULE_MENU_EDIT,
      onSelect: () => {
        dialogs?.openEdit(record);
      },
    });
    if (archiveModule !== undefined) {
      items.push(
        ...moduleArchiveMenuEntry({
          status: archiveModule.status,
          canEdit,
          module: { id: archiveModule.id, name: archiveModule.name },
          onArchive: (target) => {
            dialogs?.openArchive(target);
          },
        })
      );
    }
    items.push({
      label: MODULE_MENU_DELETE,
      onSelect: () => {
        dialogs?.openDelete(record, {
          onDeleted: () => {
            void navigate(moduleDeleteLanding(workspaceSlug, projectId));
          },
        });
      },
    });
  }

  return (
    <div data-main-menu="true" className="flex items-center">
      <Menu
        trigger={
          <IconButton label={label} size="small" data-testid="module-actions-trigger" disabled={record === undefined}>
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={items}
      />
    </div>
  );
}
