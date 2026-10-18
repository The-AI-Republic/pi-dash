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
import { usePickerCycles, usePickerModules, usePickerStates } from "../../../shared/pickers/reference.js";
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
  const editable = context.editable && !context.isRowReadOnly(issue) && isEditableColumn(column.key);
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
      {editable ? (
        <EditableCell issue={issue} column={column} context={context} onDone={focus} />
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

function EditableCell({
  issue,
  column,
  context,
  onDone,
}: {
  issue: IssueListItem;
  column: SheetColumn;
  context: SheetCellContext;
  onDone: () => void;
}): React.ReactElement {
  const disabled = !context.editable || context.isRowReadOnly(issue);
  switch (column.key) {
    case "state":
      return (
        <StatePicker
          workspaceSlug={context.workspaceSlug}
          projectId={context.projectId}
          value={issue.state_id}
          disabled={disabled}
          variant="ghost"
          onChange={(id) => {
            void context.onUpdate(issue.id, { state_id: id }).finally(onDone);
          }}
        />
      );
    case "priority":
      return (
        <PriorityPicker
          value={issue.priority}
          disabled={disabled}
          variant="ghost"
          onChange={(level) => {
            void context.onUpdate(issue.id, { priority: level }).finally(onDone);
          }}
        />
      );
    case "assignee":
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
            void context.onUpdate(issue.id, { assignee_ids: ids }).finally(onDone);
          }}
        />
      );
    case "start_date":
      return (
        <DatePicker
          value={issue.start_date ?? null}
          onChange={(iso) => {
            void context
              .onUpdate(issue.id, { start_date: iso, target_date: issue.target_date ?? null })
              .finally(onDone);
          }}
          max={issue.target_date ?? null}
          weekStart={context.weekStart}
          label="Start date"
          placeholder="Start date"
          disabled={disabled}
          testId={`sheet-start-${issue.id}`}
        />
      );
    case "due_date":
      return <DueDateCell issue={issue} context={context} onDone={onDone} disabled={disabled} />;
    case "estimate":
      return (
        <EstimatePicker
          workspaceSlug={context.workspaceSlug}
          projectId={context.projectId}
          value={estimateValue(issue.estimate_point)}
          disabled={disabled}
          variant="ghost"
          onChange={(id) => {
            void context.onUpdate(issue.id, { estimate_point: id }).finally(onDone);
          }}
        />
      );
    case "labels":
      return <LabelCell issue={issue} context={context} onDone={onDone} disabled={disabled} />;
    case "cycle":
      return <CycleCell issue={issue} context={context} onDone={onDone} disabled={disabled} />;
    case "modules":
      return <ModuleCell issue={issue} context={context} onDone={onDone} disabled={disabled} />;
    default:
      return <ReadOnlyCell issue={issue} column={column} context={context} />;
  }
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
  const overdue = issue.target_date !== null && issue.target_date !== undefined && issue.target_date < today;
  return (
    <span className={overdue ? "text-(--danger)" : undefined}>
      <DatePicker
        value={issue.target_date ?? null}
        onChange={(iso) => {
          void context.onUpdate(issue.id, { start_date: issue.start_date ?? null, target_date: iso }).finally(onDone);
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
        void context.onUpdate(issue.id, { label_ids: ids }).finally(onDone);
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
        void context.onCycle(issue, cycleId).finally(onDone);
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
          void context.onModules(issue, ids).finally(onDone);
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
          disabled={(issue.sub_issues_count ?? 0) === 0}
          onClick={() => context.onOpenSubIssues(issue)}
          className="cursor-pointer text-(--text) hover:underline disabled:cursor-default disabled:no-underline"
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

export function formatDayOrBlank(iso: string | null): string {
  return iso === null ? "" : formatDay(iso);
}
