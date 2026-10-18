// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared inline property strip (parity/specs/issues.md §2, ISS-014).
// One row's display-enabled properties in the spec's order: state,
// priority, dates, assignee, modules, cycle, estimate, read-only counts
// and labels. Editable pickers write through the issue PATCH or the
// cycle/module bridges; counts are read-only. Guests, archived work
// items and completed cycles render the whole strip disabled. The list
// and board cards reuse this component.

import type { IssueListItem, State } from "@pidash/api-client";
import { cn } from "@pidash/kit";
import { Link2, MessageSquare, Paperclip, Users } from "lucide-react";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { DateRangePicker } from "../dropdowns/DateRangePicker.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import { isPropertyShown, type DisplayFeatureFlags } from "../display/matrix.js";
import {
  CyclePicker,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PRIORITY_ICONS,
  PRIORITY_TONES,
  PriorityPicker,
  STATE_GROUP_ICONS,
  StatePicker,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { useListCycleMove, useListIssuePatch, useListModuleWrite } from "./mutations.js";

export interface InlinePropertyStripProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issue: IssueListItem;
  properties: Record<string, boolean>;
  flags: DisplayFeatureFlags;
  /** Inline editing is off for guests, archived items or completed cycles. */
  readOnly: boolean;
  /** Whether the project has an estimate system configured. */
  estimateEnabled: boolean;
  /** State rows behind the state chip's name and glyph. */
  states: readonly State[];
  stateNames: ReadonlyMap<string, string>;
  assigneeNames: ReadonlyMap<string, string>;
  labelNames: ReadonlyMap<string, string>;
  cycleNames: ReadonlyMap<string, string>;
  moduleNames: ReadonlyMap<string, string>;
  /** Ids of cycles that are still open (read-only otherwise). */
  openCycleIds: ReadonlySet<string>;
}

/** Whether a due date is due or past and the item is still open. */
export function isOverdue(
  issue: Pick<IssueListItem, "target_date" | "completed_at">,
  today: string,
  stateGroup: string | null | undefined
): boolean {
  if (stateGroup === "completed" || stateGroup === "cancelled") return false;
  if (issue.completed_at !== null && issue.completed_at !== undefined) return false;
  return issue.target_date !== null && issue.target_date !== undefined && issue.target_date <= today;
}

function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}

/** Properties in the strip's render order; the display set filters this. */
export const INLINE_PROPERTY_ORDER: readonly string[] = [
  "key",
  "state",
  "priority",
  "start_date",
  "due_date",
  "assignee",
  "modules",
  "cycle",
  "estimate",
  "sub_issue_count",
  "attachment_count",
  "link",
  "labels",
];

function Identifier({
  projectIdentifier,
  issue,
}: {
  projectIdentifier: string;
  issue: IssueListItem;
}): React.ReactElement {
  const text = issue.sequence_id === 0 ? "…" : `${projectIdentifier}-${issue.sequence_id}`;
  return <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">{text}</span>;
}

