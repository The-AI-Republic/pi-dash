// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar rows B (parity/specs/issues.md §13, ISS-153–159): start
// and due dates, estimate, modules, cycle, parent and labels. Dates bound
// each other and flag an overdue due date by state group; estimate,
// modules and cycle only render when the project runs that feature.
// Modules diff into the bridge's add/remove pair and lock while the write
// is in flight; cycle picks no-op on the current cycle and confirm with a
// notice. The parent row opens a search dialog and can clear inline;
// labels combine chips with the shared picker. Every row reads through
// the shared issue PATCH, so guests and archived items stay disabled.

import { Button, cn, type ButtonVariant } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { Link2, Pencil, X } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { CyclePicker, EstimatePicker, ModulePicker } from "../../../../shared/pickers/index.js";
import { DatePicker, useWeekStart } from "../../dropdowns/index.js";
import { LabelChips } from "../../labels/LabelChips.js";
import { LabelPicker } from "../../labels/LabelPicker.js";
import { useLabels } from "../../labels/api.js";
import { useIssueCycleWrite, useIssueModulesWrite } from "../../api/membership.js";
import { parentIssueQueryOptions, useParentWrite } from "../../api/parent.js";
import { useUpdateIssue } from "../../api/mutations.js";
import { ParentPickerDialog } from "./ParentPickerDialog.js";
import type { SidebarProject } from "./IssuePropertiesSidebar.js";
import { SidebarRow } from "./SidebarRow.js";
import { dateBounds, isDueDateOverdue, showsCycleRow, showsEstimateRow, showsModuleRow } from "./sidebarState.js";

const ROW_VARIANT: ButtonVariant = "ghost";
const ROW_TEXT = "text-body min-w-0 truncate text-left";

/** Notice title when a sidebar property write is refused (rows A use the same wording). */
export const PROPERTY_SAVE_FAILED_B_TITLE = "Couldn't update the work item";

/** The parent's short identifier, e.g. WEB-12. */
export function parentIdentifier(parentSequence: number, projectIdentifier: string): string {
  return `${projectIdentifier}-${parentSequence}`;
}

function useIssueWrite(workspaceSlug: string, projectId: string, issueId: string) {
  return useUpdateIssue({
    workspaceSlug,
    projectId,
    issueId,
    failureTitle: PROPERTY_SAVE_FAILED_B_TITLE,
  });
}

export interface StartDateRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
}

/** Start date: bounded by the due date, cleared through the calendar's clear control. */
export function StartDateRow({ workspaceSlug, projectId, issue, disabled }: StartDateRowProps): React.ReactElement {
  const weekStart = useWeekStart();
  const update = useIssueWrite(workspaceSlug, projectId, issue.id);
  const bounds = dateBounds(issue);
  return (
    <SidebarRow label="Start date">
      <DatePicker
        label="Start date"
        value={issue.start_date ?? null}
        placeholder="Add start date"
        max={bounds.startMax}
        weekStart={weekStart}
        disabled={disabled}
        testId="sidebar-start-date"
        onChange={(iso) => update.mutate({ start_date: iso })}
      />
    </SidebarRow>
  );
}

export interface DueDateRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
  stateGroup: string | null;
}

/**
 * Due date: bounded by the start date and flagged overdue by state
 * group. The cloud overdue badge beside it belongs to NEWFRONT-657; the
 * OSS build renders nothing there, as the edition spec requires.
 */
export function DueDateRow({
  workspaceSlug,
  projectId,
  issue,
  disabled,
  stateGroup,
}: DueDateRowProps): React.ReactElement {
  const weekStart = useWeekStart();
  const update = useIssueWrite(workspaceSlug, projectId, issue.id);
  const bounds = dateBounds(issue);
  const overdue = isDueDateOverdue(issue.target_date, stateGroup);
  return (
    <SidebarRow label="Due date">
      <div
        data-testid="sidebar-due-date"
        data-overdue={overdue ? "true" : undefined}
        className={cn("flex min-w-0 items-center gap-(--space-2)", overdue && "text-(--danger)")}
      >
        <DatePicker
          label="Due date"
          value={issue.target_date ?? null}
          placeholder="Add due date"
          min={bounds.dueMin}
          weekStart={weekStart}
          disabled={disabled}
          overdue={overdue}
          testId="sidebar-due-picker"
          onChange={(iso) => update.mutate({ target_date: iso })}
        />
      </div>
    </SidebarRow>
  );
}

