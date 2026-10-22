// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Selection controls (parity/specs/issues.md §10, ISS-108). A row control
// toggles one item; a group control reports the tri-state and toggles the
// whole bucket. Both are native checkboxes so keyboard and assistive
// technology behavior comes from the platform. The group control stops
// its click from reaching the collapsible header underneath. Rows that
// belong to another project show the same footprint, disabled, with the
// same-project hint a reader can hover or focus.

import { cn, Tooltip } from "@pidash/kit";
import * as React from "react";

import type { GroupSelectionState } from "./model.js";

export interface RowSelectionCheckboxProps {
  checked: boolean;
  label: string;
  onChange: (event: React.MouseEvent<HTMLInputElement>) => void;
  className?: string | undefined;
}

export function RowSelectionCheckbox({
  checked,
  label,
  onChange,
  className,
}: RowSelectionCheckboxProps): React.ReactElement {
  return (
    <input
      type="checkbox"
      aria-label={label}
      checked={checked}
      onClick={(event) => {
        event.stopPropagation();
        onChange(event);
      }}
      onChange={() => {
        // The click handler owns the toggle so shift-click can extend a
        // range; React still wants an onChange for a controlled input.
      }}
      className={cn("size-4 shrink-0 cursor-pointer accent-(--accent)", className)}
    />
  );
}

export interface GroupSelectionCheckboxProps {
  state: GroupSelectionState;
  label: string;
  onToggle: () => void;
}

export function GroupSelectionCheckbox({ state, label, onToggle }: GroupSelectionCheckboxProps): React.ReactElement {
  const ref = React.useRef<HTMLInputElement | null>(null);
  React.useEffect(() => {
    if (ref.current !== null) ref.current.indeterminate = state === "partial";
  }, [state]);
  return (
    <input
      ref={ref}
      type="checkbox"
      aria-label={label}
      checked={state === "complete"}
      onClick={(event) => event.stopPropagation()}
      onChange={onToggle}
      className="size-4 shrink-0 cursor-pointer accent-(--accent)"
    />
  );
}

export interface ForeignRowSelectionHintProps {
  label: string;
}

/** The disabled footprint for a row owned by another project. */
export function ForeignRowSelectionHint({ label }: ForeignRowSelectionHintProps): React.ReactElement {
  return (
    <Tooltip content="Selection works within this project; cross-project items are excluded.">
      <span
        aria-label={label}
        role="img"
        tabIndex={0}
        className="inline-flex size-4 shrink-0 cursor-not-allowed items-center justify-center rounded-(--radius-control) border border-(--border) opacity-50"
      >
        <span className="size-1.5 rounded-full bg-(--text-muted)" aria-hidden="true" />
      </span>
    </Tooltip>
  );
}
