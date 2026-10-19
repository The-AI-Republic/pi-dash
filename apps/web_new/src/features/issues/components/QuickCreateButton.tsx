// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar quick-create entry (parity/specs/shell.md §4, SHELL-047).
// Members and admins with a joined project open the shared creation
// dialog; guests and project-less members get the entry disabled.
// The dialog rides a lazy chunk so the sidebar stays out of the
// initial-JS chain (size budget).

import { Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

import type { WorkItemCreateProject } from "./WorkItemCreateDialog.js";

const LazyWorkItemCreateDialog = React.lazy(() =>
  import("./WorkItemCreateDialog.js").then((module) => ({ default: module.WorkItemCreateDialog }))
);

export interface QuickCreateButtonProps {
  workspaceSlug: string;
  /** Joined projects the dialog can file under; empty disables. */
  projects: WorkItemCreateProject[];
  /** Workspace admin/member standing; guests read false. */
  canCreate: boolean;
  /** The caller's workspace role, carried onto the dialog's project rows. */
  memberRole: number | null;
}

export function QuickCreateButton({
  workspaceSlug,
  projects,
  canCreate,
  memberRole,
}: QuickCreateButtonProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <Button
        variant="primary"
        className="w-full"
        disabled={!canCreate || projects.length === 0}
        onClick={() => setOpen(true)}
        data-testid="sidebar-quick-create"
      >
        <Plus size={16} strokeWidth={2} aria-hidden="true" />
        New work item
      </Button>
      {open ? (
        <React.Suspense fallback={null}>
          <LazyWorkItemCreateDialog
            open={open}
            onOpenChange={setOpen}
            workspaceSlug={workspaceSlug}
            projects={projects.map((project) => ({ ...project, memberRole }))}
            canCreate={canCreate}
          />
        </React.Suspense>
      ) : null}
    </>
  );
}