export interface EstimateRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
}

/** Estimate row, rendered only when the project runs an estimate system. */
export function EstimateRow({ workspaceSlug, projectId, issue, disabled }: EstimateRowProps): React.ReactElement {
  const update = useIssueWrite(workspaceSlug, projectId, issue.id);
  return (
    <SidebarRow label="Estimate">
      <EstimatePicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        value={typeof issue.estimate_point === "string" ? issue.estimate_point : null}
        disabled={disabled}
        variant={ROW_VARIANT}
        placeholder="None"
        preload={issue.estimate_point !== null && issue.estimate_point !== undefined}
        onChange={(id) => update.mutate({ estimate_point: id })}
      />
    </SidebarRow>
  );
}

export interface ModulesRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
  isSaving: boolean;
  onChange: (moduleIds: string[], previousIds: string[]) => void;
}

/** Multi-module row; locked while the bridge write is in flight. */
export function ModulesRow({
  workspaceSlug,
  projectId,
  issue,
  disabled,
  isSaving,
  onChange,
}: ModulesRowProps): React.ReactElement {
  return (
    <SidebarRow label="Modules">
      <ModulePicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        multiple
        value={issue.module_ids}
        disabled={disabled || isSaving}
        variant={ROW_VARIANT}
        placeholder="No module"
        onChange={(ids) => onChange(ids, issue.module_ids)}
      />
    </SidebarRow>
  );
}

export interface CycleRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
  isSaving: boolean;
  onChange: (cycleId: string | null) => void;
}

/** Single-cycle row; locked while the bridge write is in flight. */
export function CycleRow({
  workspaceSlug,
  projectId,
  issue,
  disabled,
  isSaving,
  onChange,
}: CycleRowProps): React.ReactElement {
  return (
    <SidebarRow label="Cycle">
      <CyclePicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        value={issue.cycle_id}
        disabled={disabled || isSaving}
        variant={ROW_VARIANT}
        placeholder="No cycle"
        preload={issue.cycle_id !== null}
        onChange={onChange}
      />
    </SidebarRow>
  );
}

export interface ParentRowProps {
  issue: Issue;
  disabled: boolean;
  /** The parent's short identifier once its read lands, else null. */
  parentLabel: string | null;
  onOpenPicker: () => void;
  onRemove: () => void;
}

