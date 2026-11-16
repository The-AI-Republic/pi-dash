// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emoji picker for reactions (parity/specs/issues.md §16, ISS-203/204).
// A small fixed set of people-friendly emoji behind a trigger button;
// choosing one closes the panel and hands the character back. The picker
// stays a popover so it can sit inside a comment card or the issue row
// without shifting layout.

import { IconButton, Popover } from "@pidash/kit";
import { SmilePlus } from "lucide-react";
import * as React from "react";

import { REACTION_CHOICES } from "./model.js";

export const EMOJI_PICKER_LABEL = "Add reaction";
export const EMOJI_SEARCH_LABEL = "Search emoji";

export interface EmojiPickerProps {
  onPick: (emoji: string) => void;
  disabled?: boolean;
  /** Accessible name of the trigger; cards use the default. */
  label?: string;
}

export function EmojiPicker({
  onPick,
  disabled = false,
  label = EMOJI_PICKER_LABEL,
}: EmojiPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const choices =
    query.trim() === "" ? REACTION_CHOICES : REACTION_CHOICES.filter((emoji) => emoji.includes(query.trim()));

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setQuery("");
      }}
      className="w-64"
      trigger={
        <IconButton label={label} size="small" disabled={disabled} aria-haspopup="dialog" aria-expanded={open}>
          <SmilePlus size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
    >
      <div className="flex flex-col gap-(--space-2)">
        <input
          type="search"
          aria-label={EMOJI_SEARCH_LABEL}
          placeholder="Search"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
          className="text-meta w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text) outline-none focus-visible:border-(--accent)"
        />
        {choices.length === 0 ? (
          <p className="text-meta px-(--space-2) py-(--space-1) text-(--text-muted)">No emoji match that search.</p>
        ) : (
          <div role="listbox" aria-label="Emoji" className="grid grid-cols-6 gap-(--space-1)">
            {choices.map((emoji) => (
              <button
                key={emoji}
                type="button"
                role="option"
                aria-selected={false}
                aria-label={emoji}
                data-slot="emoji-picker-list-emoji"
                onClick={() => {
                  setOpen(false);
                  setQuery("");
                  onPick(emoji);
                }}
                className="text-lg flex size-8 items-center justify-center rounded-(--radius-control) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
      </div>
    </Popover>
  );
}
