// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request properties panel (parity/specs/intake.md INT-019/026). The
// shared pickers composed for intake's limited subset: the triage state
// read-only, assignees, priority, due date and labels editable — every
// write through the intake update, which reverts to the server value
// with a notice when refused. Viewers who may not edit get the same
// rows disabled, and the labels row omits its add control.

import { cn, type ButtonVariant } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import * as React from "react";

import {
  IntakeStatePicker,
  MemberPicker,
  PriorityPicker,
  usePickerStates,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import {
  DatePicker,
  LabelChips,
  LabelPicker,
  SidebarRow,
  dateBounds,
  isDueDateOverdue,
  useLabels,
  useWeekStart,
} from "../../issues/index.js";
import { useUpdateIntakeIssue } from "../api/mutations.js";

/** Notice title when a property write is refused. */
export const INTAKE_PROPERTY_SAVE_FAILED_TITLE = "Couldn't update the request";

/** Row controls sit inline and read as text until hovered. */
const ROW_VARIANT: ButtonVariant = "ghost";

/**
 * The state row is display-only (INT-019/026): the picker never opens,
 * so selections are impossible, and it keeps its trigger shape so the
 * row reads like the other property rows.
 */
export function ignoreIntakeStateSelection(_id: string): void {}

export interface IntakePropertiesProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  /** Admin or non-guest creator on an open request; everyone else reads. */
  canEdit: boolean;
}

export function IntakeProperties({
  workspaceSlug,
  projectId,
  issue,
  canEdit,
}: IntakePropertiesProps): React.ReactElement {
  const disabled = !canEdit;
  const update = useUpdateIntakeIssue({
    workspaceSlug,
    projectId,
    issueId: issue.id,
    failureTitle: INTAKE_PROPERTY_SAVE_FAILED_TITLE,
  });
  const weekStart = useWeekStart();
  const labels = useLabels(workspaceSlug, projectId, true);
  // The overdue highlight reads the state's group, so the project states
  // load with the panel rather than waiting for the state picker.
  const states = usePickerStates(workspaceSlug, projectId, true);
  const stateGroup = states.data?.find((state) => state.id === issue.state_id)?.group ?? null;
  const bounds = dateBounds(issue);
  const overdue = isDueDateOverdue(issue.target_date, stateGroup);

  return (
    <div data-testid="intake-properties" className="flex flex-col gap-(--space-1)">
      <SidebarRow label="State">
        <IntakeStatePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={issue.state_id}
          disabled
          variant={ROW_VARIANT}
          preload
          onChange={ignoreIntakeStateSelection}
        />
      </SidebarRow>
      <SidebarRow label="Assignees">
        <MemberPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          multiple
          value={issue.assignee_ids}
          disabled={disabled}
          variant={ROW_VARIANT}
          placeholder="Add assignees"
          preload={issue.assignee_ids.length > 0}
          onChange={(ids) => update.mutate({ issue: { assignee_ids: ids } })}
        />
      </SidebarRow>
      <SidebarRow label="Priority">
        <PriorityPicker
          value={issue.priority as PriorityLevel}
          disabled={disabled}
          variant={ROW_VARIANT}
          onChange={(level) => update.mutate({ issue: { priority: level } })}
        />
      </SidebarRow>
      <SidebarRow label="Due date">
        <div
          data-testid="intake-due-date"
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
            testId="intake-due-picker"
            onChange={(iso) => update.mutate({ issue: { target_date: iso } })}
          />
        </div>
      </SidebarRow>
      <SidebarRow label="Labels">
        <div className="flex min-w-0 flex-col gap-(--space-2)">
          <LabelChips
            labels={labels.data ?? []}
            selectedIds={issue.label_ids}
            onRemove={
              disabled
                ? undefined
                : (labelId) => update.mutate({ issue: { label_ids: issue.label_ids.filter((id) => id !== labelId) } })
            }
          />
          {disabled ? null : (
            <LabelPicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={issue.label_ids}
              onChange={(ids) => update.mutate({ issue: { label_ids: ids } })}
            />
          )}
        </div>
      </SidebarRow>
    </div>
  );
}
