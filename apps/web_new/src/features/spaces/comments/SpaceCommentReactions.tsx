// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment reaction row (parity/specs/space.md §3.10, SPC-057).
// The grouped pills with their counts and the add control, over the
// shared public reaction bar: hovering a pill names its reactors with
// the visitor's own marked, a pill activation toggles the visitor's row
// for its code, and the picker adds a new code. Embedded frames show
// the counts with no add control and inert pills.

import * as React from "react";

import { SpaceReactionBar } from "../engagement/SpaceReactionBar.js";
import type { SpaceReactionGroup } from "../engagement/model.js";
import { spaceMessage } from "../lib/spaceMessages.js";

export interface SpaceCommentReactionsProps {
  groups: readonly SpaceReactionGroup[];
  /** Display name the pill labels address. */
  authorName: string;
  /** Framed rows keep inert counts with no add control. */
  embedded: boolean;
  /** A pill activation toggles the visitor's row for its code. */
  onToggle: (code: string) => void;
  /** A picker choice adds the visitor's row for its code. */
  onPick: (code: string) => void;
}

export function SpaceCommentReactions({
  groups,
  authorName,
  embedded,
  onToggle,
  onPick,
}: SpaceCommentReactionsProps): React.ReactElement {
  return (
    <SpaceReactionBar
      issueName={authorName}
      groups={groups}
      embedded={embedded}
      readOnly={embedded}
      onToggle={onToggle}
      onPick={onPick}
      addLabel={spaceMessage("comments.reactAdd")}
      panelLabel={spaceMessage("engagement.chooseReaction")}
      testId="space-comment-reactions"
      pillTestId="space-comment-reaction"
      addTestId="space-comment-reaction-add"
    />
  );
}
