// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog assignee breakdown (parity/specs/analytics.md §14,
// ANL-015). The workspace table's first column becomes `Assignee`: each
// row shows the member's avatar when the server supplies one, falls back
// to the kit Avatar's initials, and renders a neutral marker plus
// `Unassigned` for the group with no assignee. Rows keep the server's
// order. Search, the fixed-noun count label and the CSV export come from
// the slice-D table and the shared builder; there is no second table or
// serializer here.
import { Avatar } from "@pidash/kit";
import { UserRound } from "lucide-react";
import * as React from "react";

import { analyticsCsvColumns, buildAnalyticsCsv, downloadCsvFile } from "../../../shared/export/csv.js";
import { useDialogStats } from "../api/projectScope.js";
import type { AnalyticsDialogScope } from "../api/keys.js";
import {
  ASSIGNEE_COUNT_NOUN,
  assigneeCellText,
  assigneeRows,
  assigneeTableColumns,
  type AssigneeRow,
} from "../lib/assignee.js";
import { AnalyticsTable } from "./AnalyticsTable.js";

export interface AssigneeBreakdownSectionProps {
  workspaceSlug: string;
  scope: AnalyticsDialogScope;
}

/** Guidance copy for an empty or refused stats read (shared with the tab table). */
const ASSIGNEE_GUIDANCE =
  "Create projects with work items and assignees to start tracking performance, progress, and team impact here.";

/** Skeleton row count while the scoped read resolves. */
const ASSIGNEE_SKELETON_ROWS = 5;

/** The first cell: avatar or initials for a person, a marker for nobody. */
function AssigneeIdentity({ row }: { row: AssigneeRow }): React.ReactElement {
  if (row.name === null) {
    return (
      <span className="inline-flex items-center gap-(--space-2)">
        <span
          data-testid="assignee-unassigned-mark"
          className="inline-flex size-6 shrink-0 items-center justify-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
        >
          <UserRound aria-hidden="true" className="size-3.5" />
        </span>
        <span data-testid="assignee-name">{assigneeCellText(row)}</span>
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-(--space-2)">
      <Avatar size="small" name={row.name} {...(row.avatarUrl === null ? {} : { src: row.avatarUrl })} />
      <span data-testid="assignee-name">{row.name}</span>
    </span>
  );
}

export function AssigneeBreakdownSection({ workspaceSlug, scope }: AssigneeBreakdownSectionProps): React.ReactElement {
  const stats = useDialogStats(workspaceSlug, scope);
  const columns = React.useMemo(() => assigneeTableColumns(), []);
  const rows = React.useMemo(() => assigneeRows(stats.data ?? []), [stats.data]);
  const rendered = React.useMemo(
    () =>
      columns.map((column) =>
        column.key === "assignee" ? { ...column, render: (row: AssigneeRow) => <AssigneeIdentity row={row} /> } : column
      ),
    [columns]
  );

  return (
    <section aria-labelledby="analytics-dialog-assignee-heading" data-testid="analytics-dialog-assignee">
      <h2 id="analytics-dialog-assignee-heading" className="text-h3 mb-(--space-3) text-(--text)">
        State breakdown
      </h2>
      <AnalyticsTable
        testId="analytics-dialog-assignee-table"
        columns={rendered}
        rows={rows}
        countNoun={ASSIGNEE_COUNT_NOUN}
        loading={stats.isPending}
        skeletonRows={ASSIGNEE_SKELETON_ROWS}
        emptyText={ASSIGNEE_GUIDANCE}
        onExport={(visible) => {
          downloadCsvFile(buildAnalyticsCsv(workspaceSlug, analyticsCsvColumns(rendered), visible));
        }}
      />
    </section>
  );
}
