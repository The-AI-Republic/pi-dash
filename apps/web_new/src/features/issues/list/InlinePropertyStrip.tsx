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
import { CalendarDays, Hash, Link2, MessageSquare, Paperclip, Users } from "lucide-react";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { formatDay, formatMergedRange } from "../dropdowns/calendar.js";
import { DateRangePicker } from "../dropdowns/DateRangePicker.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import { isPropertyShown, type DisplayFeatureFlags } from "../display/matrix.js";
import { selectEstimateValueLabel } from "../../../shared/pickers/estimates.js";
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
  usePickerEstimates,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { stateOptionGate } from "../edition/workflow.js";
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
  /**
   * Virtualized rows mount their pickers lazily: static triggers render at
   * rest and the picker machinery mounts on first hover, focus or click.
   * The visible strip is identical; this keeps 2,000-row scrolling cheap.
   */
  deferPickers?: boolean | undefined;
  /** Project timezone for "today"; absent falls back to the viewer's zone. */
  timeZone?: string | null | undefined;
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

/** An ISO day rendered in the given zone, or the viewer's zone when absent. */
export function todayIsoInZone(now: Date = new Date(), timeZone?: string | null): string {
  const parts = new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    ...(timeZone === null || timeZone === undefined || timeZone === "" ? {} : { timeZone }),
  }).formatToParts(now);
  const pick = (type: Intl.DateTimeFormatPartTypes): string => parts.find((part) => part.type === type)?.value ?? "";
  return `${pick("year")}-${pick("month")}-${pick("day")}`;
}

