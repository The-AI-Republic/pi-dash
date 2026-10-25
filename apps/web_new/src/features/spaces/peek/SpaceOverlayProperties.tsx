// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The public overlay's read-only property panel (SPC-044). Exactly three
// fields — state, priority and due date — each with its marker and its
// text label, so colour is never the only carrier. A missing due date
// prints the explicit empty marker; a late date on an open issue reads
// as overdue. Nothing here is editable.

import type { SpaceBoardIssueDetail, SpaceBoardState } from "@pidash/api-client";
import * as React from "react";

import { PriorityMarker } from "../filters/PriorityMarker.js";
import { StateMarker } from "../layouts/StateMarker.js";
import { formatSpacePropertyDate, isSpaceDueOverdue } from "../lib/peek.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { PRIORITY_LABELS, type PriorityLevel } from "../../../shared/pickers/priorities.js";

export type SpaceOverlayPriority = NonNullable<SpaceBoardIssueDetail["priority"]>;

/** The priority a panel shows; an unset priority is the `none` level. */
export function spaceOverlayPriority(priority: SpaceBoardIssueDetail["priority"]): PriorityLevel {
  return priority ?? "none";
}

/** The workflow state the detail names, when the states fetch has it. */
export function spaceOverlayState(states: readonly SpaceBoardState[], stateId: string | null): SpaceBoardState | null {
  if (stateId === null) return null;
  return states.find((state) => state.id === stateId) ?? null;
}

/** Group of the issue's state; the detail's own fallback covers a gap. */
function stateGroupOf(issue: SpaceBoardIssueDetail, state: SpaceBoardState | null): string | null {
  return state?.group ?? issue.state__group;
}

export interface SpaceOverlayPropertiesProps {
  issue: SpaceBoardIssueDetail;
  states: readonly SpaceBoardState[];
}

export function SpaceOverlayProperties({ issue, states }: SpaceOverlayPropertiesProps): React.ReactElement {
  const state = spaceOverlayState(states, issue.state_id);
  const priority = spaceOverlayPriority(issue.priority);
  const dueDate = formatSpacePropertyDate(issue.target_date);
  const overdue = isSpaceDueOverdue(issue.target_date, stateGroupOf(issue, state));
  return (
    <dl data-testid="space-peek-properties" className="flex flex-col gap-(--space-2)">
      <div data-property="state" className="flex items-center gap-(--space-3)">
        <dt className="text-meta w-24 shrink-0 text-(--text-muted)">{spaceMessage("peek.property.state")}</dt>
        <dd className="text-body m-0 flex min-w-0 items-center gap-(--space-2) text-(--text)">
          {state === null ? null : <StateMarker state={state} />}
          <span className="truncate">{state?.name ?? spaceMessage("peek.empty")}</span>
        </dd>
      </div>
      <div data-property="priority" className="flex items-center gap-(--space-3)">
        <dt className="text-meta w-24 shrink-0 text-(--text-muted)">{spaceMessage("peek.property.priority")}</dt>
        <dd className="text-body m-0 flex min-w-0 items-center gap-(--space-2) text-(--text)">
          <PriorityMarker level={priority} />
          <span className="truncate">{PRIORITY_LABELS[priority]}</span>
        </dd>
      </div>
      <div data-property="due-date" className="flex items-center gap-(--space-3)">
        <dt className="text-meta w-24 shrink-0 text-(--text-muted)">{spaceMessage("peek.property.dueDate")}</dt>
        <dd className="text-body m-0 min-w-0">
          {dueDate === null ? (
            <span data-testid="space-peek-due-empty" className="text-(--text-muted)">
              {spaceMessage("peek.empty")}
            </span>
          ) : (
            <span
              data-testid="space-peek-due"
              data-overdue={overdue ? "true" : undefined}
              className={overdue ? "font-medium text-(--danger)" : "text-(--text)"}
            >
              {dueDate}
            </span>
          )}
        </dd>
      </div>
    </dl>
  );
}
