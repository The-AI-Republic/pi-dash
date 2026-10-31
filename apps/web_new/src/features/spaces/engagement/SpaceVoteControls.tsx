// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public vote controls (parity/specs/space.md §3.8, SPC-047). One shared
// pair serves the overlay and the board-card footer: each side shows its
// live count, names its voters in order on hover/focus, and marks the
// visitor's own side. Inside a frame both sides keep their counts and
// marks but stay inert (SPC-015).

import { Tooltip } from "@pidash/kit";
import { ThumbsDown, ThumbsUp } from "lucide-react";
import * as React from "react";

import { spaceMessage } from "../lib/spaceMessages.js";
import { spaceVoteHint, type SpaceVoteTally } from "./model.js";

export interface SpaceVoteControlsProps {
  /** The issue title the accessible names address. */
  issueName: string;
  tally: SpaceVoteTally;
  /** Framed controls keep counts and marks but never activate. */
  embedded: boolean;
  onVote: (side: 1 | -1) => void;
  testId: string;
  upTestId: string;
  downTestId: string;
}

export function SpaceVoteControls({
  issueName,
  tally,
  embedded,
  onVote,
  testId,
  upTestId,
  downTestId,
}: SpaceVoteControlsProps): React.ReactElement {
  const words = { you: spaceMessage("engagement.you"), empty: spaceMessage("engagement.noVotes") };
  return (
    <div data-testid={testId} className="flex items-center gap-(--space-1)">
      <SpaceVoteButton
        side={1}
        issueName={issueName}
        state={tally.up}
        hint={spaceVoteHint(tally.up, words)}
        embedded={embedded}
        onVote={onVote}
        testId={upTestId}
      />
      <SpaceVoteButton
        side={-1}
        issueName={issueName}
        state={tally.down}
        hint={spaceVoteHint(tally.down, words)}
        embedded={embedded}
        onVote={onVote}
        testId={downTestId}
      />
    </div>
  );
}

function SpaceVoteButton({
  side,
  issueName,
  state,
  hint,
  embedded,
  onVote,
  testId,
}: {
  side: 1 | -1;
  issueName: string;
  state: SpaceVoteTally["up"];
  hint: string;
  embedded: boolean;
  onVote: (side: 1 | -1) => void;
  testId: string;
}): React.ReactElement {
  const Glyph = side === 1 ? ThumbsUp : ThumbsDown;
  const verb = side === 1 ? "Upvote" : "Downvote";
  return (
    <Tooltip content={hint}>
      <button
        type="button"
        aria-label={`${verb} ${issueName}`}
        aria-pressed={state.mine}
        aria-disabled={embedded ? "true" : undefined}
        disabled={embedded}
        data-testid={testId}
        onClick={embedded ? undefined : () => onVote(side)}
        className={`text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) ${state.mine ? "text-(--accent)" : "text-(--text-muted)"} ${embedded ? "cursor-default opacity-60" : "cursor-pointer hover:bg-(--subtle) hover:text-(--text)"}`}
      >
        <Glyph size={12} strokeWidth={1.75} aria-hidden="true" />
        <span className="tabular-nums">{state.count}</span>
      </button>
    </Tooltip>
  );
}
