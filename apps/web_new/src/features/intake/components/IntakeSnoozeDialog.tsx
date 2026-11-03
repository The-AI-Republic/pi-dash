// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snooze dialog (parity/specs/intake.md INT-022). A single-date
// calendar whose past days are disabled; confirming snoozes the
// request until the picked day's midnight. The calendar starts on the
// request's current snooze day when there is one, otherwise on today.
// The caller owns the write and closes on settle.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { DateCalendar, todayIsoDay, useWeekStart } from "../../issues/index.js";

/** Snooze dialog heading. */
export const SNOOZE_DIALOG_TITLE = "Snooze request";

export interface IntakeSnoozeDialogProps {
  open: boolean;
  /** The snooze write is in flight; controls lock. */
  pending: boolean;
  /** The request's current snooze day, if it has one. */
  initialDay: string | null;
  onCancel: () => void;
  onConfirm: (isoDay: string) => void;
}

export function IntakeSnoozeDialog({
  open,
  pending,
  initialDay,
  onCancel,
  onConfirm,
}: IntakeSnoozeDialogProps): React.ReactElement {
  const weekStart = useWeekStart();
  const [picked, setPicked] = React.useState<string | null>(initialDay);
  React.useEffect(() => {
    if (open) setPicked(initialDay);
  }, [open, initialDay]);
  const today = todayIsoDay();

  return (
    <Dialog
      title={SNOOZE_DIALOG_TITLE}
      description="Pick the day the request returns to triage."
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      actions={
        <>
          <Button variant="secondary" onClick={onCancel} disabled={pending}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              if (picked !== null) onConfirm(picked);
            }}
            disabled={pending || picked === null}
            loading={pending}
          >
            Snooze
          </Button>
        </>
      }
    >
      <DateCalendar
        value={picked}
        onSelect={setPicked}
        min={today}
        weekStart={weekStart}
        testId="intake-snooze-calendar"
      />
    </Dialog>
  );
}
