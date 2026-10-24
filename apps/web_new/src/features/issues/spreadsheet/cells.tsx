// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spreadsheet property cells (parity/specs/issues.md §3, ISS-017/020).
// Each editable cell renders the shared picker as its trigger; a pick
// writes immediately through the sheet's mutation and returns focus to
// the cell. Read-only cells render plain text or a navigating count.
// The cell wrapper is focusable and carries the column key so arrow-key
// navigation can find it by coordinates.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { formatDay, todayIsoDay } from "../dropdowns/calendar.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import { useLabels } from "../labels/api.js";
import {
  CyclePicker,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PriorityPicker,
  StatePicker,
} from "../../../shared/pickers/index.js";
import { isCompletedCycle } from "../../../shared/pickers/cycles.js";
import { stateOptionGate } from "../edition/workflow.js";
import {
  usePickerCycles,
  usePickerEstimates,
  usePickerModules,
  usePickerStates,
  usePickerWorkspaceMembers,
} from "../../../shared/pickers/reference.js";
import { selectActiveEstimate } from "@pidash/api-client";
import { selectEstimateOptions } from "../../../shared/pickers/estimates.js";
import {
  countLabel,
  labelNamesTooltip,
  ReadOnlyDate,
  ReadOnlyMembers,
  ReadOnlyProperty,
} from "../readonly/ReadOnlyProperty.js";
import { Avatar } from "@pidash/kit";
import { sheetCellId, type SheetColumn } from "./model.js";

export interface SheetCellContext {
  workspaceSlug: string;
  projectId: string;
  editable: boolean;
  weekStart: number;
  /** Writes one issue field; resolves after the optimistic paint. */
  onUpdate: (issueId: string, patch: Record<string, unknown>) => Promise<void>;
  onCycle: (issue: IssueListItem, cycleId: string | null) => Promise<void>;
  onModules: (issue: IssueListItem, moduleIds: string[]) => Promise<void>;
  onOpenSubIssues: (issue: IssueListItem) => void;
  /** Row-level lock: completed cycle or archived issue (ISS-017). */
  isRowReadOnly: (issue: IssueListItem) => boolean;
  /** Refocus a cell after a picker closes. */
  focusCell: (rowId: string, columnKey: string, columnIndex: number) => void;
}

const cellClasses =
  "h-11 min-w-36 border-r border-b border-(--border) px-(--space-2) text-caption align-middle text-(--text) outline-none focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-inset";

/** One property cell, editable or read-only, in the sheet grid. */
export function SheetPropertyCell({
  issue,
  column,
  context,
  rowIndex,
  columnIndex,
  onKeyDown,
}: {
  issue: IssueListItem;
  column: SheetColumn;
  context: SheetCellContext;
  rowIndex: number;
  columnIndex: number;
  onKeyDown: (event: React.KeyboardEvent<HTMLTableCellElement>) => void;
}): React.ReactElement {
  const editableKey = asEditableKey(column.key);
  const editable = context.editable && !context.isRowReadOnly(issue) && editableKey !== null;
  const focus = (): void => context.focusCell(issue.id, column.key, columnIndex);
  return (
    <td
      id={sheetCellId(issue.id, column.key)}
      tabIndex={0}
      data-row-index={rowIndex}
      data-column-index={columnIndex}
      onKeyDown={onKeyDown}
      className={cellClasses}
      aria-label={`${column.label} for ${issue.name}`}
    >
      {editable && editableKey !== null ? (
        <EditableCell columnKey={editableKey} issue={issue} context={context} onDone={focus} disabled={false} />
      ) : // A row whose own state froze shows every property as a value
      // with no affordance (ISS-220); a sheet that is merely not
      // editable keeps the plain read-only cell it always had.
      context.isRowReadOnly(issue) ? (
        <FrozenPropertyValue issue={issue} column={column} context={context} />
      ) : (
        <ReadOnlyCell issue={issue} column={column} context={context} />
      )}
    </td>
  );
}

const EDITABLE_KEYS = new Set([
  "state",
  "priority",
  "assignee",
  "start_date",
  "due_date",
  "estimate",
  "labels",
  "cycle",
  "modules",
]);

