// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Event selection control shared by the create dialog and the detail form
// (parity/specs/workspace-settings.md §19/§21, WSET-020/022). Two choices
// — everything, or individual events — and, only in the individual mode,
// the five delivery toggles. Native radios and checkboxes keep the group
// keyboard-navigable without extra key bindings (§37).

import * as React from "react";

import {
  selectMode,
  toggleEvent,
  WEBHOOK_EVENT_OPTIONS,
  type WebhookEventFlags,
  type WebhookEventMode,
} from "./events.js";

export const EVENT_MODE_LABELS: Record<WebhookEventMode, string> = {
  all: "Send me everything",
  individual: "Select individual events",
};

export interface WebhookEventSelection {
  mode: WebhookEventMode;
  flags: WebhookEventFlags;
}

export interface EventPickerProps {
  value: WebhookEventSelection;
  onChange: (next: WebhookEventSelection) => void;
}

export function EventPicker({ value, onChange }: EventPickerProps): React.ReactElement {
  return (
    <fieldset className="flex flex-col gap-(--space-4)">
      <legend className="text-body font-medium text-(--text)">Events</legend>
      <div className="flex flex-col gap-(--space-2)" role="radiogroup" aria-label="Event selection">
        {(["all", "individual"] as const).map((mode) => (
          <label
            key={mode}
            htmlFor={mode}
            className="text-body flex cursor-pointer items-center gap-(--space-3) text-(--text)"
          >
            <input
              type="radio"
              id={mode}
              name="event-mode"
              value={mode}
              checked={value.mode === mode}
              onChange={() => onChange(selectMode(value, mode))}
              className="size-4 accent-(--accent)"
            />
            {EVENT_MODE_LABELS[mode]}
          </label>
        ))}
      </div>
      {value.mode === "individual" ? (
        <div className="flex flex-col gap-(--space-3) border-l border-(--border) pl-(--space-4)">
          {WEBHOOK_EVENT_OPTIONS.map((option) => (
            <div key={option.flag} className="flex items-start gap-(--space-3)">
              <input
                type="checkbox"
                id={option.flag}
                name={option.flag}
                checked={value.flags[option.flag]}
                onChange={() => onChange({ ...value, flags: toggleEvent(value.flags, option.flag) })}
                className="mt-1 size-4 accent-(--accent)"
              />
              <div className="flex flex-col">
                <label htmlFor={option.flag} className="text-body cursor-pointer text-(--text)">
                  {option.label}
                </label>
                <span className="text-meta text-(--text-muted)">{option.description}</span>
              </div>
            </div>
          ))}
        </div>
      ) : null}
    </fieldset>
  );
}
