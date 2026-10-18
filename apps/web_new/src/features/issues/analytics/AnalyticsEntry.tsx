// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues-header analytics entry (parity/specs/issues.md §9, ISS-107).
// The button shows only to people who can create work items in *this*
// project — project admins and members, plus workspace admins — and opens
// a work-items dialog scoped to the current project, with the epic flag
// carried through. The analytics area owns the destination; until it
// lands this mount names the project and the server's total, plus the
// per-group counts once the full list is loaded.

import { Button, Dialog } from "@pidash/kit";
import { ChartColumn } from "lucide-react";
import * as React from "react";

import { usePermissions } from "../../../core/session/index.js";
import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { useProjectRoles } from "../../auth/index.js";
import { usePickerStates } from "../../../shared/pickers/index.js";

export interface AnalyticsEntryProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  /** True when the list shows epics; the dialog header reflects it. */
  isEpics?: boolean;
  /** Rows already loaded, for the group breakdown. */
  rows?: readonly AnalyticsRow[] | undefined;
  /** Server total; the dialog always reports this number. */
  totalCount?: number | undefined;
}

/** One list row, reduced to the state it belongs to. */
export interface AnalyticsRow {
  state_id: string | null;
}

/** Counts rows by their workflow group, in first-seen order. */
export function groupCounts(
  rows: readonly AnalyticsRow[],
  states: readonly { id: string; group: string }[] | undefined
): { label: string; count: number }[] {
  const groupOf = new Map((states ?? []).map((state) => [state.id, state.group]));
  const counts = new Map<string, number>();
  for (const row of rows) {
    const label = row.state_id === null ? "No state" : (groupOf.get(row.state_id) ?? "Other");
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return [...counts.entries()].map(([label, count]) => ({ label, count }));
}

export function AnalyticsEntry({
  workspaceSlug,
  projectId,
  projectName,
  isEpics = false,
  rows = [],
  totalCount = 0,
}: AnalyticsEntryProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const projectRoles = useProjectRoles(workspaceSlug);
  const [open, setOpen] = React.useState(false);
  // Groups resolve only while the dialog is open; the header entry costs
  // no extra read on a plain list visit.
  const states = usePickerStates(workspaceSlug, projectId, open);
  // Fail closed while either role read resolves, so nobody sees the entry
  // flash in and out. Workspace admins pass everywhere; otherwise the row
  // needs admin or member standing on this project, which is exactly the
  // set that may create work items here.
  if (permissions.isLoading || projectRoles.isPending) return null;
  const workspaceAdmin = permissions.role !== null && permissions.role >= ROLE_ADMIN;
  const projectRole = projectRoles.data?.[projectId];
  const allowed = workspaceAdmin || (projectRole !== undefined && projectRole >= ROLE_MEMBER);
  if (!allowed) return null;
  // A partial page would undercount, so the breakdown waits for the full
  // list; the server total is authoritative either way.
  const groups = rows.length === totalCount ? groupCounts(rows, states.data) : [];
  const total = totalCount > 0 ? totalCount : rows.length;

  return (
    <>
      <Button variant="ghost" size="small" onClick={() => setOpen(true)} data-testid="issues-analytics-entry">
        <ChartColumn size={14} strokeWidth={2} aria-hidden="true" />
        <span>Analytics</span>
      </Button>
      <Dialog
        open={open}
        onOpenChange={setOpen}
        title={isEpics ? "Epic analytics" : "Work items analytics"}
        description={`Work items in ${projectName}`}
      >
        <div className="flex flex-col gap-(--space-3)" data-testid="issues-analytics-body">
          <p className="text-body text-(--text)">
            Total Work items <span data-testid="analytics-total">{total}</span>
          </p>
          {groups.length > 0 ? (
            <dl className="flex flex-col gap-(--space-1)">
              {groups.map((row) => (
                <div key={row.label} className="flex items-center justify-between gap-(--space-4)">
                  <dt className="text-body text-(--text-muted)">{row.label}</dt>
                  <dd className="text-body text-(--text)">{row.count}</dd>
                </div>
              ))}
            </dl>
          ) : (
            <p className="text-body text-(--text-muted)">No grouped counts to show yet.</p>
          )}
        </div>
      </Dialog>
    </>
  );
}