/** Properties in the strip's render order; the display set filters this. */
export const INLINE_PROPERTY_ORDER: readonly string[] = [
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

/**
 * Mounts a picker on first interaction when `defer` is set. The trigger is
 * the picker's own trigger element, so the static chip and the interactive
 * control are the same markup. Keyboard activation opens the popup and its
 * initial focus (search field or list) takes over.
 */
function DeferredPicker({
  defer,
  disabled,
  trigger,
  children,
}: {
  defer: boolean;
  disabled: boolean;
  trigger: React.ReactElement<Record<string, unknown>>;
  children: (open: boolean | undefined, onOpenChange: ((open: boolean) => void) | undefined) => React.ReactNode;
}): React.ReactElement {
  const [active, setActive] = React.useState(false);
  const [open, setOpen] = React.useState(false);

  if (!defer || active) return <>{children(defer ? open : undefined, setOpen)}</>;

  function activate(nextOpen: boolean): void {
    if (disabled) return;
    setOpen(nextOpen);
    setActive(true);
  }

  return React.cloneElement(trigger, {
    onPointerEnter: () => {
      if (!disabled) setActive(true);
    },
    onPointerDown: (event: React.PointerEvent) => {
      if (event.pointerType === "touch") activate(true);
    },
    onClick: (event: React.MouseEvent) => {
      event.preventDefault();
      activate(true);
    },
    onKeyDown: (event: React.KeyboardEvent) => {
      if (event.key === "Enter" || event.key === " " || event.key === "ArrowDown") {
        event.preventDefault();
        activate(true);
      }
    },
  });
}

/** One inline property, or null when hidden/unavailable. */
function InlinePropertyStripImpl({
  workspaceSlug,
  projectId,
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
  deferPickers = false,
  timeZone = null,
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
  // A set point names itself on the chip in every state (EST-022), so the
  // systems load beside the row rather than only when the popup opens. The
  // read is shared by every row of the project and skipped otherwise.
  const estimatePoint =
    typeof issue.estimate_point === "string" && issue.estimate_point !== "" ? issue.estimate_point : null;
  const estimates = usePickerEstimates(
    workspaceSlug,
    projectId,
    estimateEnabled && keys.includes("estimate") && estimatePoint !== null
  );
  // The chip follows the peek rule rather than the sheet's raw-id fallback:
  // an id no system lists reads as the empty placeholder, as the picker does.
  const estimateText = selectEstimateValueLabel(estimates.data, estimatePoint) ?? "None";
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
  const overdue = isOverdue(issue, todayIsoInZone(new Date(), timeZone), stateGroup);
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
        if (key === "state") {
          const stateTrigger = (
            <button type="button" disabled={disabled} className={chipClasses(true)}>
              {StateIcon === null || state === undefined ? null : (
                <StateIcon size={14} strokeWidth={1.5} style={{ color: state.color }} aria-hidden="true" />
              )}
              <span>{stateNames.get(issue.state_id ?? "") ?? "No state"}</span>
            </button>
          );
          return (
            <span
              key={key}
              className="inline-flex min-w-0 shrink-0"
              title={`State: ${stateNames.get(issue.state_id ?? "") ?? "None"}`}
            >
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={stateTrigger}>
                {(open, onOpenChange) => (
                  <StatePicker
                    workspaceSlug={workspaceSlug}
                    projectId={projectId}
                    value={issue.state_id}
                    onChange={(id) => patch.patchIssue({ state_id: id })}
                    disabled={disabled}
                    variant="ghost"
                    trigger={stateTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                    optionGate={stateOptionGate(projectId, issue.state_id)}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "priority") {
          const priorityTrigger = <PriorityButton level={issue.priority} disabled={disabled} />;
          return (
            <span key={key} className="inline-flex shrink-0" title={`Priority: ${issue.priority}`}>
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={priorityTrigger}>
                {(open, onOpenChange) => (
                  <PriorityPicker
                    value={issue.priority}
                    onChange={(priority) => patch.patchIssue({ priority })}
                    disabled={disabled}
                    trigger={priorityTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (mergedDates && key === "start_date") {
          const datesTrigger = (
            <button
              type="button"
              aria-label="Dates"
              disabled={disabled}
              data-testid="row-dates-trigger"
              className={cn(
                "text-caption inline-flex h-5 items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) hover:bg-(--subtle) disabled:cursor-not-allowed",
                overdue ? "text-(--danger)" : "text-(--text-muted)"
              )}
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
              <span className="truncate">{formatMergedRange(issue.start_date ?? "", issue.target_date ?? "")}</span>
            </button>
          );
          return (
            <span
              key="dates"
              data-overdue={overdue ? "true" : undefined}
              className={cn("shrink-0", overdue && "text-(--danger)")}
            >
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={datesTrigger}>
                {(open, onOpenChange) => (
                  <DateRangePicker
                    value={{ start: issue.start_date ?? null, end: issue.target_date ?? null }}
                    onChange={(range) => patch.patchIssue({ start_date: range.start, target_date: range.end })}
                    mode="merged"
                    label="Dates"
                    disabled={disabled}
                    clearable
                    testId="row-dates"
                    trigger={datesTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (mergedDates && key === "due_date") return null;
        if (key === "start_date") {
          const startTrigger = (
            <button
              type="button"
              aria-label="Start date"
              disabled={disabled}
              className={chipClasses(issue.start_date !== null && issue.start_date !== undefined)}
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
              <span className="truncate">
                {issue.start_date === null || issue.start_date === undefined
                  ? "Start date"
                  : formatDay(issue.start_date)}
              </span>
            </button>
          );
          return (
            <span key={key} className="shrink-0">
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={startTrigger}>
                {(open, onOpenChange) => (
                  <DatePicker
                    value={issue.start_date ?? null}
                    onChange={(start) => patch.patchIssue({ start_date: start })}
                    max={issue.target_date ?? null}
                    label="Start date"
                    placeholder="Start date"
                    disabled={disabled}
                    clearable
                    testId="row-start-date"
                    trigger={startTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "due_date") {
          const dueTrigger = (
            <button
              type="button"
              aria-label="Due date"
              disabled={disabled}
              className={cn(
                chipClasses(issue.target_date !== null && issue.target_date !== undefined),
                overdue && "text-(--danger)"
              )}
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
              <span className="truncate">
                {issue.target_date === null || issue.target_date === undefined
                  ? "Due date"
                  : formatDay(issue.target_date)}
              </span>
            </button>
          );
          return (
            <span
              key={key}
              data-overdue={overdue ? "true" : undefined}
              className={cn("shrink-0", overdue && "text-(--danger)")}
            >
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={dueTrigger}>
                {(open, onOpenChange) => (
                  <DatePicker
                    value={issue.target_date ?? null}
                    onChange={(target) => patch.patchIssue({ target_date: target })}
                    min={issue.start_date ?? null}
                    label="Due date"
                    placeholder="Due date"
                    disabled={disabled}
                    clearable
                    testId="row-due-date"
                    trigger={dueTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "assignee") {
          const assigneeTrigger = (
            <button
              type="button"
              aria-label={`Assignees: ${assigneeLabel}`}
              disabled={disabled}
              className={chipClasses(issue.assignee_ids.length > 0)}
            >
              <Users size={14} strokeWidth={1.5} aria-hidden="true" />
              {issue.assignee_ids.length > 0 ? <span>{issue.assignee_ids.length}</span> : null}
            </button>
          );
          return (
            <span key={key} className="inline-flex shrink-0" title={assigneeLabel}>
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={assigneeTrigger}>
                {(open, onOpenChange) => (
                  <MemberPicker
                    workspaceSlug={workspaceSlug}
                    projectId={projectId}
                    multiple
                    value={issue.assignee_ids}
                    onChange={(ids) => patch.patchIssue({ assignee_ids: ids })}
                    disabled={disabled}
                    iconOnly
                    trigger={assigneeTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "modules") {
          const moduleTrigger = (
            <button type="button" disabled={disabled} className={chipClasses(issue.module_ids.length > 0)}>
              <span className="truncate">{moduleText}</span>
            </button>
          );
          return (
            <span key={key} className="inline-flex min-w-0 shrink-0" title={`Modules: ${moduleText}`}>
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={moduleTrigger}>
                {(open, onOpenChange) => (
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
                    trigger={moduleTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "cycle") {
          const cycleTrigger = (
            <button type="button" disabled={disabled} className={chipClasses(issue.cycle_id !== null)}>
              <span className="truncate">{cycleText}</span>
            </button>
          );
          return (
            <span key={key} className="inline-flex min-w-0 shrink-0" title={`Cycle: ${cycleText}`}>
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={cycleTrigger}>
                {(open, onOpenChange) => (
                  <CyclePicker
                    workspaceSlug={workspaceSlug}
                    projectId={projectId}
                    value={issue.cycle_id}
                    onChange={(id) => cycleMove.moveToCycle(id, issue.cycle_id)}
                    disabled={disabled}
                    trigger={cycleTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
            </span>
          );
        }
        if (key === "estimate") {
          const estimateTrigger = (
            <button
              type="button"
              aria-label={`Estimate: ${estimateText}`}
              data-testid="row-estimate-trigger"
              disabled={disabled}
              className={chipClasses(estimatePoint !== null)}
            >
              <Hash size={14} strokeWidth={1.5} aria-hidden="true" />
              <span className="truncate">{estimateText}</span>
            </button>
          );
          return (
            <span key={key} className="inline-flex min-w-0 shrink-0" title={`Estimate: ${estimateText}`}>
              <DeferredPicker defer={deferPickers} disabled={disabled} trigger={estimateTrigger}>
                {(open, onOpenChange) => (
                  <EstimatePicker
                    workspaceSlug={workspaceSlug}
                    projectId={projectId}
                    value={estimatePoint}
                    onChange={(id) => patch.patchIssue({ estimate_point: id })}
                    disabled={disabled}
                    trigger={estimateTrigger}
                    open={open}
                    onOpenChange={onOpenChange}
                  />
                )}
              </DeferredPicker>
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
            <DeferredPicker
              defer={deferPickers}
              disabled={disabled}
              trigger={<LabelsTrigger label={`Labels: ${labelText}`} disabled={disabled} />}
            >
              {(open, onOpenChange) => (
                <LabelPicker
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  value={issue.label_ids}
                  onChange={(ids) => patch.patchIssue({ label_ids: ids })}
                  disabled={disabled}
                  label={`Labels: ${labelText}`}
                  open={open}
                  onOpenChange={onOpenChange}
                />
              )}
            </DeferredPicker>
          </span>
        );
      })}
    </div>
  );
}

/**
 * Label trigger named by the caller for the deferred path; the mounted
 * picker renders its own trigger. DeferredPicker hands this element its
 * click and key handlers, so anything beyond the named props must land
 * on the button rather than be swallowed here.
 */
function LabelsTrigger({
  label,
  disabled,
  ...rest
}: {
  label: string;
  disabled: boolean;
} & React.ButtonHTMLAttributes<HTMLButtonElement>): React.ReactElement {
  return (
    <button
      {...rest}
      type="button"
      aria-label={label}
      disabled={disabled}
      className="text-caption inline-flex h-5 items-center rounded-(--radius-control) px-(--space-1) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed"
    >
      <span className="max-w-24 truncate">{label.replace(/^Labels: /, "")}</span>
    </button>
  );
}

/**
 * Icon-only priority trigger; its inner class is the parity value marker.
 * Forwards injected trigger props like LabelsTrigger above: without the
 * spread the chip renders with no click or keyboard handler at all.
 */
function PriorityButton({
  level,
  disabled,
  ...rest
}: {
  level: PriorityLevel;
  disabled: boolean;
} & React.ButtonHTMLAttributes<HTMLButtonElement>): React.ReactElement {
  const Icon = PRIORITY_ICONS[level];
  return (
    <button
      {...rest}
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

/**
 * Rows re-render while the virtualizer scrolls; memo keeps the strip's
 * pickers and mutation hooks off the scroll path for rows that stay
 * mounted (Quality gates: 2,000-row frame budget).
 */
export const InlinePropertyStrip = React.memo(InlinePropertyStripImpl);