/** Whether a column supports inline editing (ISS-017). */
export function isEditableColumn(key: string): boolean {
  return EDITABLE_KEYS.has(key);
}

type EditableKey =
  | "state"
  | "priority"
  | "assignee"
  | "start_date"
  | "due_date"
  | "estimate"
  | "labels"
  | "cycle"
  | "modules";

function asEditableKey(key: string): EditableKey | null {
  return EDITABLE_KEYS.has(key) ? (key as EditableKey) : null;
}

type EditableCellProps = {
  issue: IssueListItem;
  context: SheetCellContext;
  onDone: () => void;
  disabled: boolean;
};

function StateCell({ issue, context, onDone, disabled }: EditableCellProps): React.ReactElement {
  return (
    <StatePicker
      workspaceSlug={context.workspaceSlug}
      projectId={context.projectId}
      value={issue.state_id}
      disabled={disabled}
      variant="ghost"
      optionGate={stateOptionGate(context.projectId, issue.state_id)}
      onChange={(id) => {
        writeThenRefocus(() => context.onUpdate(issue.id, { state_id: id }), onDone);
      }}
    />
  );
}

function PriorityCell({ issue, context, onDone, disabled }: EditableCellProps): React.ReactElement {
  return (
    <PriorityPicker
      value={issue.priority}
      disabled={disabled}
      variant="ghost"
      onChange={(level) => {
        writeThenRefocus(() => context.onUpdate(issue.id, { priority: level }), onDone);
      }}
    />
  );
}

function AssigneeCell({ issue, context, onDone, disabled }: EditableCellProps): React.ReactElement {
  return (
    <MemberPicker
      workspaceSlug={context.workspaceSlug}
      projectId={context.projectId}
      multiple
      value={issue.assignee_ids}
      disabled={disabled}
      variant="ghost"
      placeholder="Assignees"
      onChange={(ids) => {
        writeThenRefocus(() => context.onUpdate(issue.id, { assignee_ids: ids }), onDone);
      }}
    />
  );
}

function StartDateCell({ issue, context, onDone, disabled }: EditableCellProps): React.ReactElement {
  return (
    <DatePicker
      value={issue.start_date ?? null}
      onChange={(iso) => {
        writeThenRefocus(
          () => context.onUpdate(issue.id, { start_date: iso, target_date: issue.target_date ?? null }),
          onDone
        );
      }}
      max={issue.target_date ?? null}
      weekStart={context.weekStart}
      label="Start date"
      placeholder="Start date"
      disabled={disabled}
      testId={`sheet-start-${issue.id}`}
    />
  );
}

function EstimateCell({ issue, context, onDone, disabled }: EditableCellProps): React.ReactElement {
  return (
    <EstimatePicker
      workspaceSlug={context.workspaceSlug}
      projectId={context.projectId}
      value={estimateValue(issue.estimate_point)}
      disabled={disabled}
      variant="ghost"
      onChange={(id) => {
        writeThenRefocus(() => context.onUpdate(issue.id, { estimate_point: id }), onDone);
      }}
    />
  );
}

function EditableCell({ columnKey, ...props }: EditableCellProps & { columnKey: EditableKey }): React.ReactElement {
  const Renderer = EDITABLE_CELLS[columnKey];
  return <Renderer {...props} />;
}

/** Renderers for the editable property keys, keyed by the display property. */
const EDITABLE_CELLS: Record<EditableKey, (props: EditableCellProps) => React.ReactElement> = {
  state: StateCell,
  priority: PriorityCell,
  assignee: AssigneeCell,
  start_date: StartDateCell,
  due_date: DueDateCell,
  estimate: EstimateCell,
  labels: LabelCell,
  cycle: CycleCell,
  modules: ModuleCell,
};

/**
 * Run one cell write, then return focus whether it landed or was
 * refused. The data layer reports the refusal (toast and rollback), so
 * the cell only has to settle the promise without leaking a rejection.
 */
function writeThenRefocus(write: () => Promise<void>, onDone: () => void): void {
  void (async () => {
    try {
      await write();
    } catch {
      // Reported by runSheetWrite; the cell still returns focus.
    } finally {
      onDone();
    }
  })();
}

