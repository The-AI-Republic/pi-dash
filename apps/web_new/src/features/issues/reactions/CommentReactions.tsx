// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment reaction footer (parity/specs/issues.md §16, ISS-203). Reads the
// comment's reaction collection only when the card shows it: folded
// comments and archived issues never mount this component, so no request
// goes out for a cluster that would not be seen. The footer disappears
// once the last reaction is withdrawn.

import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useCommentReactions, useToggleCommentReaction } from "./api.js";
import { ReactionPicker } from "./ReactionPicker.js";
import { emojiToReaction, groupReactions } from "./model.js";
import { ReactionBar } from "./ReactionBar.js";

export const COMMENT_REACTIONS_LABEL = "Reactions on this comment";

export interface CommentReactionsProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  commentId: string;
  className?: string;
}

export function CommentReactions({
  workspaceSlug,
  projectId,
  issueId,
  commentId,
  className,
}: CommentReactionsProps): React.ReactElement | null {
  const me = useMe();
  const scope = { workspaceSlug, projectId, issueId };
  const reactions = useCommentReactions(scope, commentId);
  const toggle = useToggleCommentReaction(scope, commentId);
  const viewerId = me.data?.id ?? null;
  const chips = groupReactions(reactions.data ?? [], viewerId);

  if (reactions.isError) return null;
  // The footer renders only once something has been chosen; the picker
  // lives in the card header, so an empty list shows nothing here.
  if (reactions.isPending || chips.length === 0) return null;

  return (
    <section aria-label={COMMENT_REACTIONS_LABEL} className={className}>
      <ReactionBar
        chips={chips}
        disabled={toggle.isPending}
        onToggle={(code, action) => {
          toggle.mutate({ code, action });
        }}
      />
    </section>
  );
}

/**
 * Header picker for one comment (ISS-203). Choosing an emoji posts it
 * directly; the footer chips then carry the toggle to withdraw it.
 */
export function CommentReactionPicker({
  workspaceSlug,
  projectId,
  issueId,
  commentId,
}: CommentReactionsProps): React.ReactElement {
  const scope = { workspaceSlug, projectId, issueId };
  const toggle = useToggleCommentReaction(scope, commentId);
  return (
    <ReactionPicker
      disabled={toggle.isPending}
      onPick={(emoji) => {
        toggle.mutate({ code: emojiToReaction(emoji), action: "add" });
      }}
    />
  );
}
