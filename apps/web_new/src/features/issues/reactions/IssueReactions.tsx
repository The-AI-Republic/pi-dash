// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-level reaction row (parity/specs/issues.md §16, ISS-204). Mounted
// for every viewer, even with no reactions yet: the add button is the
// entry point. Any signed-in viewer may react, whatever their project
// role or the item's sync state. An archived issue keeps the chips it
// already has but offers no add button and no toggle.

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
  /** Archived issues freeze the row; every other issue accepts a reaction. */
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
  const reactions = useIssueReactions(scope);
  const toggle = useToggleIssueReaction(scope);
  const viewerId = me.data?.id ?? null;

  const chips = groupReactions(reactions.data ?? [], viewerId);

  return (
    <section aria-label={ISSUE_REACTIONS_LABEL} className={className}>
      <ReactionBar
        chips={chips}
        disabled={disabled || toggle.isPending}
        onToggle={(code, action) => {
          toggle.mutate({ code, action });
        }}
      />
    </section>
  );
}
