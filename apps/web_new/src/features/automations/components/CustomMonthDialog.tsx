// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom month dialog behind either delay picker
// (parity/specs/project-settings-part-f.md, PSET-053/054; AGT-036).
// The field seeds at one month every open and submits a whole month to
// the row, then closes. The number input carries required bounds, so a
// zero, an over-range or a fractional count never reaches the handler:
// native validation keeps the dialog open without any app text. Only an
// empty field gets the range message, which clears as soon as the user
// types.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { DELAY_RANGE_MESSAGE, MAX_DELAY_MONTHS, MIN_DELAY_MONTHS, parseDelayInput } from "../api/months.js";

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
    // Native validation normally stops invalid values before submit; the
    // guard covers a programmatic submit and keeps the outcome the same.
    if (!event.currentTarget.checkValidity()) return;
    const months = parseDelayInput(raw);
    if (months === null) {
      // Everything but an empty field was refused above; the empty case
      // is the one the app explains.
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
      <form onSubmit={submit}>
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
