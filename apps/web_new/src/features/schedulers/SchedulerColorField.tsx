// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scheduler color picker: one-click preset swatches with a "More" overflow
// that opens the native custom-color picker. Shared by the workspace
// catalog dialog and the project install dialog's author path.

import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { DEFAULT_SCHEDULER_COLOR, isSchedulerSwatchColor, SCHEDULER_COLOR_SWATCHES } from "./definition.js";

const FOCUS_RING =
  "focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-(--focus-ring-color) focus-visible:outline-offset-1";

const HEX_COLOR = /^#[0-9a-fA-F]{6}$/;

export interface SchedulerColorFieldProps {
  value: string;
  disabled?: boolean;
  error?: string | undefined;
  onChange: (color: string) => void;
}

export function SchedulerColorField({
  value,
  disabled = false,
  error,
  onChange,
}: SchedulerColorFieldProps): React.ReactElement {
  const pickerRef = React.useRef<HTMLInputElement>(null);
  const hintId = React.useId();
  const errorId = React.useId();
  // A stored color outside the presets (picked through "More", or saved
  // before presets existed) leaves every swatch unselected and shows on
  // the overflow button instead.
  const custom = !isSchedulerSwatchColor(value);
  // The native picker only takes six-digit hex; anything else opens it at
  // the default instead of rendering an invalid control value.
  const pickerValue = HEX_COLOR.test(value) ? value : DEFAULT_SCHEDULER_COLOR;
  const folded = value.toLowerCase();

  return (
    <div className="flex flex-col gap-(--space-2)">
      <span className="text-body font-medium text-(--text)">Color</span>
      <div
        role="group"
        aria-label="Color"
        aria-describedby={error ? errorId : hintId}
        className="flex flex-wrap items-center gap-(--space-2)"
      >
        {SCHEDULER_COLOR_SWATCHES.map((swatch) => {
          const selected = swatch.toLowerCase() === folded;
          return (
            <button
              key={swatch}
              type="button"
              aria-label={`Scheduler color ${swatch}`}
              aria-pressed={selected}
              disabled={disabled}
              onClick={() => onChange(swatch)}
              style={{ backgroundColor: swatch }}
              className={`h-(--space-7) w-(--space-7) rounded-(--radius-control) border border-(--border) disabled:cursor-not-allowed disabled:opacity-50 ${FOCUS_RING} ${
                selected ? "outline-2 outline-offset-1 outline-(--accent) outline-solid" : ""
              }`}
            />
          );
        })}
        <Tooltip content="More">
          <button
            type="button"
            aria-label="More colors"
            aria-pressed={custom}
            disabled={disabled}
            onClick={() => pickerRef.current?.click()}
            className={`text-body flex h-(--space-7) min-w-(--space-7) items-center justify-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-1) text-(--text-muted) disabled:cursor-not-allowed disabled:opacity-50 ${FOCUS_RING}`}
          >
            {custom ? (
              <span
                aria-hidden="true"
                style={{ backgroundColor: pickerValue }}
                className="h-(--space-3) w-(--space-3) rounded-full"
              />
            ) : null}
            <span aria-hidden="true">…</span>
          </button>
        </Tooltip>
        <input
          ref={pickerRef}
          type="color"
          aria-label="Custom color"
          tabIndex={-1}
          className="sr-only"
          disabled={disabled}
          value={pickerValue}
          onChange={(event) => onChange(event.target.value)}
        />
      </div>
      {error ? (
        <p id={errorId} role="alert" className="text-meta text-(--danger)">
          {error}
        </p>
      ) : (
        <p id={hintId} className="text-meta text-(--text-muted)">
          Marks this scheduler&apos;s blocks on the project calendar.
        </p>
      )}
    </div>
  );
}