/** Parent row: the parent's identifier with an inline remove, or the add entry. */
export function ParentRow({
  issue,
  disabled,
  parentLabel,
  onOpenPicker,
  onRemove,
}: ParentRowProps): React.ReactElement {
  const hasParent = issue.parent_id !== null;
  return (
    <SidebarRow label="Parent">
      <div className="group flex min-w-0 items-center gap-(--space-1)">
        <Button variant={ROW_VARIANT} className={ROW_TEXT} disabled={disabled} onClick={onOpenPicker}>
          <Link2 size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
          <span className={cn("min-w-0 truncate", hasParent ? "text-(--text)" : "text-(--text-muted)")}>
            {hasParent ? (parentLabel ?? "Parent work item") : "Add parent work item"}
          </span>
          {disabled ? null : (
            <Pencil
              size={11}
              strokeWidth={1.5}
              aria-hidden="true"
              className="shrink-0 opacity-0 group-hover:opacity-100"
            />
          )}
        </Button>
        {hasParent && !disabled ? (
          <button
            type="button"
            aria-label="Remove parent"
            onClick={onRemove}
            className="inline-flex shrink-0 items-center rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:text-(--danger)"
          >
            <X size={11} strokeWidth={2} aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </SidebarRow>
  );
}

export interface LabelsRowProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  disabled: boolean;
}

/**
 * Labels row: the selected chips (removable without opening the picker)
 * plus the shared searchable picker, which handles inline create for
 * project admins. Every change PATCHes the full label id list.
 */
export function LabelsRow({ workspaceSlug, projectId, issue, disabled }: LabelsRowProps): React.ReactElement {
  const update = useIssueWrite(workspaceSlug, projectId, issue.id);
  const labels = useLabels(workspaceSlug, projectId, true);
  return (
    <SidebarRow label="Labels">
      <div className="flex min-w-0 flex-col gap-(--space-2)">
        <LabelChips
          labels={labels.data ?? []}
          selectedIds={issue.label_ids}
          onRemove={
            disabled
              ? undefined
              : (labelId) => update.mutate({ label_ids: issue.label_ids.filter((id) => id !== labelId) })
          }
        />
        <LabelPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={issue.label_ids}
          disabled={disabled}
          onChange={(ids) => update.mutate({ label_ids: ids })}
        />
      </div>
    </SidebarRow>
  );
}

export interface SidebarPropertiesBProps {
  workspaceSlug: string;
  projectId: string;
  /** The issue's project identifier, for the parent's short code. */
  identifier: string;
  issue: Issue;
  /** The project row carrying the feature switches. */
  project: Pick<SidebarProject, "identifier" | "module_view" | "cycle_view" | "estimate">;
  /** The state group the overdue highlight reads, or null while unknown. */
  stateGroup: string | null;
  /** The same read-only rule rows A use. */
  editable: boolean;
}

/**
 * Rows B as one block: the two date rows, the conditional estimate,
 * module and cycle rows, then parent and labels. The block owns the
 * membership writes, the parent dialog and the parent label read, so
 * the sidebar above it stays a plain list of rows.
 */
export function SidebarPropertiesB({
  workspaceSlug,
  projectId,
  identifier,
  issue,
  project,
  stateGroup,
  editable,
}: SidebarPropertiesBProps): React.ReactElement {
  const disabled = !editable || issue.archived_at !== null || issue.is_synced;
  const cycle = useIssueCycleWrite({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    currentCycleId: issue.cycle_id,
  });
  const modules = useIssueModulesWrite({ workspaceSlug, projectId, issueId: issue.id });
  const { setParent } = useParentWrite({ workspaceSlug, projectId, issueId: issue.id });
  const [parentOpen, setParentOpen] = React.useState(false);
  const parentRead = useQuery(
    parentIssueQueryOptions(getAppClient(), workspaceSlug, issue.project_id, issue.parent_id ?? "")
  );
  const parentLabel =
    issue.parent_id === null || parentRead.data === undefined
      ? null
      : parentIdentifier(parentRead.data.sequence_id, identifier);

  return (
    <>
      <StartDateRow workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} disabled={disabled} />
      <DueDateRow
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issue={issue}
        disabled={disabled}
        stateGroup={stateGroup}
      />
      {showsEstimateRow(project) ? (
        <EstimateRow workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} disabled={disabled} />
      ) : null}
      {showsModuleRow(project) ? (
        <ModulesRow
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issue={issue}
          disabled={disabled}
          isSaving={modules.isSavingModules}
          onChange={modules.updateModules}
        />
      ) : null}
      {showsCycleRow(project) ? (
        <CycleRow
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          issue={issue}
          disabled={disabled}
          isSaving={cycle.isSavingCycle}
          onChange={cycle.updateCycle}
        />
      ) : null}
      <ParentRow
        issue={issue}
        disabled={disabled}
        parentLabel={parentLabel}
        onOpenPicker={() => setParentOpen(true)}
        onRemove={() => void setParent(null).catch(() => undefined)}
      />
      <LabelsRow workspaceSlug={workspaceSlug} projectId={projectId} issue={issue} disabled={disabled} />
      <ParentPickerDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issueId={issue.id}
        open={parentOpen}
        onOpenChange={setParentOpen}
        onPick={(parentId) => void setParent(parentId).catch(() => undefined)}
      />
    </>
  );
}
