// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module progress section (parity/specs/modules.md §9.2, MOD-034). The
// second pane of the analytics sidebar, below slice C's fields: a
// collapsible section with a completion badge, a burndown chart with
// its measure picker, and the States/Assignees/Labels breakdown tabs.
// Each tab shows its own breakdown (the NEWFRONT-337 fix). Clicking an
// entry toggles that value as a rich filter on the module's work-item
// list; the peek and archived mounts render the same data read-only.

import { Combobox } from "@pidash/kit";
import type { ApiClient } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { useResolvedScheme } from "../../../../core/theme/index.js";
import { useProjectRoles } from "../../../auth/index.js";
import { useRichFilters, useUpdateRichFilters } from "../../../issues/index.js";
import { isProjectAuthorized } from "../../../projects/index.js";
import { AreaChart, chartSlotColor, resolveChartScheme } from "../../../../shared/charts/index.js";
import { isArchivedRecord } from "./ModuleAnalyticsSidebar.js";
import { moduleSidebarEditable, showsModulePoints } from "./moduleSidebarModel.js";
import {
  moduleSidebarDetailQueryOptions,
  moduleSidebarEstimatesQueryOptions,
  type ModuleSidebarRecord,
} from "./moduleSidebarQueries.js";
import {
  MODULE_BREAKDOWN_FILTER_STEMS,
  MODULE_BREAKDOWN_TABS,
  MODULE_NO_ASSIGNEE_TITLE,
  MODULE_NO_LABELS_TITLE,
  MODULE_PROGRESS_MEASURES,
  moduleAssigneeEntries,
  moduleBreakdownEntryActive,
  moduleCompletionChartPresent,
  moduleCompletionPoints,
  moduleLabelEntries,
  moduleProgressDateState,
  moduleProgressHeaderPercent,
  moduleProgressMeasureLabel,
  moduleProgressNotice,
  readModuleProgressTab,
  selectModuleAssigneeRows,
  selectModuleLabelRows,
  selectModuleStateGroupRows,
  toggleModuleBreakdownCondition,
  writeModuleProgressTab,
  type ModuleBreakdownRow,
  type ModuleBreakdownTab,
  type ModuleDistributionLike,
  type ModuleProgressMeasure,
  type ModuleStateGroupRow,
} from "./moduleProgressModel.js";

/** Burndown plot height inside the narrow sidebar. */
const BURNDOWN_HEIGHT = 180;

/** Series key behind the remaining-work area. */
const REMAINING_SERIES_KEY = "remaining";

/** Accessible name of the measure dropdown list. */
const MEASURE_LIST_LABEL = "Progress measure";

export interface ModuleProgressSectionProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  /** The peek's read-only mount; the detail page leaves it false. */
  readOnly?: boolean;
  /** The archived peek: entries stay inert even for an editor. */
  archived?: boolean;
  /**
   * Explicit editor standing. Omit to read the project role map; tests
   * and hosts that already resolved access may pass it directly.
   */
  canEdit?: boolean;
  /** Test seam; the app client by default. */
  client?: ApiClient;
}

export function ModuleProgressSection({
  workspaceSlug,
  projectId,
  moduleId,
  readOnly = false,
  archived = false,
  canEdit,
  client = getAppClient(),
}: ModuleProgressSectionProps): React.ReactElement | null {
  const roles = useProjectRoles(workspaceSlug);
  const role = roles.data?.[projectId] ?? null;
  const editor = canEdit ?? isProjectAuthorized(role);

  const detail = useQuery(moduleSidebarDetailQueryOptions(client, workspaceSlug, projectId, moduleId, archived));
  const estimates = useQuery(moduleSidebarEstimatesQueryOptions(client, workspaceSlug, projectId));

  if (detail.data === undefined) return null;
  return (
    <ModuleProgressSectionContent
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      moduleId={moduleId}
      record={detail.data}
      readOnly={readOnly}
      archived={archived}
      editor={editor}
      showPoints={showsModulePoints(estimates.data)}
      onRefetchDetails={() => {
        void detail.refetch();
      }}
    />
  );
}

export interface ModuleProgressSectionContentProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  record: ModuleSidebarRecord;
  readOnly: boolean;
  archived: boolean;
  editor: boolean;
  showPoints: boolean;
  /** Refetch the module details (the measure switch fires it). */
  onRefetchDetails: () => void;
}

