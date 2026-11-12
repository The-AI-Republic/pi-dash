// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public reaction picker (parity/specs/space.md §3.8, SPC-048). A small
// fixed shelf of emoji behind the add trigger; choosing one closes the
// panel and hands the API code back. The panel stays a popover so it can
// sit in the overlay or a card footer without shifting layout. The shelf
// is this subtree's own selection; the API accepts any code-point key.

import { Popover } from "@pidash/kit";
import { SmilePlus } from "lucide-react";
import * as React from "react";

import { spaceEmojiToReaction } from "./model.js";

/** The emoji the public picker offers, in panel order. */
export const SPACE_REACTION_SHELF: readonly string[] = [
  "👍",
  "❤️",
  "🎉",
  "😄",
  "🚀",
  "👀",
  "🔥",
  "✅",
  "💡",
  "🙏",
  "🤔",
  "👎",
];

export interface SpaceReactionPickerProps {
  /** The API code of the picked emoji ("🎉" -> "127881"). */
  onPick: (code: string) => void;
  /** Accessible name of the add trigger. */
  label: string;
  /** Accessible name of the emoji panel. */
  panelLabel: string;
  testId: string;
}

export function SpaceReactionPicker({
  onPick,
  label,
  panelLabel,
  testId,
}: SpaceReactionPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="w-60 p-(--space-3)"
      trigger={
        <button
          type="button"
          aria-label={label}
          aria-haspopup="dialog"
          aria-expanded={open}
          data-testid={testId}
          className="inline-flex cursor-pointer items-center rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <SmilePlus size={13} strokeWidth={1.75} aria-hidden="true" />
        </button>
      }
    >
      <div role="group" aria-label={panelLabel} className="grid grid-cols-6 gap-(--space-1)">
        {SPACE_REACTION_SHELF.map((emoji) => (
          <button
            key={emoji}
            type="button"
            aria-label={emoji}
            data-emoji={emoji}
            data-testid={`${testId}-option`}
            onClick={() => {
              setOpen(false);
              onPick(spaceEmojiToReaction(emoji));
            }}
            className="text-lg flex size-8 cursor-pointer items-center justify-center rounded-(--radius-control) hover:bg-(--subtle) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
          >
            {emoji}
          </button>
        ))}
      </div>
    </Popover>
  );
}
