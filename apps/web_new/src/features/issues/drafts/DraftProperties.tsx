// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline property strip on a draft block (parity/specs/issues.md §12,
// ISS-138; parity/specs/drafts.md §3.2, DRAFT-012–016). Each control
// writes exactly the field it names, except the dates, which always
// travel as a pair; the strip only draws modules, cycles and estimates
// when the draft's project enables them. A due day reads danger-toned
// when overdue. Clicks stop at the strip so the block's double-click
// edit and context menu stay quiet while a picker is in use.

import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
import { useLabels } from "../labels/api.js";
import { LabelPicker } from "../labels/LabelPicker.js";
import {
  CyclePicker,
  EstimatePicker,
  MemberPicker,
  ModulePicker,
  PriorityPicker,
  StatePicker,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { usePickerStates } from "../../../shared/pickers/reference.js";
import { notifyError } from "../../../shared/shell/index.js";
import {
  asPriority,
  assigneePatch,
  draftLabelsTriggerText,
  draftStateId,
  cyclePatch,
  estimatePatch,
  isDateRangeValid,
  isDraftOverdue,
  labelPatch,
  modulePatch,
  priorityPatch,
  startDatePatch,
  statePatch,
  targetDatePatch,
  type DraftFieldPatch,
} from "./model.js";

export interface DraftPropertiesProps {
  workspaceSlug: string;
  draft: DraftIssue;
  /** The draft's project row, once the projects read resolves. */
  project: ProjectDetails | undefined;
  /** Members edit; guests and viewers without standing see the same values locked. */
  editable: boolean;
  onWrite: (patch: DraftFieldPatch) => void;
  className?: string | undefined;
}

/** The state's group from the project workflow, for the overdue read. */
export function stateGroupOf(
  states: readonly { id: string; group: string }[] | undefined,
  stateId: string | null
): string | null {
  if (stateId === null || states === undefined) return null;
  return states.find((state) => state.id === stateId)?.group ?? null;
}

/** Notice when a date pick would invert the stored range. */
export const DRAFT_DATE_RANGE_TITLE = "Couldn't save the dates";
export const DRAFT_DATE_RANGE_BODY = "The start day must not fall after the due day.";

export function DraftProperties({
  workspaceSlug,
  draft,
  project,
  editable,
  onWrite,
  className,
}: DraftPropertiesProps): React.ReactElement {
  const weekStart = useWeekStart();
  const today = todayIsoDay();
  const projectId = draft.project_id ?? "";
  const states = usePickerStates(workspaceSlug, projectId, projectId !== "");
  const labels = useLabels(workspaceSlug, projectId, projectId !== "");
  // The picker reads the project default while the draft holds no state
  // (the old block does), but the overdue tone follows only the state the
  // draft actually stores — an unset state reads plain.
  const stateId = draftStateId(draft, project);
  const stateGroup = stateGroupOf(states.data, draft.state_id);
  const overdue = isDraftOverdue(draft.target_date ?? null, stateGroup, today);
  const stop = (event: React.SyntheticEvent): void => {
    event.stopPropagation();
  };
  const disabled = !editable || projectId === "";
  const labelText = draftLabelsTriggerText(draft.label_ids, labels.data ?? [], "Labels");

  // A date pick always writes both ends so the server range check runs;
  // an inverted pair is refused before any write with the prior dates
  // untouched (parity/specs/drafts.md §3.2).
  function writeDate(field: "start" | "target", date: string | null): void {
    const patch = field === "start" ? startDatePatch(date, draft.target_date) : targetDatePatch(date, draft.start_date);
    if (!isDateRangeValid(patch.start_date ?? null, patch.target_date ?? null)) {
      notifyError(DRAFT_DATE_RANGE_TITLE, DRAFT_DATE_RANGE_BODY);
      return;
    }
    onWrite(patch);
  }

  return (
    <div
      data-testid="draft-properties"
      className={`flex flex-wrap items-center gap-(--space-2) ${className ?? ""}`}
      onClick={stop}
      onDoubleClick={stop}
    >
      <div data-draft-property="state">
        <StatePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={stateId}
          disabled={disabled}
          placeholder="State"
          preload
          onChange={(stateId) => onWrite(statePatch(stateId))}
        />
      </div>
      <div data-draft-property="priority">
        <PriorityPicker
          value={asPriority(draft.priority) as PriorityLevel}
          disabled={disabled}
          onChange={(priority) => onWrite(priorityPatch(priority))}
        />
      </div>
      <div data-draft-property="labels">
        <LabelPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={draft.label_ids}
          disabled={disabled}
          emptyText="Labels"
          trigger={
            <button
              type="button"
              aria-label="Labels"
              disabled={disabled}
              data-testid="draft-labels-trigger"
              className="text-body inline-flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-2) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-50"
            >
              <span className="min-w-0 flex-1 truncate text-left">{labelText}</span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
            </button>
          }
          onChange={(labelIds) => onWrite(labelPatch(labelIds))}
        />
      </div>
      <div data-draft-property="start">
        <DatePicker
          label="Start date"
          value={draft.start_date ?? null}
          max={draft.target_date ?? null}
          weekStart={weekStart}
          disabled={disabled}
          placeholder="Start date"
          testId="draft-start-date"
          onChange={(date) => writeDate("start", date)}
        />
      </div>
      <div data-draft-property="due" className={overdue ? "text-(--danger)" : undefined}>
        <DatePicker
          label="Due date"
          value={draft.target_date ?? null}
          min={draft.start_date ?? null}
          weekStart={weekStart}
          disabled={disabled}
          placeholder="Due date"
          testId="draft-due-date"
          onChange={(date) => writeDate("target", date)}
        />
      </div>
      <div data-draft-property="assignees">
        <MemberPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          multiple
          value={draft.assignee_ids}
          disabled={disabled}
          placeholder="Assignees"
          preload
          onChange={(assigneeIds) => onWrite(assigneePatch(assigneeIds))}
        />
      </div>
      {project?.module_view === true ? (
        <div data-draft-property="modules">
          <ModulePicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            multiple
            value={draft.module_ids}
            disabled={disabled}
            placeholder="Modules"
            onChange={(moduleIds) => onWrite(modulePatch(moduleIds))}
          />
        </div>
      ) : null}
      {project?.cycle_view === true ? (
        <div data-draft-property="cycle">
          <CyclePicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            value={draft.cycle_id}
            disabled={disabled}
            placeholder="Cycle"
            onChange={(cycleId) => onWrite(cyclePatch(cycleId))}
          />
        </div>
      ) : null}
      {project?.estimate != null ? (
        <div data-draft-property="estimate">
          <EstimatePicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            value={draft.estimate_point === undefined ? null : String(draft.estimate_point ?? "") || null}
            disabled={disabled}
            preload={draft.estimate_point !== undefined && draft.estimate_point !== null}
            onChange={(estimatePoint) => onWrite(estimatePatch(estimatePoint))}
          />
        </div>
      ) : null}
    </div>
  );
}
