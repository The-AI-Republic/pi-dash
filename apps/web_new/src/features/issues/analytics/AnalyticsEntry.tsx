// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues-header analytics entry (parity/specs/issues.md §9, ISS-107).
// The button shows only to people who can create work items in *this*
// project — project admins and members, plus workspace admins — and opens
// the scoped analytics panel (parity/specs/analytics.md §13). Analytics
// owns the panel content; this file owns the trigger, the role gate and
// the scope it hands over. The cycle detail (CYC-033, NEWFRONT-712) mounts
// it with a cycle scope and a narrow-only trigger for every role; the
// module detail (MOD-032, NEWFRONT-802) mounts it with a module scope. The
// desktop trigger stays member+.

import { Button } from "@pidash/kit";
import { ChartColumn } from "lucide-react";
import * as React from "react";

import { usePermissions } from "../../../core/session/index.js";
import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { useProjectRoles } from "../../auth/index.js";
import { ScopedAnalyticsDialog } from "../../analytics/index.js";

export interface AnalyticsEntryProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  /**
   * Accepted for the surfaces that still pass their list totals; the
   * scoped panel reads its own project totals (ANL-014), so the entry no
   * longer needs them. Kept so callers need no coordinated change.
   */
  isEpics?: boolean | undefined;
  rows?: readonly { state_id: string | null }[] | undefined;
  totalCount?: number | undefined;
  /**
   * Cycle scope (NEWFRONT-712): the dialog reads
   * `Analytics for {project} in {cycle}`.
   */
  cycle?: { id: string; name: string } | undefined;
  /**
   * Module scope (NEWFRONT-802): the dialog reads
   * `Analytics for {project} in {module}`.
   */
  module?: { id: string; name: string } | undefined;
  /**
   * True also renders a narrow-only trigger for every role (the cycle
   * detail bar, CYC-033). The desktop trigger stays member+.
   */
  narrowGuestAccess?: boolean | undefined;
}

export function AnalyticsEntry({
  workspaceSlug,
  projectId,
  projectName,
  cycle,
  module,
  narrowGuestAccess = false,
}: AnalyticsEntryProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const projectRoles = useProjectRoles(workspaceSlug);
  const [open, setOpen] = React.useState(false);
  // Fail closed while either role read resolves, so nobody sees the entry
  // flash in and out. Workspace admins pass everywhere; otherwise the row
  // needs admin or member standing on this project, which is exactly the
  // set that may create work items here.
  if (permissions.isLoading || projectRoles.isPending) return null;
  const workspaceAdmin = permissions.role !== null && permissions.role >= ROLE_ADMIN;
  const projectRole = projectRoles.data?.[projectId];
  const allowed = workspaceAdmin || (projectRole !== undefined && projectRole >= ROLE_MEMBER);
  if (!allowed && !narrowGuestAccess) return null;
  let entryTestId = "issues-analytics-entry";
  if (cycle !== undefined) {
    entryTestId = "cycle-analytics-entry";
  } else if (module !== undefined) {
    entryTestId = "module-analytics-entry";
  }

  return (
    <>
      {allowed ? (
        <Button variant="ghost" size="small" onClick={() => setOpen(true)} data-testid={entryTestId}>
          <ChartColumn size={14} strokeWidth={2} aria-hidden="true" />
          <span>Analytics</span>
        </Button>
      ) : null}
      {narrowGuestAccess ? (
        <span className="md:hidden">
          <Button variant="ghost" size="small" onClick={() => setOpen(true)} data-testid="cycle-analytics-entry-narrow">
            <ChartColumn size={14} strokeWidth={2} aria-hidden="true" />
            <span>Analytics</span>
          </Button>
        </span>
      ) : null}
      <ScopedAnalyticsDialog
        workspaceSlug={workspaceSlug}
        open={open}
        onOpenChange={setOpen}
        projectId={projectId}
        projectName={projectName}
        cycle={cycle}
        module={module}
      />
    </>
  );
}
