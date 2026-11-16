// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cancelled-state picker for the auto-close row
// (parity/specs/agents.md §8, AGT-035). Lists exactly the cancelled-group
// states; it opens only once a second one exists, and with none it shows
// the "State" placeholder while staying closed. The trigger is a
// button named by its label so the row reads as text in parity probes.

import type { State } from "@pidash/api-client";
import { cn } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

import { isStatePickerEnabled } from "../lib/rows.js";

export const STATE_FALLBACK_LABEL = "State";
export const STATE_PICKER_LIST_LABEL = "Cancelled states";

export interface CloseTargetPickerProps {
  /** The cancelled states the picker offers. */
  states: readonly State[];
  /** The resolved current target, or null when none exists. */
  selected: State | null;
  onSelect: (stateId: string) => void;
}

export function CloseTargetPicker({ states, selected, onSelect }: CloseTargetPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const listId = React.useId();
  const rootRef = React.useRef<HTMLDivElement>(null);
  const disabled = !isStatePickerEnabled(states);

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

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        disabled={disabled}
        onClick={() => setOpen((previous) => !previous)}
        className="text-body flex h-(--control-height) w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) disabled:cursor-not-allowed disabled:opacity-60"
      >
        <span
          aria-hidden="true"
          className="size-2.5 shrink-0 rounded-full"
          style={{ backgroundColor: selected?.color ?? "var(--text-muted)" }}
        />
        <span className={cn(selected === null && "text-(--text-muted)")}>{selected?.name ?? STATE_FALLBACK_LABEL}</span>
        <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className="ml-auto shrink-0 text-(--text-muted)" />
      </button>
      {open ? (
        <ul
          id={listId}
          role="listbox"
          aria-label={STATE_PICKER_LIST_LABEL}
          className="shadow-popover absolute z-10 mt-(--space-1) w-full rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
        >
          {states.map((state) => (
            <li key={state.id}>
              <button
                type="button"
                role="option"
                aria-selected={state.id === selected?.id}
                onClick={() => {
                  setOpen(false);
                  onSelect(state.id);
                }}
                className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
              >
                <span
                  aria-hidden="true"
                  className="size-2.5 shrink-0 rounded-full"
                  style={{ backgroundColor: state.color }}
                />
                {state.name}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
