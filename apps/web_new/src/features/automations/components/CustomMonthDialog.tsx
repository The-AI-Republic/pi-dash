// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom month dialog behind either delay picker
// (parity/specs/agents.md §8, AGT-036). Written from the spec and the
// custom-month oracle: the field seeds at one month every time it opens,
// submit refuses empty fields and out-of-range counts with one inline
// message, and a valid month forwards to the row and closes the dialog.
// The form turns off native constraint validation so zero and
// over-range counts reach the handler and show the message instead of a
// browser tooltip. The input keeps its min/max/step attributes for
// assistive tech, and a fractional count is refused silently, the way
// the number field's own step validation would have swallowed the
// submit.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import {
  DELAY_RANGE_MESSAGE,
  MAX_DELAY_MONTHS,
  MIN_DELAY_MONTHS,
  parseDelayInput,
  rejectsFractionOnly,
} from "../api/months.js";

export const CUSTOM_MONTH_TITLE = "Customize time range";
export const CUSTOM_MONTH_SEED = String(MIN_DELAY_MONTHS);

export interface CustomMonthDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Fired with a validated whole-month count. */
  onSubmit: (months: number) => void;
}

export function CustomMonthDialog({ open, onOpenChange, onSubmit }: CustomMonthDialogProps): React.ReactElement {
  const [raw, setRaw] = React.useState(CUSTOM_MONTH_SEED);
  const [showRangeError, setShowRangeError] = React.useState(false);

  // Every open starts clean: the seed value, no error left over from a
  // cancelled attempt.
  React.useEffect(() => {
    if (!open) return;
    setRaw(CUSTOM_MONTH_SEED);
    setShowRangeError(false);
  }, [open]);

  function submit(event: React.FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    const months = parseDelayInput(raw);
    if (months === null) {
      // A fractional count never submits and never explains itself; only
      // empty or out-of-range whole counts raise the range message.
      if (rejectsFractionOnly(raw)) return;
      setShowRangeError(true);
      return;
    }
    onSubmit(months);
    onOpenChange(false);
  }

  function cancel(): void {
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={(next) => onOpenChange(next)} title={CUSTOM_MONTH_TITLE}>
      <form onSubmit={submit} noValidate>
        <Input
          label="Months"
          type="number"
          inputMode="numeric"
          min={MIN_DELAY_MONTHS}
          max={MAX_DELAY_MONTHS}
          step={1}
          placeholder="Enter Months"
          value={raw}
          onChange={(event) => {
            setRaw(event.target.value);
            if (showRangeError) setShowRangeError(false);
          }}
          {...(showRangeError ? { error: DELAY_RANGE_MESSAGE } : {})}
        />
        <div className="mt-(--space-8) flex justify-end gap-(--space-2)">
          <Button variant="secondary" size="large" onClick={cancel}>
            Cancel
          </Button>
          <Button variant="primary" size="large" type="submit">
            Submit
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
