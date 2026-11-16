// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card snooze menu (written from parity/specs/notifications.md §9,
// NTF-020). Five fixed delays measured from the moment of snoozing, a
// custom-resume entry that hands off to the dialog, and — only for a card
// that already carries a snooze — a remove entry. Menu rows are the only
// way in: no key or drag gesture reaches them (NTF-029).
import { Popover } from "@pidash/kit";
import { Clock, X } from "lucide-react";
import * as React from "react";

import { SNOOZE_PRESETS, type SnoozePreset } from "../lib/snooze.js";

export const CUSTOM_SNOOZE_ENTRY = "Custom date and time";
export const REMOVE_SNOOZE_ENTRY = "Remove snooze";

export interface SnoozeMenuProps {
  /** True when the card already carries a snooze. */
  snoozed: boolean;
  /** Snooze writes in flight (menu rows stay inert while true). */
  busy: boolean;
  onPreset: (preset: SnoozePreset) => void;
  onCustom: () => void;
  onRemove: () => void;
}

/**
 * The snooze trigger sits in the card's action row; the panel opens on
 * pointer activation and closes after a choice. A preset starts its write
 * immediately; the custom entry closes the menu and opens the dialog.
 */
export function SnoozeMenu({ snoozed, busy, onPreset, onCustom, onRemove }: SnoozeMenuProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const rowClass =
    "text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-50";
  const pick = (action: () => void): void => {
    setOpen(false);
    action();
  };
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="w-56"
      trigger={
        <button
          type="button"
          aria-label="Snooze notification"
          aria-haspopup="menu"
          data-testid="snooze-trigger"
          disabled={busy}
          onClick={() => setOpen((current) => !current)}
          className="inline-flex size-6 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Clock size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      }
    >
      <div data-testid="snooze-panel" className="flex flex-col gap-(--space-1)">
        {SNOOZE_PRESETS.map((preset) => (
          <button
            key={preset.id}
            type="button"
            data-testid={`snooze-option-${preset.id}`}
            disabled={busy}
            onClick={() => pick(() => onPreset(preset))}
            className={rowClass}
          >
            {preset.label}
          </button>
        ))}
        <button
          type="button"
          data-testid="snooze-option-custom"
          disabled={busy}
          onClick={() => pick(onCustom)}
          className={rowClass}
        >
          {CUSTOM_SNOOZE_ENTRY}
        </button>
        {snoozed ? (
          <button
            type="button"
            data-testid="snooze-option-unsnooze"
            disabled={busy}
            onClick={() => pick(onRemove)}
            className={rowClass}
          >
            <X size={14} strokeWidth={2} aria-hidden="true" />
            {REMOVE_SNOOZE_ENTRY}
          </button>
        ) : null}
      </div>
    </Popover>
  );
}