function DueDateCell({
  issue,
  context,
  onDone,
  disabled,
}: {
  issue: IssueListItem;
  context: SheetCellContext;
  onDone: () => void;
  disabled: boolean;
}): React.ReactElement {
  const today = todayIsoDay();
  const states = usePickerStates(context.workspaceSlug, context.projectId, true);
  const stateGroup = states.data?.find((state) => state.id === issue.state_id)?.group;
  const overdue = isOverdue(issue.target_date ?? null, stateGroup, today);
  return (
    <span className={overdue ? "text-(--danger)" : undefined}>
      <DatePicker
        value={issue.target_date ?? null}
        onChange={(iso) => {
          writeThenRefocus(
            () => context.onUpdate(issue.id, { start_date: issue.start_date ?? null, target_date: iso }),
            onDone
          );
        }}
        min={issue.start_date ?? null}
        weekStart={context.weekStart}
        label="Due date"
        placeholder="Due date"
        disabled={disabled}
        testId={`sheet-due-${issue.id}`}
      />
    </span>
  );
}

/**
 * Whether a due date reads as overdue. Completed and cancelled work is
 * never flagged, and neither is a row whose state group is unknown yet.
 */
export function isOverdue(date: string | null, stateGroup: string | undefined, today: string): boolean {
  if (date === null || stateGroup === undefined) return false;
  if (stateGroup === "completed" || stateGroup === "cancelled") return false;
  return date < today;
}

function estimateValue(point: IssueListItem["estimate_point"]): string | null {
  if (point === null || point === undefined) return null;
  return String(point);
}

function LabelCell({
  issue,
  context,
  onDone,
  disabled,
}: {
  issue: IssueListItem;
  context: SheetCellContext;
  onDone: () => void;
  disabled: boolean;
}): React.ReactElement {
  const labels = useLabels(context.workspaceSlug, context.projectId);
  return (
    <LabelPicker
      workspaceSlug={context.workspaceSlug}
      projectId={context.projectId}
      value={issue.label_ids}
      disabled={disabled}
      onChange={(ids) => {
        writeThenRefocus(() => context.onUpdate(issue.id, { label_ids: ids }), onDone);
      }}
      label={`Labels for ${issue.name}${selectedLabelCount(labels.data ?? [], issue.label_ids)}`}
    />
  );
}

function selectedLabelCount(rows: readonly { id: string; name: string }[], ids: readonly string[]): string {
  const names = rows.filter((row) => ids.includes(row.id)).map((row) => row.name);
  return names.length === 0 ? "" : `: ${names.join(", ")}`;
}

function CycleCell({
  issue,
  context,
  onDone,
  disabled,
}: {
  issue: IssueListItem;
  context: SheetCellContext;
  onDone: () => void;
  disabled: boolean;
}): React.ReactElement {
  const cycles = usePickerCycles(context.workspaceSlug, context.projectId, true);
  const completed =
    issue.cycle_id !== null && isCompletedCycle(cycles.data?.find((cycle) => cycle.id === issue.cycle_id)?.status);
  return (
    <CyclePicker
      workspaceSlug={context.workspaceSlug}
      projectId={context.projectId}
      value={issue.cycle_id}
      disabled={disabled || completed}
      variant="ghost"
      onChange={(cycleId) => {
        writeThenRefocus(() => context.onCycle(issue, cycleId), onDone);
      }}
    />
  );
}

function ModuleCell({
  issue,
  context,
  onDone,
  disabled,
}: {
  issue: IssueListItem;
  context: SheetCellContext;
  onDone: () => void;
  disabled: boolean;
}): React.ReactElement {
  const modules = usePickerModules(context.workspaceSlug, context.projectId, true);
  return (
    <span className="inline-flex min-w-0 items-center gap-(--space-1)">
      <span className="min-w-0 flex-1 truncate" aria-hidden="true">
        {(modules.data ?? [])
          .filter((row) => issue.module_ids.includes(row.id))
          .map((row) => row.name)
          .join(", ")}
      </span>
      <ModulePicker
        workspaceSlug={context.workspaceSlug}
        projectId={context.projectId}
        multiple
        value={issue.module_ids}
        disabled={disabled}
        variant="ghost"
        iconOnly
        placeholder="Modules"
        onChange={(ids) => {
          writeThenRefocus(() => context.onModules(issue, ids), onDone);
        }}
      />
    </span>
  );
}

