// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public overlay engagement (parity/specs/space.md §3.8, SPC-047/048). The
// vote pair and the reaction row sit below the issue title; each half
// renders only while its publisher flag is on, and the reaction half
// stays out of embedding frames entirely (SPC-015). Anonymous activations
// leave for sign-in through the board's handler, defaulting to a full
// page navigation that keeps the whole board position (SPC-013).

import type { SpaceBoardIssueDetail } from "@pidash/api-client";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { groupSpaceReactions, tallySpaceVotes } from "./model.js";
import { SpaceReactionBar } from "./SpaceReactionBar.js";
import { SpaceVoteControls } from "./SpaceVoteControls.js";
import { useSpaceEngagement } from "./useSpaceEngagement.js";

export interface SpaceIssueEngagementProps {
  anchor: string;
  search: SpaceBoardSearch;
  issue: SpaceBoardIssueDetail;
  votesEnabled: boolean;
  reactionsEnabled: boolean;
  /** Framed overlays keep inert votes and no reactions at all. */
  embedded: boolean;
  /** Anonymous activations leave here; without one the page navigates. */
  onRequireSignIn?: (() => void) | undefined;
}

/**
 * The anonymous fallback: a full page navigation to sign-in carrying the
 * whole board position. Outside a browser window there is nowhere to go.
 */
export function defaultSpaceSignInRedirect(anchor: string, search: SpaceBoardSearch): void {
  if (typeof globalThis.window !== "undefined") {
    globalThis.window.location.assign(spaceSignInHref(anchor, search));
  }
}

export function SpaceIssueEngagement({
  anchor,
  search,
  issue,
  votesEnabled,
  reactionsEnabled,
  embedded,
  onRequireSignIn,
}: SpaceIssueEngagementProps): React.ReactElement | null {
  const fallback = React.useCallback(() => {
    defaultSpaceSignInRedirect(anchor, search);
  }, [anchor, search]);
  const engagement = useSpaceEngagement(spaceRuntime.client, anchor, onRequireSignIn ?? fallback);
  const showReactions = reactionsEnabled && !embedded;
  if (!votesEnabled && !showReactions) return null;
  const tally = tallySpaceVotes(issue.vote_items, engagement.viewerId);
  const groups = groupSpaceReactions(issue.reaction_items, engagement.viewerId);
  return (
    <section
      aria-label={spaceMessage("engagement.label")}
      data-testid="space-peek-engagement"
      className="flex flex-wrap items-center gap-(--space-3)"
    >
      {votesEnabled ? (
        <SpaceVoteControls
          issueName={issue.name}
          tally={tally}
          embedded={embedded}
          onVote={(side) => {
            void engagement.vote(issue, side);
          }}
          testId="space-peek-votes"
          upTestId="space-peek-vote-up"
          downTestId="space-peek-vote-down"
        />
      ) : null}
      {showReactions ? (
        <SpaceReactionBar
          issueName={issue.name}
          groups={groups}
          embedded={embedded}
          onToggle={(code) => {
            void engagement.react(issue, code);
          }}
          onPick={(code) => {
            void engagement.react(issue, code);
          }}
          addLabel={`Add a reaction to ${issue.name}`}
          panelLabel={spaceMessage("engagement.chooseReaction")}
          testId="space-peek-reactions"
          pillTestId="space-peek-reaction"
          addTestId="space-peek-reaction-add"
        />
      ) : null}
    </section>
  );
}
