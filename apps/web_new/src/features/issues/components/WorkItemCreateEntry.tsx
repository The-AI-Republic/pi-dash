// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-level creation entry (parity/specs/issues.md §11, ISS-117
// and the shell seam SHELL-086/047). The issues list screen registers
// the "New work item" command today, but the old app's `ni` sequence
// lives at the workspace level and works on every page. This entry
// mounts beside the shell chrome, registers the command for a member
// with at least one project they may file into, and opens the shared
// dialog on the page's current project when there is one.

import * as React from "react";

import { registerCommands } from "../../../shared/commands/index.js";
import { selectCreatableWorkItemProjects } from "../create/createForm.js";
import type { WorkItemCreateProject } from "./WorkItemCreateDialog.js";

const LazyWorkItemCreateDialog = React.lazy(() =>
  import("./WorkItemCreateDialog.js").then((module) => ({ default: module.WorkItemCreateDialog }))
);

/** Stable id shared with the issues list's own entry. */
export const CREATE_WORK_ITEM_COMMAND_ID = "issues.create-work-item";

export interface WorkItemCreateEntryProps {
  workspaceSlug: string;
  /** Joined projects, each with its own project membership role. */
  projects: WorkItemCreateProject[];
  /** Workspace admin/member standing; guests read false. */
  canCreate: boolean;
  /** The project whose page is open, or null on workspace pages. */
  initialProjectId: string | null;
}

/**
 * Registers `ni` and renders the lazy create dialog it opens. Renders
 * nothing until the command fires, so the shell keeps its initial
 * bundle; the dialog module loads on first use.
 */
export function WorkItemCreateEntry({
  workspaceSlug,
  projects,
  canCreate,
  initialProjectId,
}: WorkItemCreateEntryProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const creatable = React.useMemo(() => selectCreatableWorkItemProjects(projects), [projects]);
  const allowed = canCreate && creatable.length > 0;
  const hint = React.useMemo(
    () => creatable.find((project) => project.id === initialProjectId)?.name ?? undefined,
    [creatable, initialProjectId]
  );

  React.useEffect(() => {
    if (!allowed) return undefined;
    return registerCommands([
      {
        id: CREATE_WORK_ITEM_COMMAND_ID,
        title: "New work item",
        ...(hint === undefined ? {} : { hint }),
        section: "Create",
        sequence: "ni",
        keywords: ["create", "add", "new", "issue"],
        run: () => {
          setOpen(true);
        },
      },
    ]);
  }, [allowed, hint]);

  return open ? (
    <React.Suspense fallback={null}>
      <LazyWorkItemCreateDialog
        open={open}
        onOpenChange={setOpen}
        workspaceSlug={workspaceSlug}
        projects={projects}
        canCreate={canCreate}
        {...(initialProjectId === null ? {} : { initialProjectId })}
      />
    </React.Suspense>
  ) : null;
}
