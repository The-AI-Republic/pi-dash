// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-level reaction row (parity/specs/issues.md §16, ISS-204). Always
// mounted on a live issue, even with no reactions yet: the add button is
// the entry point. Archived issues render nothing, matching the rest of
// the frozen detail surface.

import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useIssueReactions, useToggleIssueReaction } from "./api.js";
import { groupReactions } from "./model.js";
import { ReactionBar } from "./ReactionBar.js";

export const ISSUE_REACTIONS_LABEL = "Reactions on this work item";

export interface IssueReactionsProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Archived issues freeze the row; hidden projects still react. */
  disabled?: boolean;
  className?: string;
}

export function IssueReactions({
  workspaceSlug,
  projectId,
  issueId,
  disabled = false,
  className,
}: IssueReactionsProps): React.ReactElement | null {
  const me = useMe();
  const scope = { workspaceSlug, projectId, issueId };
  // An archived issue shows no row, so its collection is not read either.
  const reactions = useIssueReactions(scope, !disabled);
  const toggle = useToggleIssueReaction(scope);
  const viewerId = me.data?.id ?? null;

  if (disabled) return null;

  const chips = groupReactions(reactions.data ?? [], viewerId);

  return (
    <section aria-label={ISSUE_REACTIONS_LABEL} className={className}>
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
