// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction row (parity/specs/issues.md §16, ISS-203/204). One chip per
// emoji with its count, a highlight when the signed-in viewer is among
// the reactors and a tooltip naming them; the add button sits beside the
// chips and stays visible even before anyone has reacted. The same row
// serves the issue and each comment card.

import { Tooltip, cn } from "@pidash/kit";
import * as React from "react";

import { EmojiPicker } from "./EmojiPicker.js";
import { chipTooltip, emojiToReaction, reactionAction, type ReactionChip } from "./model.js";

export interface ReactionBarProps {
  chips: readonly ReactionChip[];
  /** Chosen emoji or chip activates a toggle. */
  onToggle: (code: string, action: "add" | "remove") => void;
  /** Archived or otherwise frozen surfaces drop both chips and the button. */
  disabled?: boolean;
  className?: string;
  /** Accessible name for the add trigger. */
  addLabel?: string;
}

/**
 * Highlight rule the parity driver reads off the chip: a reacted chip
 * carries the accent border class.
 */
export const REACTED_CHIP_CLASS = "border-accent-strong";

export function ReactionBar({
  chips,
  onToggle,
  disabled = false,
  className,
  addLabel,
}: ReactionBarProps): React.ReactElement {
  return (
    <div className={cn("flex flex-wrap items-center gap-(--space-1)", className)}>
      {chips.map((chip) => (
        <Tooltip key={chip.code} content={chipTooltip(chip)}>
          <button
            type="button"
            aria-label={`${chip.emoji} reaction, ${chip.count}`}
            aria-pressed={chip.reacted}
            onClick={() => {
              onToggle(chip.code, reactionAction(chip));
            }}
            className={cn(
              "text-meta inline-flex items-center gap-(--space-1) rounded-full border px-(--space-2) py-px transition-colors",
              chip.reacted
                ? `${REACTED_CHIP_CLASS} bg-(--accent-subtle) text-(--accent)`
                : "border-(--border) bg-(--surface) text-(--text) hover:border-(--text-muted)"
            )}
          >
            <span aria-hidden="true">{chip.emoji}</span>
            <span>{chip.count}</span>
          </button>
        </Tooltip>
      ))}
      {disabled ? null : (
        <EmojiPicker
          onPick={(emoji) => {
            onToggle(emojiToReaction(emoji), "add");
          }}
          {...(addLabel === undefined ? {} : { label: addLabel })}
        />
      )}
    </div>
  );
}
