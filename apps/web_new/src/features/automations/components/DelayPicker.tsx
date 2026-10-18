// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delay picker shared by both automation rows
// (parity/specs/agents.md §8, AGT-034/036). The trigger reads the
// current whole-month delay; the open list offers the fixed presets,
// each a listbox option, with a custom entry that opens the month
// dialog. Selecting a preset reports the count; the custom entry only
// opens the dialog, which reports through the same callback.

import * as React from "react";
import { ChevronDown } from "lucide-react";

import { DELAY_PRESETS, delayLabel } from "../api/months.js";

export const CUSTOMIZE_DELAY_LABEL = "Customize time range";

export interface DelayPickerProps {
  /** Current delay in months; also the trigger's label. */
  value: number;
  /** Accessible name for the trigger and its list. */
  label: string;
  disabled?: boolean;
  onSelect: (months: number) => void;
  onCustomize: () => void;
}

export function DelayPicker({
  value,
  label,
  disabled = false,
  onSelect,
  onCustomize,
}: DelayPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const listId = React.useId();
  const rootRef = React.useRef<HTMLDivElement>(null);

  // A click outside the picker or Escape closes it without choosing.
  React.useEffect(() => {
    if (!open) return;
    function onPointerDown(event: MouseEvent): void {
      if (rootRef.current !== null && !rootRef.current.contains(event.target as Node)) setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  function choose(months: number): void {
    setOpen(false);
    onSelect(months);
  }

  function customize(): void {
    setOpen(false);
    onCustomize();
  }

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-label={`${label}, ${delayLabel(value)}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        disabled={disabled}
        onClick={() => setOpen((previous) => !previous)}
        className="text-body flex h-(--control-height) w-full cursor-pointer items-center justify-between gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
      >
        <span>{delayLabel(value)}</span>
        <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      </button>
      {open ? (
        <ul
          id={listId}
          role="listbox"
          aria-label={label}
          className="shadow-popover absolute z-10 mt-(--space-1) w-full rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
        >
          {DELAY_PRESETS.map((months) => (
            <li key={months}>
              <button
                type="button"
                role="option"
                aria-selected={months === value}
                onClick={() => choose(months)}
                className="text-body flex w-full cursor-pointer items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
              >
                {delayLabel(months)}
              </button>
            </li>
          ))}
          <li>
            <button
              type="button"
              onClick={customize}
              className="text-body flex w-full cursor-pointer items-center rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text-muted) hover:bg-(--subtle)"
            >
              {CUSTOMIZE_DELAY_LABEL}
            </button>
          </li>
        </ul>
      ) : null}
    </div>
  );
}
