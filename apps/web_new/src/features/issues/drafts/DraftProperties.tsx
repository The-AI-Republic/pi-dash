// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline property strip on a draft block (parity/specs/issues.md §12,
// ISS-138). Each control writes exactly the field it names; the strip
// only draws modules, cycles and estimates when the draft's project
// enables them. A due day reads danger-toned when overdue. Clicks stop
// at the strip so the block's double-click edit and context menu stay
// quiet while a picker is in use.

import type { DraftIssue, ProjectDetails } from "@pidash/api-client";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
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
import { usePickerEstimates, usePickerStates } from "../../../shared/pickers/reference.js";
import {
  asPriority,
  assigneePatch,
  cyclePatch,
  estimatePatch,
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
  const estimates = usePickerEstimates(workspaceSlug, projectId, projectId !== "" && project?.estimate != null);
  const stateGroup = stateGroupOf(states.data, draft.state_id);
  const overdue = isDraftOverdue(draft.target_date ?? null, stateGroup, today);
  const stop = (event: React.SyntheticEvent): void => {
    event.stopPropagation();
  };
  const disabled = !editable || projectId === "";

  return (
    <div
      data-testid="draft-properties"
      className={`flex flex-wrap items-center gap-(--space-2) ${className ?? ""}`}
      onClick={stop}
      onDoubleClick={stop}
    >
      <StatePicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        value={draft.state_id}
        disabled={disabled}
        placeholder="State"
        onChange={(stateId) => onWrite(statePatch(stateId))}
      />
      <PriorityPicker
        value={asPriority(draft.priority) as PriorityLevel}
        disabled={disabled}
        onChange={(priority) => onWrite(priorityPatch(priority))}
      />
      <LabelPicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        value={draft.label_ids}
        disabled={disabled}
        emptyText="Labels"
        onChange={(labelIds) => onWrite(labelPatch(labelIds))}
      />
      <DatePicker
        label="Start date"
        value={draft.start_date ?? null}
        max={draft.target_date ?? null}
        weekStart={weekStart}
        disabled={disabled}
        placeholder="Start date"
        testId="draft-start-date"
        onChange={(date) => onWrite(startDatePatch(date))}
      />
      <div className={overdue ? "text-(--danger)" : undefined}>
        <DatePicker
          label="Due date"
          value={draft.target_date ?? null}
          min={draft.start_date ?? null}
          weekStart={weekStart}
          disabled={disabled}
          placeholder="Due date"
          testId="draft-due-date"
          onChange={(date) => onWrite(targetDatePatch(date))}
        />
      </div>
      <MemberPicker
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        multiple
        value={draft.assignee_ids}
        disabled={disabled}
        placeholder="Assignees"
        onChange={(assigneeIds) => onWrite(assigneePatch(assigneeIds))}
      />
      {project?.module_view === true ? (
        <ModulePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          multiple
          value={draft.module_ids}
          disabled={disabled}
          placeholder="Modules"
          onChange={(moduleIds) => onWrite(modulePatch(moduleIds))}
        />
      ) : null}
      {project?.cycle_view === true ? (
        <CyclePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={draft.cycle_id}
          disabled={disabled}
          placeholder="Cycle"
          onChange={(cycleId) => onWrite(cyclePatch(cycleId))}
        />
      ) : null}
      {project?.estimate != null && (estimates.data?.length ?? 0) > 0 ? (
        <EstimatePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={draft.estimate_point === undefined ? null : String(draft.estimate_point ?? "") || null}
          disabled={disabled}
          onChange={(estimatePoint) => onWrite(estimatePatch(estimatePoint))}
        />
      ) : null}
    </div>
  );
}