interface EntryProps {
  title: string;
  testKey: string;
  total: number;
  percent: number;
  color?: string | undefined;
  /** Inert rows (no filter id, or a read-only mount) render a div. */
  interactive: boolean;
  active: boolean;
  /** Defined exactly for interactive rows; inert rows render a div. */
  onToggle: (() => void) | undefined;
}

function ProgressEntry({ title, testKey, total, percent, color, interactive, active, onToggle }: EntryProps) {
  const body = (
    <>
      <span className="flex min-w-0 flex-1 items-center gap-(--space-2) truncate">
        {color === undefined ? null : (
          <span aria-hidden="true" className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
        )}
        <span className="min-w-0 flex-1 truncate">{title}</span>
      </span>
      <span className="text-meta shrink-0 text-(--text-muted)">{percent}%</span>
      <span className="text-meta shrink-0 text-(--text-muted)">of {total}</span>
    </>
  );
  const shape =
    "flex w-full items-center justify-between gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1)";
  if (!interactive) {
    return (
      <div data-testid={`module-progress-entry-${testKey}`} className={`${shape} text-(--text-muted)`}>
        {body}
      </div>
    );
  }
  return (
    <button
      type="button"
      data-testid={`module-progress-entry-${testKey}`}
      aria-pressed={active}
      onClick={onToggle}
      className={`${shape} text-left text-(--text) hover:bg-(--subtle) ${active ? "bg-(--subtle)" : ""}`}
    >
      {body}
    </button>
  );
}

