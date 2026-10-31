// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar inline date editing (parity/specs/cycles.md §14, CYC-036).
// Members and admins move the cycle's start and end here; guests and
// finished cycles read the same span disabled. Every completed pick
// saves through the dialog's own overlap gate (needsCycleDateCheck):
// a collision refuses with the same draft guidance, and the rejection
// renders both as a toast and inline beside the control, so a second
// save landing while a toast is still up can never swallow it
// (NEWFRONT-264 intended). Clearing both dates saves without a check.

import { useQueryClient } from "@tanstack/react-query";
import { checkCycleDates, updateCycle, type CycleRow } from "@pidash/api-client";
import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { DateRangePicker, todayIsoDay, type RangeValue } from "../../issues/index.js";
import { invalidateAfterCycleWrite } from "../api/invalidate.js";
import {
  CYCLE_OVERLAP_TITLE,
  cycleDaysComplete,
  cycleFailureMessage,
  cycleFormDays,
  cycleOverlapMessage,
  needsCycleDateCheck,
} from "../dialog/cycleRange.js";
import { cycleSpanModel, instantToProjectDate } from "../lib/cycleDates.js";

export interface CycleSidebarDatesProps {
  workspaceSlug: string;
  projectId: string;
  cycle: CycleRow;
  /** IANA name of the project's timezone. */
  projectTimeZone: string;
  /** Viewer zone override; defaults to the browser zone. Tests inject. */
  viewerTimeZone?: string | undefined;
  /** Member/admin on a live cycle; everyone else reads the span disabled. */
  editable: boolean;
}

/** Success confirmation behind every inline date save (spec §7). */
export const CYCLE_DATES_UPDATED_TITLE = "Cycle updated successfully.";

/** Trigger text while the cycle carries no dates. */
export const CYCLE_DATES_EMPTY_LABEL = "Start date";

function storedDays(cycle: CycleRow, projectTimeZone: string): RangeValue {
  return {
    start: cycle.start_date === null ? null : instantToProjectDate(cycle.start_date, projectTimeZone),
    end: cycle.end_date === null ? null : instantToProjectDate(cycle.end_date, projectTimeZone),
  };
}

export function CycleSidebarDates({
  workspaceSlug,
  projectId,
  cycle,
  projectTimeZone,
  viewerTimeZone,
  editable,
}: CycleSidebarDatesProps): React.ReactElement {
  const queryClient = useQueryClient();
  const stored = storedDays(cycle, projectTimeZone);
  const [draft, setDraft] = React.useState<RangeValue>(stored);
  const [error, setError] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);
  // Serializes rapid picks: each completed click saves in click order
  // instead of racing the previous write.
  const chain = React.useRef(Promise.resolve());

  // The stored range is the picker's anchor: a settled save re-bases
  // the next click onto the server state.
  React.useEffect(() => {
    setDraft(storedDays(cycle, projectTimeZone));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- re-base on the stored dates only.
  }, [cycle.start_date, cycle.end_date, projectTimeZone]);

  /** Revert the draft onto the stored range after a refused save. */
  function revert(): void {
    setDraft(storedDays(cycle, projectTimeZone));
  }

  async function saveRange(next: RangeValue): Promise<void> {
    const days = cycleFormDays(next);
    // Clicking the settled end re-fires the range; nothing changed, so
    // nothing saves.
    if (stored.start === days.start_date && stored.end === days.end_date) return;
    const cleared = days.start_date === null && days.end_date === null;
    // A pending one-sided pick waits for its closing click; only
    // complete or fully cleared ranges save.
    if (!cleared && !cycleDaysComplete(days)) return;
    if (needsCycleDateCheck({ start_date: cycle.start_date, end_date: cycle.end_date }, next, projectTimeZone)) {
      let check;
      try {
        check = await checkCycleDates(getAppClient(), workspaceSlug, projectId, {
          start_date: next.start,
          end_date: next.end,
          cycle_id: cycle.id,
        });
      } catch (checkError) {
        setError(cycleFailureMessage(checkError, "update"));
        revert();
        return;
      }
      if (!check.status) {
        const message = cycleOverlapMessage("update");
        setError(message);
        notifyError(CYCLE_OVERLAP_TITLE, message);
        revert();
        return;
      }
    }
    setSaving(true);
    try {
      await updateCycle(getAppClient(), workspaceSlug, projectId, cycle.id, {
        start_date: days.start_date,
        end_date: days.end_date,
      });
    } catch (writeError) {
      setError(cycleFailureMessage(writeError, "update"));
      revert();
      return;
    } finally {
      setSaving(false);
    }
    setError(null);
    await invalidateAfterCycleWrite(queryClient, workspaceSlug, projectId, cycle.id);
    notifySuccess(CYCLE_DATES_UPDATED_TITLE);
  }

  function handleChange(next: RangeValue): void {
    setDraft(next);
    setError(null);
    const run = chain.current.then(() => saveRange(next));
    chain.current = run.catch(() => undefined);
  }

  const span = cycleSpanModel(cycle.start_date, cycle.end_date, projectTimeZone, viewerTimeZone);
  const disabled = !editable || saving;
  const row = (
    <span data-testid="cycle-sidebar-dates" className="inline-flex flex-wrap items-center gap-(--space-2)">
      <DateRangePicker
        value={draft}
        onChange={handleChange}
        mode="merged"
        min={todayIsoDay()}
        label="Cycle dates"
        testId="cycle-sidebar-dates-picker"
        clearable
        disabled={disabled}
        trigger={
          <button
            type="button"
            aria-label="Cycle dates"
            disabled={disabled}
            data-testid="cycle-sidebar-dates-trigger"
            className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:hover:bg-transparent"
          >
            <span>{span.text ?? CYCLE_DATES_EMPTY_LABEL}</span>
          </button>
        }
      />
      {span.chip === null ? null : (
        <span
          data-testid="cycle-date-offset"
          className="text-meta inline-flex items-center rounded-full border border-(--border) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
        >
          {span.chip}
        </span>
      )}
    </span>
  );
  return (
    <div className="flex flex-col items-start gap-(--space-1)">
      {span.tooltip === null ? row : <Tooltip content={span.tooltip}>{row}</Tooltip>}
      {error === null ? null : (
        <p role="alert" data-testid="cycle-sidebar-dates-error" className="text-meta text-(--danger)">
          {error}
        </p>
      )}
    </div>
  );
}