/** One inline property, or null when hidden/unavailable. */
export function InlinePropertyStrip({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issue,
  properties,
  flags,
  readOnly,
  estimateEnabled,
  states,
  stateNames,
  assigneeNames,
  labelNames,
  cycleNames,
  moduleNames,
  openCycleIds,
}: InlinePropertyStripProps): React.ReactElement {
  const patch = useListIssuePatch(workspaceSlug, projectId, issue.id);
  const cycleMove = useListCycleMove(workspaceSlug, projectId, issue.id);
  const moduleWrite = useListModuleWrite(workspaceSlug, projectId, issue.id);
  const cycleCompleted = issue.cycle_id !== null && !openCycleIds.has(issue.cycle_id);
  const disabled = readOnly || cycleCompleted || issue.archived_at !== null;
  const keys = INLINE_PROPERTY_ORDER.filter(
    (key) =>
      isPropertyShown(properties, key) &&
      (key !== "cycle" || !flags.hideCycle) &&
      (key !== "modules" || !flags.hideModule) &&
      (key !== "estimate" || estimateEnabled)
  );
  const state = states.find((row) => row.id === issue.state_id);
  const stateGroup = state?.group ?? null;
  const StateIcon = state === undefined ? null : STATE_GROUP_ICONS[state.group];
  const assigneeLabel =
    issue.assignee_ids.length === 0
      ? "Assignee"
      : issue.assignee_ids
          .map((id) => assigneeNames.get(id) ?? "Member")
          .slice(0, 2)
          .join(", ");
  const labelText =
    issue.label_ids.length === 0
      ? "Labels"
      : issue.label_ids
          .map((id) => labelNames.get(id) ?? "Label")
          .slice(0, 2)
          .join(", ");
  const moduleText =
    issue.module_ids.length === 0
      ? "Modules"
      : issue.module_ids.map((id) => moduleNames.get(id) ?? "Module").join(", ");
  const cycleText = issue.cycle_id === null ? "Cycle" : (cycleNames.get(issue.cycle_id) ?? "Cycle");
  const overdue = isOverdue(issue, todayIso(), stateGroup);
  const mergedDates =
    issue.start_date !== null &&
    issue.start_date !== undefined &&
    issue.target_date !== null &&
    issue.target_date !== undefined &&
    keys.includes("start_date") &&
    keys.includes("due_date");

  return (
    <div data-testid="inline-property-strip" className="flex min-w-0 flex-wrap items-center gap-(--space-2)">
      {keys.map((key) => {
        if (key === "key") return <Identifier key={key} projectIdentifier={projectIdentifier} issue={issue} />;
        if (key === "state") {
          return (
            <span
              key={key}
              className="inline-flex min-w-0 shrink-0"
              title={`State: ${stateNames.get(issue.state_id ?? "") ?? "None"}`}
            >
              <StatePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={issue.state_id}
                onChange={(id) => patch.patchIssue({ state_id: id })}
                disabled={disabled}
                variant="ghost"
                trigger={
                  <button type="button" disabled={disabled} className={chipClasses(true)}>
                    {StateIcon === null || state === undefined ? null : (
                      <StateIcon size={14} strokeWidth={1.5} style={{ color: state.color }} aria-hidden="true" />
                    )}
                    <span>{stateNames.get(issue.state_id ?? "") ?? "No state"}</span>
                  </button>
                }
              />
            </span>
          );
        }
        if (key === "priority") {
          return (
            <span key={key} className="inline-flex shrink-0" title={`Priority: ${issue.priority}`}>
              <PriorityPicker
                value={issue.priority}
                onChange={(priority) => patch.patchIssue({ priority })}
                disabled={disabled}
                trigger={<PriorityButton level={issue.priority} disabled={disabled} />}
              />
            </span>
          );
        }
        if (mergedDates && key === "start_date") {
          return (
            <span
              key="dates"
              data-overdue={overdue ? "true" : undefined}
              className={cn("shrink-0", overdue && "text-(--danger)")}
            >
              <DateRangePicker
                value={{ start: issue.start_date ?? null, end: issue.target_date ?? null }}
                onChange={(range) => patch.patchIssue({ start_date: range.start, target_date: range.end })}
                mode="merged"
                label="Dates"
                disabled={disabled}
                clearable
                testId="row-dates"
              />
            </span>
          );
        }
        if (mergedDates && key === "due_date") return null;
        if (key === "start_date") {
          return (
            <span key={key} className="shrink-0">
              <DatePicker
                value={issue.start_date ?? null}
                onChange={(start) => patch.patchIssue({ start_date: start })}
                max={issue.target_date ?? null}
                label="Start date"
                placeholder="Start date"
                disabled={disabled}
                clearable
                testId="row-start-date"
              />
            </span>
          );
        }
        if (key === "due_date") {
          return (
            <span
              key={key}
              data-overdue={overdue ? "true" : undefined}
              className={cn("shrink-0", overdue && "text-(--danger)")}
            >
              <DatePicker
                value={issue.target_date ?? null}
                onChange={(target) => patch.patchIssue({ target_date: target })}
                min={issue.start_date ?? null}
                label="Due date"
                placeholder="Due date"
                disabled={disabled}
                clearable
                testId="row-due-date"
              />
            </span>
          );
        }
        if (key === "assignee") {
          return (
            <span key={key} className="inline-flex shrink-0" title={assigneeLabel}>
              <MemberPicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                multiple
                value={issue.assignee_ids}
                onChange={(ids) => patch.patchIssue({ assignee_ids: ids })}
                disabled={disabled}
                iconOnly
                trigger={
                  <button
                    type="button"
                    aria-label={`Assignees: ${assigneeLabel}`}
                    disabled={disabled}
                    className={chipClasses(issue.assignee_ids.length > 0)}
                  >
                    <Users size={14} strokeWidth={1.5} aria-hidden="true" />
                    {issue.assignee_ids.length > 0 ? <span>{issue.assignee_ids.length}</span> : null}
                  </button>
                }
              />
            </span>
          );
        }
        if (key === "modules") {
          return (
            <span key={key} className="inline-flex min-w-0 shrink-0" title={`Modules: ${moduleText}`}>
              <ModulePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                multiple
                value={issue.module_ids}
                onChange={(ids) => {
                  const removed = issue.module_ids.filter((id) => !ids.includes(id));
                  moduleWrite.writeModules(ids, removed);
                }}
                disabled={disabled}
                trigger={
                  <button type="button" disabled={disabled} className={chipClasses(issue.module_ids.length > 0)}>
                    <span className="truncate">{moduleText}</span>
                  </button>
                }
              />
            </span>
          );
        }
        if (key === "cycle") {
          return (
            <span key={key} className="inline-flex min-w-0 shrink-0" title={`Cycle: ${cycleText}`}>
              <CyclePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={issue.cycle_id}
                onChange={(id) => cycleMove.moveToCycle(id, issue.cycle_id)}
                disabled={disabled}
                trigger={
                  <button type="button" disabled={disabled} className={chipClasses(issue.cycle_id !== null)}>
                    <span className="truncate">{cycleText}</span>
                  </button>
                }
              />
            </span>
          );
        }
        if (key === "estimate") {
          return (
            <span key={key} className="inline-flex shrink-0">
              <EstimatePicker
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                value={typeof issue.estimate_point === "string" ? issue.estimate_point : null}
                onChange={(id) => patch.patchIssue({ estimate_point: id })}
                disabled={disabled}
                iconOnly
              />
            </span>
          );
        }
        if (key === "sub_issue_count") {
          if (issue.is_epic === true) return null;
          return (
            <ReadOnlyCount
              key={key}
              icon={<MessageSquare size={14} strokeWidth={1.5} aria-hidden="true" />}
              value={issue.sub_issues_count}
              label="Sub-items"
            />
          );
        }
        if (key === "attachment_count") {
          return (
            <ReadOnlyCount
              key={key}
              icon={<Paperclip size={14} strokeWidth={1.5} aria-hidden="true" />}
              value={issue.attachment_count}
              label="Attachments"
            />
          );
        }
        if (key === "link") {
          return (
            <ReadOnlyCount
              key={key}
              icon={<Link2 size={14} strokeWidth={1.5} aria-hidden="true" />}
              value={issue.link_count}
              label="Links"
            />
          );
        }
        return (
          <span key={key} className="inline-flex shrink-0">
            <LabelPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={issue.label_ids}
              onChange={(ids) => patch.patchIssue({ label_ids: ids })}
              disabled={disabled}
              label={`Labels: ${labelText}`}
            />
          </span>
        );
      })}
    </div>
  );
}