export function ModuleProgressSectionContent({
  workspaceSlug,
  projectId,
  moduleId,
  record,
  readOnly,
  archived,
  editor,
  showPoints,
  onRefetchDetails,
}: ModuleProgressSectionContentProps): React.ReactElement {
  const { scheme, customDark } = useResolvedScheme();
  const dateState = moduleProgressDateState(record.start_date, record.target_date);
  const [open, setOpen] = React.useState(() => dateState === "active");
  const [measure, setMeasure] = React.useState<ModuleProgressMeasure>("issues");
  const [tab, setTab] = React.useState<ModuleBreakdownTab>(() => readModuleProgressTab(moduleId));
  const entity = React.useMemo(() => ({ scope: "module", projectId, moduleId }) as const, [projectId, moduleId]);
  const editable = moduleSidebarEditable({
    readOnly,
    archived: archived || isArchivedRecord(record),
    canEdit: editor,
  });
  const filters = useRichFilters(workspaceSlug, entity, editable);
  const { updateRichFilters } = useUpdateRichFilters(workspaceSlug, entity, editable);
  const filtersReady = !filters.isPending;

  const notice = moduleProgressNotice(dateState);
  if (notice !== null) {
    return (
      <section
        aria-label="Progress"
        data-testid="module-progress-section"
        className="flex flex-col gap-(--space-2) border-t border-(--border) pt-(--space-4)"
      >
        <div className="flex w-full items-center justify-between gap-(--space-2)">
          <span className="text-h3 font-medium text-(--text)">Progress</span>
          <span data-testid="module-progress-notice" className="text-meta text-(--text-muted) italic">
            {notice}
          </span>
        </div>
      </section>
    );
  }

  const distribution: ModuleDistributionLike =
    measure === "points" ? record.estimate_distribution : record.distribution;
  const percent =
    measure === "points"
      ? moduleProgressHeaderPercent(record.completed_estimate_points, record.total_estimate_points)
      : moduleProgressHeaderPercent(record.completed_issues, record.total_issues);
  const stem = MODULE_BREAKDOWN_FILTER_STEMS[tab];

  function handleSelectMeasure(id: string): void {
    setMeasure(id === "points" ? "points" : "issues");
    onRefetchDetails();
  }

  function handleTabChange(next: ModuleBreakdownTab): void {
    setTab(next);
    writeModuleProgressTab(moduleId, next);
  }

  function toggleEntry(id: string): void {
    updateRichFilters(toggleModuleBreakdownCondition(filters.conditions, stem, id));
  }

  function entryActive(id: string): boolean {
    return editable && moduleBreakdownEntryActive(filters.conditions, stem, id);
  }

  function stateEntries(): React.ReactElement[] {
    return selectModuleStateGroupRows(record, measure).map((row: ModuleStateGroupRow) => {
      const interactive = editable && filtersReady;
      return (
        <ProgressEntry
          key={row.key}
          title={row.title}
          testKey={row.key}
          total={row.total}
          percent={row.percent}
          interactive={interactive}
          active={entryActive(row.key)}
          onToggle={interactive ? () => toggleEntry(row.key) : undefined}
        />
      );
    });
  }

  function breakdownEntries(rows: ModuleBreakdownRow[], unkeyed: string): React.ReactElement[] {
    return rows.map((row, index) => {
      const key = row.id ?? `${unkeyed}-${index}`;
      const interactive = editable && filtersReady && row.id !== null;
      const id = row.id;
      return (
        <ProgressEntry
          key={key}
          title={row.title}
          testKey={key}
          total={row.total}
          percent={row.percent}
          color={row.color}
          interactive={interactive}
          active={id !== null && entryActive(id)}
          onToggle={interactive && id !== null ? () => toggleEntry(id) : undefined}
        />
      );
    });
  }

  const entries =
    tab === "States"
      ? stateEntries()
      : tab === "Labels"
        ? breakdownEntries(selectModuleLabelRows(moduleLabelEntries(distribution), measure), "unlabeled")
        : breakdownEntries(selectModuleAssigneeRows(moduleAssigneeEntries(distribution), measure), "unassigned");

  return (
    <section
      aria-label="Progress"
      data-testid="module-progress-section"
      className="flex flex-col gap-(--space-3) border-t border-(--border) pt-(--space-4)"
    >
      <div className="flex w-full items-center justify-between gap-(--space-2)">
        <button
          type="button"
          data-testid="module-progress-toggle"
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
          className="inline-flex min-w-0 flex-1 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-1) py-(--space-1) text-left"
        >
          <span className="text-h3 font-medium text-(--text)">Progress</span>
          {percent > 0 ? (
            <span
              data-testid="module-progress-percent"
              className="text-meta rounded-full bg-(--subtle) px-(--space-2) py-px font-medium text-(--text)"
            >
              {percent}%
            </span>
          ) : null}
          <ChevronDown
            aria-hidden="true"
            className={`h-4 w-4 shrink-0 text-(--text-muted) ${open ? "rotate-180" : ""}`}
          />
        </button>
        {showPoints ? (
          <Combobox
            trigger={
              <button
                type="button"
                data-testid="module-progress-measure"
                className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle)"
              >
                {moduleProgressMeasureLabel(measure)}
              </button>
            }
            options={MODULE_PROGRESS_MEASURES.map((entry) => ({ id: entry.value, label: entry.label }))}
            value={measure}
            onSelect={handleSelectMeasure}
            listLabel={MEASURE_LIST_LABEL}
            emptyText="No measures"
            searchEnabled={false}
          />
        ) : null}
      </div>

      {open ? (
        <>
          {moduleCompletionChartPresent(distribution) ? (
            <div data-testid="module-progress-chart">
              <AreaChart
                points={moduleCompletionPoints(distribution)}
                series={[
                  {
                    key: REMAINING_SERIES_KEY,
                    name: "Remaining",
                    color: chartSlotColor(0, resolveChartScheme(scheme, customDark)),
                  },
                ]}
                legend={false}
                height={BURNDOWN_HEIGHT}
              />
            </div>
          ) : null}
          <div
            role="tablist"
            aria-label="Progress breakdown"
            data-testid="module-progress-tabs"
            className="flex gap-(--space-1)"
          >
            {MODULE_BREAKDOWN_TABS.map((name) => (
              <button
                key={name}
                type="button"
                role="tab"
                aria-selected={tab === name}
                data-testid={`module-progress-tab-${name}`}
                onClick={() => handleTabChange(name)}
                className={
                  tab === name
                    ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-3) py-(--space-1) font-medium text-(--text)"
                    : "text-body rounded-(--radius-control) px-(--space-3) py-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
                }
              >
                {name}
              </button>
            ))}
          </div>
          <div role="tabpanel" data-testid="module-progress-entries" className="flex flex-col gap-(--space-1)">
            {entries.length === 0 ? (
              <p data-testid="module-progress-entries-empty" className="text-body text-(--text-muted)">
                {tab === "Labels" ? MODULE_NO_LABELS_TITLE : MODULE_NO_ASSIGNEE_TITLE}
              </p>
            ) : (
              entries
            )}
          </div>
        </>
      ) : null}
    </section>
  );
}
