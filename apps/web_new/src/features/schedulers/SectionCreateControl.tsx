// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Creation entry point for the project schedulers section (parity/specs/
// agents.md §3, §5; AGT-010, AGT-021). It lives beside the List and
// Calendar tabs rather than inside one tab body, so a project admin can
// start an install from either view or from an install detail page. A
// caller without project admin standing gets nothing rendered. A finished
// install settles the project's install list and every cached calendar
// window, whichever view is showing.

import { Button } from "@pidash/kit";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import * as React from "react";

import { usePermissions } from "../../core/session/index.js";
import { useProjectRoles, useWorkspaceMembership } from "../auth/index.js";
import { isProjectAdmin } from "../projects/index.js";
import { InstallDialog } from "./InstallDialog.js";
import { invalidateInstalls, useSchedulerBindings } from "./installData.js";
import { occurrenceKeys } from "./occurrences.js";

export interface SectionCreateControlProps {
  workspaceSlug: string;
  projectId: string;
}

/** Install list, catalog counts and all calendar windows for one project. */
export async function settleAfterInstall(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  await Promise.all([
    invalidateInstalls(queryClient, workspaceSlug, projectId),
    queryClient.invalidateQueries({ queryKey: occurrenceKeys.all(workspaceSlug, projectId) }),
  ]);
}

export function SectionCreateControl({
  workspaceSlug,
  projectId,
}: SectionCreateControlProps): React.ReactElement | null {
  const queryClient = useQueryClient();
  const permissions = usePermissions(workspaceSlug);
  const roles = useProjectRoles(workspaceSlug);
  const membership = useWorkspaceMembership(workspaceSlug);
  const bindings = useSchedulerBindings(workspaceSlug, projectId);
  const [open, setOpen] = React.useState(false);

  // Both standing reads must have answered and the project role must be
  // admin; a failed or pending read shows no control at all.
  const canManage = roles.isSuccess && membership.isSuccess && isProjectAdmin(roles.data[projectId] ?? null);
  if (!canManage) return null;

  const installedIds = (bindings.data ?? []).map((binding) => binding.scheduler);

  return (
    <>
      <Button variant="primary" size="small" onClick={() => setOpen(true)}>
        <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
        New Scheduler
      </Button>
      <InstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        open={open}
        onClose={() => setOpen(false)}
        canInstall={canManage}
        canAuthor={permissions.isAdmin}
        installedSchedulerIds={installedIds}
        onInstalled={() => {
          void settleAfterInstall(queryClient, workspaceSlug, projectId);
        }}
      />
    </>
  );
}
