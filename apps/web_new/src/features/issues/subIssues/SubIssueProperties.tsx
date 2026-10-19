// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline property controls on a sub-issue row (parity/specs/issues.md
// §15 + §17, ISS-177). State, priority, assignees and dates each own one
// control and each writes exactly one field; the date range appears only
// when both ends are set and both date columns are shown. Every control
// stops its click from reaching the row link. A member edits; a guest
// reads the same values through disabled triggers.

import { Button } from "@pidash/kit";
import type { SubIssueItem } from "@pidash/api-client";
import { Flag, Minus } from "lucide-react";
import * as React from "react";

import { DatePicker } from "../dropdowns/DatePicker.js";
import { DateRangePicker } from "../dropdowns/DateRangePicker.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
import {
  MemberPicker,
  PriorityPicker,
  PRIORITY_ICONS,
  PRIORITY_TONES,
  StatePicker,
  type PriorityLevel,
} from "../../../shared/pickers/index.js";
import { isPropertyVisible, type SubIssueDisplayProperties } from "./rules.js";
import type { SubIssueRowActions } from "./types.js";

export interface SubIssuePropertyControlsProps {
  workspaceSlug: string;
  projectId: string;
  row: SubIssueItem;
  properties: SubIssueDisplayProperties;
  editable: boolean;
  merged: boolean;
  overdue: boolean;
  actions: SubIssueRowActions;
}

/** A priority glyph with its tone, reused as the row trigger. */
function PriorityTrigger({ level }: { level: PriorityLevel }): React.ReactElement {
  const Icon = PRIORITY_ICONS[level];
  return (
    <Button variant="ghost" size="small" aria-label={`Priority: ${level}`}>
      <span aria-hidden="true" className={`inline-flex items-center ${PRIORITY_TONES[level]}`}>
        {level === "none" ? (
          <Minus size={14} strokeWidth={1.5} />
        ) : level === "urgent" ? (
          <span className="rounded-full border border-current p-px">
            <Flag size={10} strokeWidth={2} />
          </span>
        ) : (
          <Icon size={14} strokeWidth={2} />
        )}
      </span>
    </Button>
  );
}

/** Every visible property control on one row, in the fixed column order. */
export function SubIssuePropertyControls({
  workspaceSlug,
  projectId,
  row,
  properties,
  editable,
  merged,
  overdue,
  actions,
}: SubIssuePropertyControlsProps): React.ReactElement {
  const weekStart = useWeekStart();
  const stop = (event: React.SyntheticEvent): void => {
    event.stopPropagation();
  };
  const disabled = !editable;
  return (
    <div className="flex items-center gap-(--space-1)" onClick={stop}>
      {isPropertyVisible(properties, "state") ? (
        <StatePicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          value={row.state_id}
          iconOnly
          variant="ghost"
          disabled={disabled}
          placeholder="State"
          onChange={(stateId) => actions.write({ row, patch: { state_id: stateId }, field: "state_id" })}
        />
      ) : null}
      {isPropertyVisible(properties, "priority") ? (
        <PriorityPicker
          value={row.priority}
          variant="ghost"
          iconOnly
          disabled={disabled}
          trigger={<PriorityTrigger level={row.priority} />}
          onChange={(priority) => actions.write({ row, patch: { priority }, field: "priority" })}
        />
      ) : null}
      {merged ? (
        <DateRangePicker
          label="Date range"
          mode="merged"
          value={{ start: row.start_date ?? null, end: row.target_date ?? null }}
          weekStart={weekStart}
          disabled={disabled}
          clearable
          onChange={(range) => {
            // Both ends travel in one PATCH so the pair never half-writes.
            actions.write({
              row,
              patch: { start_date: range.start, target_date: range.end },
              field: "start_date",
            });
          }}
          testId="sub-issue-date-range"
        />
      ) : (
        <>
          {isPropertyVisible(properties, "start_date") ? (
            <DatePicker
              label="Start date"
              value={row.start_date ?? null}
              max={row.target_date ?? null}
              weekStart={weekStart}
              disabled={disabled}
              onChange={(startDate) => actions.write({ row, patch: { start_date: startDate }, field: "start_date" })}
              testId="sub-issue-start-date"
            />
          ) : null}
          {isPropertyVisible(properties, "due_date") ? (
            <div className={overdue ? "text-(--danger)" : undefined}>
              <DatePicker
                label="Due date"
                value={row.target_date ?? null}
                min={row.start_date ?? null}
                weekStart={weekStart}
                disabled={disabled}
                onChange={(targetDate) =>
                  actions.write({ row, patch: { target_date: targetDate }, field: "target_date" })
                }
                testId="sub-issue-due-date"
              />
            </div>
          ) : null}
        </>
      )}
      {isPropertyVisible(properties, "assignee") ? (
        <MemberPicker
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          multiple
          value={row.assignee_ids}
          iconOnly
          variant="ghost"
          disabled={disabled}
          onChange={(assigneeIds) =>
            actions.write({ row, patch: { assignee_ids: assigneeIds }, field: "assignee_ids" })
          }
        />
      ) : null}
    </div>
  );
}
