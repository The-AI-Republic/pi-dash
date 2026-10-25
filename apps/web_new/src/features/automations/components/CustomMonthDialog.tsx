// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom month dialog behind either delay picker
// (parity/specs/project-settings-part-f.md, PSET-053/054; AGT-036).
// The field seeds at one month every open and submits a whole month to
// the row, then closes. Empty and whole-number out-of-range values get
// the inline range message; a fractional count stays silent because the
// handler refuses it. The message clears as soon as the user types.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import {
  DELAY_RANGE_MESSAGE,
  MAX_DELAY_MONTHS,
  MIN_DELAY_MONTHS,
  isMonthRangeError,
  parseDelayInput,
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
    if (isMonthRangeError(raw)) {
      setShowRangeError(true);
      return;
    }
    const months = parseDelayInput(raw);
    if (months === null) {
      // Fractional counts are refused here; keep the dialog open without
      // adding a range message.
      return;
    }
    onSubmit(months);
    onOpenChange(false);
  }

  function cancel(): void {
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={CUSTOM_MONTH_TITLE}>
      <form noValidate onSubmit={submit}>
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
