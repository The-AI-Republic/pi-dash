// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module detail page's add entries in one place (parity/specs/modules.md
// §8.3, MOD-031): the header button, the shared creation dialog seeded with
// this module, and the searchable picker of project work items the module
// does not hold yet. The page mounts this once; the header and the empty
// card both reach it through the controller, so there is a single creation
// form and a single picker per visit. Creation is the Issues area's dialog
// (NEWFRONT-630) — this surface only decides the module it lands in.

import * as React from "react";

import type { WorkItemCreateProject } from "../../issues/index.js";
import { MODULE_PICKER_SEARCH_HINT } from "../lib/moduleAddWorkItem.js";
import type { ModuleAddWorkItemController } from "../lib/useModuleAddWorkItem.js";
import { ModuleAddWorkItemButton } from "./ModuleAddWorkItemButton.js";

// Both dialogs ride their own chunks, as they do on the issues screens:
// the create form carries the description editor, and the picker carries
// its search modal (Quality gates, initial-JS budget).
const LazyCreateDialog = React.lazy(() =>
  import("../../issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);
const LazyAddExistingModal = React.lazy(() =>
  import("../../issues/list/AddExistingModal.js").then((module) => ({ default: module.AddExistingModal }))
);

export interface ModuleAddWorkItemProps {
  workspaceSlug: string;
  project: WorkItemCreateProject & { identifier: string };
  moduleId: string;
  controller: ModuleAddWorkItemController;
}

export function ModuleAddWorkItem({
  workspaceSlug,
  project,
  moduleId,
  controller,
}: ModuleAddWorkItemProps): React.ReactElement | null {
  if (!controller.canAdd) return null;
  return (
    <>
      <ModuleAddWorkItemButton onOpen={controller.openCreate} />
      {controller.createOpen ? (
        <React.Suspense fallback={null}>
          <LazyCreateDialog
            open
            onOpenChange={(open) => {
              if (!open) controller.setCreateOpen(false);
            }}
            workspaceSlug={workspaceSlug}
            projects={[project]}
            initialProjectId={project.id}
            lockedProjectId={project.id}
            context={{ moduleId }}
            onCreated={() => {
              controller.handleCreated();
            }}
          />
        </React.Suspense>
      ) : null}
      {controller.pickerOpen ? (
        <React.Suspense fallback={null}>
          <LazyAddExistingModal
            open
            onOpenChange={(open) => {
              controller.setPickerOpen(open);
            }}
            workspaceSlug={workspaceSlug}
            projectId={project.id}
            excludeModuleId={moduleId}
            searchPlaceholder={MODULE_PICKER_SEARCH_HINT}
            onAttach={async (ids) => {
              await controller.addExisting(ids);
            }}
          />
        </React.Suspense>
      ) : null}
    </>
  );
}
