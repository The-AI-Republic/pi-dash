// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom creation-date picker (SHELL-029). Before, after or a
// day-granular range; every pick replaces the prior date filter.
// Written from parity/specs/shell.md §3.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

export type CustomDateMode = "before" | "after" | "range";

export interface ProjectsCustomDateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onApply: (values: string[]) => void;
}

function toInputDay(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

function defaultDays(): { start: string; end: string } {
  const today = new Date();
  const nextMonth = new Date(today.getFullYear(), today.getMonth() + 1, today.getDate());
  return { start: toInputDay(today), end: toInputDay(nextMonth) };
}

const MODES: ReadonlyArray<{ value: CustomDateMode; label: string }> = [
  { value: "before", label: "Before" },
  { value: "after", label: "After" },
  { value: "range", label: "Range" },
];

export function ProjectsCustomDateDialog({
  open,
  onOpenChange,
  onApply,
}: ProjectsCustomDateDialogProps): React.ReactElement {
  const [mode, setMode] = React.useState<CustomDateMode>("range");
  const [start, setStart] = React.useState(() => defaultDays().start);
  const [end, setEnd] = React.useState(() => defaultDays().end);

  React.useEffect(() => {
    if (!open) return;
    const defaults = defaultDays();
    setMode("range");
    setStart(defaults.start);
    setEnd(defaults.end);
  }, [open]);

  const ranged = mode === "range";
  const rangeBroken = ranged && (end === "" || start > end);
  const valid = start !== "" && !rangeBroken;

  const apply = (): void => {
    if (mode === "range") onApply([`${start};after`, `${end};before`]);
    else if (mode === "after") onApply([`${start};after`]);
    else onApply([`${start};before`]);
    onOpenChange(false);
  };

  return (
    <Dialog
      title="Created date"
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" size="small" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" size="small" disabled={!valid} onClick={apply}>
            Apply
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-6)">
        <div role="radiogroup" aria-label="Date match" className="flex gap-(--space-4)">
          {MODES.map((entry) => (
            <label key={entry.value} className="text-body flex cursor-pointer items-center gap-(--space-2)">
              <input
                type="radio"
                name="custom-date-mode"
                value={entry.value}
                checked={mode === entry.value}
                onChange={() => setMode(entry.value)}
              />
              {entry.label}
            </label>
          ))}
        </div>
        <Input
          label={ranged ? "Start date" : "Date"}
          type="date"
          value={start}
          onChange={(event) => setStart(event.target.value)}
        />
        {ranged ? (
          <Input label="End date" type="date" value={end} onChange={(event) => setEnd(event.target.value)} />
        ) : null}
        {rangeBroken ? (
          <p role="alert" className="text-body text-(--danger)">
            The end date must be on or after the start date.
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
