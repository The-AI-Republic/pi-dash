// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module inline date-range control (parity/specs/modules.md §4.1,
// MOD-013). The trigger reads the saved range as two day labels, or the
// pinned start/end placeholders while a side is empty; the shared range
// calendar opens from it. Every pick saves both sides at once (an empty
// side saves an empty date): a lone start extends or re-anchors, a click
// on the lone selected day clears both, and a complete range re-anchors
// to the clicked day. There is no per-side clear on this surface. Guests
// and archived rows get the same control disabled.

import type { ModuleRow } from "@pidash/api-client";
import { ArrowRight, CalendarDays } from "lucide-react";
import * as React from "react";

import { DateRangePicker, useWeekStart } from "../../../issues/index.js";
import { moduleDateText, moduleRangeClick } from "../../lib/moduleDates.js";
import { useModuleInlineSave } from "../../lib/useModuleInlineSave.js";

export interface ModuleDateControlProps {
  workspaceSlug: string;
  projectId: string;
  module: Pick<ModuleRow, "id" | "name" | "start_date" | "target_date">;
  /** Guests and archived rows render the control disabled. */
  disabled: boolean;
  /** Test handle root: the trigger and calendar hang under it. */
  testId: string;
}

export function ModuleDateControl({
  workspaceSlug,
  projectId,
  module,
  disabled,
  testId,
}: ModuleDateControlProps): React.ReactElement {
  const save = useModuleInlineSave({ workspaceSlug, projectId });
  const weekStart = useWeekStart();
  const startLabel = moduleDateText(module.start_date, "Start date");
  const endLabel = moduleDateText(module.target_date, "End date");
  const empty = module.start_date === null && module.target_date === null;

  return (
    <DateRangePicker
      mode="split"
      label={`Module ${module.name} dates`}
      value={{ start: module.start_date, end: module.target_date }}
      onDayClick={(day) => {
        const next = moduleRangeClick({ start: module.start_date, end: module.target_date }, day);
        save.saveDates(module.id, { startDate: next.start, targetDate: next.end });
      }}
      weekStart={weekStart}
      disabled={disabled}
      testId={testId}
      trigger={
        <button
          type="button"
          aria-label={`Module ${module.name} dates`}
          disabled={disabled}
          data-testid={`${testId}-trigger`}
          className={`text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:hover:bg-transparent ${
            empty ? "text-(--text-muted)" : "text-(--text)"
          }`}
        >
          <CalendarDays size={14} strokeWidth={2} aria-hidden="true" className="shrink-0" />
          <span>{startLabel}</span> <ArrowRight size={14} strokeWidth={2} aria-hidden="true" className="shrink-0" />{" "}
          <span>{endLabel}</span>
        </button>
      }
    />
  );
}
