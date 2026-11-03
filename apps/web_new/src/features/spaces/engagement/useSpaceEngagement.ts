// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public engagement writes (parity/specs/space.md §3.8, SPC-047/048). One
// hook serves both surfaces: the overlay section and the board cards call
// the same vote/react, so a write from either place updates every cached
// copy of the issue. Anonymous activations leave through onRequireSignIn
// (the board's full-position sign-in address); votes in a frame are inert
// while reaction pills stay live, matching the old surfaces (SPC-015).

import {
  addSpaceIssueReaction,
  removeSpaceIssueReaction,
  removeSpaceIssueVote,
  setSpaceIssueVote,
  type ApiClient,
  type Me,
  type SpaceEngagementActor,
} from "@pidash/api-client";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { spaceViewerQueryOptions } from "../api/queries.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { notifySpaceReactionFailure, notifySpaceVoteFailure } from "../lib/spaceNotices.js";
import {
  applySpaceEngagement,
  invalidateSpaceEngagement,
  restoreSpaceEngagement,
  snapshotSpaceEngagement,
} from "./cache.js";
import {
  currentSpaceVote,
  nextSpaceReaction,
  nextSpaceVote,
  optimisticSpaceReactions,
  optimisticSpaceVotes,
  viewerSpaceReacted,
  type SpaceEngageableIssue,
} from "./model.js";

function viewerActor(viewer: Me): SpaceEngagementActor {
  return {
    id: viewer.id,
    display_name: viewer.display_name,
    first_name: viewer.first_name,
    last_name: viewer.last_name,
    avatar: viewer.avatar,
    avatar_url: viewer.avatar_url,
  };
}

export interface SpaceEngagement {
  /** The signed-in visitor's id, or null while anonymous or still loading. */
  viewerId: string | null;
  /** Toggle the visitor's vote to one side (no-op in a frame). */
  vote: (issue: SpaceEngageableIssue, side: 1 | -1) => Promise<void>;
  /** Toggle the visitor's reaction for one code. */
  react: (issue: SpaceEngageableIssue, code: string) => Promise<void>;
}

/**
 * Engagement writes for one board. While identity is still loading the
 * visitor reads as anonymous — the same shape the account slot offers —
 * so an early activation lands on sign-in rather than failing a write.
 */
export function useSpaceEngagement(client: ApiClient, anchor: string, onRequireSignIn: () => void): SpaceEngagement {
  const viewer = useQuery(spaceViewerQueryOptions(client));
  const embedded = useSpaceEmbedded();
  const queryClient = useQueryClient();

  const vote = React.useCallback(
    async (issue: SpaceEngageableIssue, side: 1 | -1): Promise<void> => {
      if (embedded) return;
      const signedIn = viewer.data;
      if (signedIn === null || signedIn === undefined) {
        onRequireSignIn();
        return;
      }
      const actor = viewerActor(signedIn);
      const write = nextSpaceVote(currentSpaceVote(issue.vote_items, actor.id), side);
      const snapshot = snapshotSpaceEngagement(queryClient, anchor, issue.id);
      applySpaceEngagement(queryClient, anchor, issue.id, (row) => ({
        ...row,
        vote_items: optimisticSpaceVotes(row.vote_items, actor, write),
      }));
      try {
        if (write.action === "remove") {
          await removeSpaceIssueVote(client, anchor, issue.id);
        } else {
          await setSpaceIssueVote(client, anchor, issue.id, write.value);
        }
        await invalidateSpaceEngagement(queryClient, anchor, issue.id);
      } catch {
        restoreSpaceEngagement(queryClient, snapshot);
        await invalidateSpaceEngagement(queryClient, anchor, issue.id);
        notifySpaceVoteFailure();
      }
    },
    [anchor, client, embedded, onRequireSignIn, queryClient, viewer.data]
  );

  const react = React.useCallback(
    async (issue: SpaceEngageableIssue, code: string): Promise<void> => {
      const signedIn = viewer.data;
      if (signedIn === null || signedIn === undefined) {
        onRequireSignIn();
        return;
      }
      const actor = viewerActor(signedIn);
      const write = nextSpaceReaction(viewerSpaceReacted(issue.reaction_items, actor.id, code), code);
      const snapshot = snapshotSpaceEngagement(queryClient, anchor, issue.id);
      applySpaceEngagement(queryClient, anchor, issue.id, (row) => ({
        ...row,
        reaction_items: optimisticSpaceReactions(row.reaction_items, actor, write),
      }));
      try {
        if (write.action === "remove") {
          await removeSpaceIssueReaction(client, anchor, issue.id, code);
        } else {
          await addSpaceIssueReaction(client, anchor, issue.id, code);
        }
        await invalidateSpaceEngagement(queryClient, anchor, issue.id);
      } catch {
        restoreSpaceEngagement(queryClient, snapshot);
        await invalidateSpaceEngagement(queryClient, anchor, issue.id);
        notifySpaceReactionFailure();
      }
    },
    [anchor, client, onRequireSignIn, queryClient, viewer.data]
  );

  return { viewerId: viewer.data?.id ?? null, vote, react };
}