function ReadOnlyCell({
  issue,
  column,
  context,
}: {
  issue: IssueListItem;
  column: SheetColumn;
  context: SheetCellContext;
}): React.ReactElement {
  switch (column.key) {
    case "created_on":
      return <span>{formatDay(issue.created_at.slice(0, 10))}</span>;
    case "updated_on":
      return <span>{formatDay(issue.updated_at.slice(0, 10))}</span>;
    case "link":
      return <span>{issue.link_count ?? 0}</span>;
    case "attachment_count":
      return <span>{issue.attachment_count ?? 0}</span>;
    case "sub_issue_count":
      return (
        <button
          type="button"
          aria-disabled={(issue.sub_issues_count ?? 0) === 0}
          onClick={() => {
            if ((issue.sub_issues_count ?? 0) > 0) context.onOpenSubIssues(issue);
          }}
          className="cursor-pointer text-(--text) hover:underline aria-disabled:cursor-default aria-disabled:no-underline"
        >
          {issue.sub_issues_count ?? 0} sub-work item{(issue.sub_issues_count ?? 0) === 1 ? "" : "s"}
        </button>
      );
    case "assignee":
      return (
        <span className="flex items-center gap-(--space-1)">
          {issue.assignee_ids.slice(0, 3).map((id) => (
            <Avatar key={id} name={id.slice(0, 2)} size="small" />
          ))}
        </span>
      );
    case "state":
      return <ReadOnlyStateName issue={issue} context={context} />;
    case "priority":
      return <span>{issue.priority === "none" ? "" : issue.priority}</span>;
    case "labels":
      return <span>{issue.label_ids.length}</span>;
    case "modules":
      return <span>{issue.module_ids.length}</span>;
    case "cycle":
      return <span>{issue.cycle_id === null ? "" : issue.cycle_id.slice(0, 8)}</span>;
    case "start_date":
      return <span>{formatDayOrBlank(issue.start_date ?? null)}</span>;
    case "due_date":
      return <span>{formatDayOrBlank(issue.target_date ?? null)}</span>;
    case "estimate":
      return <span>{issue.estimate_point ?? ""}</span>;
    default:
      return <span />;
  }
}

/**
 * A frozen row's property value, with no trigger at all (ISS-220): the
 * row is archived, synced or inside a completed cycle, so every column
 * reads as icon plus value and an explicit "None" when unset.
 */
function FrozenPropertyValue({
  issue,
  column,
  context,
}: {
  issue: IssueListItem;
  column: SheetColumn;
  context: SheetCellContext;
}): React.ReactElement {
  switch (column.key) {
    case "state":
      return <FrozenState issue={issue} context={context} />;
    case "priority":
      return <ReadOnlyProperty kind="priority" value={PRIORITY_LABELS[issue.priority]} />;
    case "assignee":
      return <FrozenAssignees issue={issue} context={context} />;
    case "labels":
      return <FrozenLabels issue={issue} context={context} />;
    case "modules":
      return <FrozenModules issue={issue} context={context} />;
    case "cycle":
      return <FrozenCycle issue={issue} context={context} />;
    case "start_date":
      return <ReadOnlyDate iso={issue.start_date ?? null} />;
    case "due_date":
      return <ReadOnlyDate iso={issue.target_date ?? null} />;
    case "estimate":
      return <FrozenEstimate issue={issue} context={context} />;
    case "created_on":
      return <ReadOnlyProperty kind="date" value={formatDay(issue.created_at.slice(0, 10))} />;
    case "updated_on":
      return <ReadOnlyProperty kind="date" value={formatDay(issue.updated_at.slice(0, 10))} />;
    case "link":
      return <ReadOnlyProperty kind="parent" value={String(issue.link_count ?? 0)} />;
    case "attachment_count":
      return <ReadOnlyProperty kind="parent" value={String(issue.attachment_count ?? 0)} />;
    case "sub_issue_count":
      return <ReadOnlyProperty kind="parent" value={countLabel(issue.sub_issues_count ?? 0, "sub-work item")} />;
    default:
      return <ReadOnlyProperty kind="parent" value={null} />;
  }
}

