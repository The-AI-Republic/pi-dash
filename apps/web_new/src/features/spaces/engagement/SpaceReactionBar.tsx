// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public reaction row (parity/specs/space.md §3.8, SPC-048). One pill per
// emoji with its count and a hover hint naming the reactors, the visitor's
// own pills marked, plus the add control opening the picker. The same row
// serves the overlay and the board-card footer; inside a frame the pills
// stay readable while the add control is not rendered (SPC-015).
// Read-only callers (framed comment rows, SPC-057) render the pills as
// static counts instead of toggles.

import { Tooltip } from "@pidash/kit";
import * as React from "react";

import { spaceMessage } from "../lib/spaceMessages.js";
import { spaceReactionHint, type SpaceReactionGroup } from "./model.js";
import { SpaceReactionPicker } from "./SpaceReactionPicker.js";

export interface SpaceReactionBarProps {
  /** The issue title the accessible names address. */
  issueName: string;
  groups: readonly SpaceReactionGroup[];
  /** Framed rows keep the pills but offer no add control. */
  embedded: boolean;
  /** Read-only rows render the pills as static counts with no toggle. */
  readOnly?: boolean | undefined;
  /** A pill activation toggles the visitor's row for its code. */
  onToggle: (code: string) => void;
  /** A picker choice adds the visitor's row for its code. */
  onPick: (code: string) => void;
  addLabel: string;
  panelLabel: string;
  testId: string;
  pillTestId: string;
  addTestId: string;
}

export function SpaceReactionBar({
  issueName,
  groups,
  embedded,
  readOnly = false,
  onToggle,
  onPick,
  addLabel,
  panelLabel,
  testId,
  pillTestId,
  addTestId,
}: SpaceReactionBarProps): React.ReactElement {
  const you = spaceMessage("engagement.you");
  return (
    <div data-testid={testId} className="flex min-w-0 flex-wrap items-center gap-(--space-1)">
      {groups.map((group) => (
        <Tooltip key={group.code} content={spaceReactionHint(group, you)}>
          {readOnly ? (
            <span
              data-testid={pillTestId}
              data-reaction={group.code}
              className={`text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) px-(--space-1) ${group.reacted ? "text-(--accent)" : "text-(--text)"}`}
            >
              <span aria-hidden="true">{group.emoji}</span>
              <span className={`${group.reacted ? "text-(--accent)" : "text-(--text-muted)"} tabular-nums`}>
                {group.count}
              </span>
            </span>
          ) : (
            <button
              type="button"
              aria-label={`React with ${group.emoji} to ${issueName}`}
              aria-pressed={group.reacted}
              data-testid={pillTestId}
              data-reaction={group.code}
              onClick={() => onToggle(group.code)}
              className={`text-meta inline-flex cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) px-(--space-1) ${group.reacted ? "text-(--accent)" : "text-(--text)"}`}
            >
              <span aria-hidden="true">{group.emoji}</span>
              <span className={`${group.reacted ? "text-(--accent)" : "text-(--text-muted)"} tabular-nums`}>
                {group.count}
              </span>
            </button>
          )}
        </Tooltip>
      ))}
      {embedded ? null : (
        <SpaceReactionPicker onPick={onPick} label={addLabel} panelLabel={panelLabel} testId={addTestId} />
      )}
    </div>
  );
}