/** Icon-only priority trigger; its inner class is the parity value marker. */
function PriorityButton({ level, disabled }: { level: PriorityLevel; disabled: boolean }): React.ReactElement {
  const Icon = PRIORITY_ICONS[level];
  return (
    <button
      type="button"
      aria-label={`Priority: ${level}`}
      disabled={disabled}
      className="inline-flex h-5 w-5 items-center justify-center rounded-(--radius-control) disabled:cursor-not-allowed"
    >
      <Icon
        size={16}
        strokeWidth={1.5}
        className={cn(PRIORITY_TONES[level], `border-priority-${level} border-0`)}
        aria-hidden="true"
      />
    </button>
  );
}

function chipClasses(active: boolean): string {
  return cn(
    "text-caption inline-flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) py-px disabled:cursor-not-allowed",
    active ? "text-(--text) hover:bg-(--subtle)" : "text-(--text-muted) hover:bg-(--subtle)"
  );
}

function ReadOnlyCount({
  icon,
  value,
  label,
}: {
  icon: React.ReactNode;
  value: number | null;
  label: string;
}): React.ReactElement | null {
  if (value === null || value === 0) return null;
  return (
    <span
      aria-label={`${label}: ${value}`}
      className="text-caption inline-flex shrink-0 items-center gap-(--space-1) text-(--text-muted)"
    >
      {icon}
      <span>{value}</span>
    </span>
  );
}