function ReadOnlyStateName({
  issue,
  context,
}: {
  issue: IssueListItem;
  context: SheetCellContext;
}): React.ReactElement {
  const states = usePickerStates(context.workspaceSlug, context.projectId, true);
  const name = states.data?.find((state) => state.id === issue.state_id)?.name ?? "";
  return <span>{name}</span>;
}

/**
 * The names behind a list of reference ids, in the order the ids were
 * stored. A missing row (a reference the caller cannot see, or a read
 * that has not answered yet) is skipped, so a frozen row shows the
 * counts it has rather than placeholders.
 */
function referenceNames<T extends { id: string }>(
  rows: readonly T[] | undefined,
  ids: readonly string[],
  nameOf: (row: T) => string
): string[] {
  if (rows === undefined) return [];
  const names: string[] = [];
  for (const id of ids) {
    const row = rows.find((candidate) => candidate.id === id);
    if (row !== undefined) names.push(nameOf(row));
  }
  return names;
}

function stateName(states: readonly { id: string; name: string }[] | undefined, stateId: string | null): string | null {
  if (stateId === null) return null;
  const names = referenceNames(states, [stateId], (state) => state.name);
  return names[0] ?? null;
}

function FrozenState({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const states = usePickerStates(context.workspaceSlug, context.projectId, true);
  return <ReadOnlyProperty kind="state" value={stateName(states.data, issue.state_id)} />;
}

const PRIORITY_LABELS: Record<IssueListItem["priority"], string | null> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: null,
};

/**
 * Assignee ids with the directory's names. The directory already loads
 * with the sheet, so a missing entry renders the id's short form; while
 * the directory is still arriving the same short form stands in.
 */
function FrozenAssignees({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const members = usePickerWorkspaceMembers(context.workspaceSlug, true);
  const names = React.useMemo(
    () =>
      issue.assignee_ids.map((id) => {
        const member = members.data?.find((row) => row.member.id === id);
        return { id, name: member?.member.display_name ?? id.slice(0, 2) };
      }),
    [issue.assignee_ids, members.data]
  );
  return <ReadOnlyMembers members={names} />;
}

function FrozenLabels({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const labels = useLabels(context.workspaceSlug, context.projectId, true);
  const names = React.useMemo(
    () => referenceNames(labels.data, issue.label_ids, (label) => label.name),
    [issue.label_ids, labels.data]
  );
  return (
    <ReadOnlyProperty
      kind="labels"
      value={issue.label_ids.length === 0 ? null : countLabel(issue.label_ids.length, "label")}
      tooltip={labelNamesTooltip(names)}
    />
  );
}

function FrozenCycle({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const cycles = usePickerCycles(context.workspaceSlug, context.projectId, true);
  const name = issue.cycle_id === null ? null : (cycles.data?.find((row) => row.id === issue.cycle_id)?.name ?? null);
  return <ReadOnlyProperty kind="cycle" value={name} />;
}

function FrozenModules({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const modules = usePickerModules(context.workspaceSlug, context.projectId, true);
  const names = referenceNames(modules.data, issue.module_ids, (row) => row.name);
  return (
    <ReadOnlyProperty
      kind="module"
      value={issue.module_ids.length === 0 ? null : countLabel(issue.module_ids.length, "module")}
      tooltip={labelNamesTooltip(names)}
    />
  );
}

function FrozenEstimate({ issue, context }: { issue: IssueListItem; context: SheetCellContext }): React.ReactElement {
  const estimates = usePickerEstimates(context.workspaceSlug, context.projectId, true);
  const system = estimates.data === undefined ? undefined : selectActiveEstimate(estimates.data);
  const value =
    issue.estimate_point === null || issue.estimate_point === undefined ? null : String(issue.estimate_point);
  const option = system === undefined ? undefined : selectEstimateOptions(system).find((row) => row.id === value);
  return <ReadOnlyProperty kind="estimate" value={option?.label ?? value} />;
}

export function formatDayOrBlank(iso: string | null): string {
  return iso === null ? "" : formatDay(iso);
}
