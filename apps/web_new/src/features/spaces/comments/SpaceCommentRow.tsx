// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One read-only public comment row (parity/specs/space.md §3.9,
// SPC-050–052). Identity, bot suffix, relative age, optional own-line
// speaker label, rich body and the folded branch. Mutation controls
// (edit, delete, reactions, composer) belong to slice J2 and are
// deliberately absent here: J2 can wrap this row without rewriting it.

import { Avatar } from "@pidash/kit";
import type { SpaceComment } from "@pidash/api-client";
import * as React from "react";

import { RichTextView } from "../../../shared/editor/index.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { isSpaceCommentFolded, spaceCommentAuthor, spaceCommentBodyHtml, spaceCommentSpeakerLabel } from "./model.js";
import { useRelativeAge } from "./useRelativeAge.js";

export interface SpaceCommentRowProps {
  comment: SpaceComment;
  /** Resolve stored image asset ids through the public anchor route. */
  resolveAsset: (src: string) => string;
}

/** Fold toggle text a visitor sees; the accessible name carries "Folded". */
function foldLabel(expanded: boolean): string {
  return expanded ? spaceMessage("comments.collapse") : spaceMessage("comments.expand");
}

export function SpaceCommentRow({ comment, resolveAsset }: SpaceCommentRowProps): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  const folded = isSpaceCommentFolded(comment);
  const showBody = !folded || expanded;
  const author = spaceCommentAuthor(comment);
  const speaker = spaceCommentSpeakerLabel(comment);
  const age = useRelativeAge(comment.created_at);
  const bodyHtml = spaceCommentBodyHtml(comment);

  return (
    <div className="relative flex items-start gap-(--space-2)">
      <div className="relative px-1 pt-0.5">
        <Avatar
          name={author.name}
          initial={author.initial}
          {...(author.avatar === undefined ? {} : { src: author.avatar })}
          size="small"
        />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
        <div className="flex min-w-0 flex-wrap items-center gap-(--space-2)">
          <span className="text-meta font-medium text-(--text)">{author.name}</span>
          <p className="text-caption m-0 text-(--text-muted)" title={comment.created_at}>
            {spaceMessage("comments.commented")} {age}
          </p>
          {folded ? (
            <button
              type="button"
              aria-expanded={expanded}
              className="text-caption cursor-pointer rounded-(--radius-control) text-(--text-muted) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
              onClick={() => {
                setExpanded((value) => !value);
              }}
            >
              {foldLabel(expanded)}
            </button>
          ) : null}
        </div>
        {showBody ? (
          <div className="flex min-w-0 flex-col gap-(--space-1)">
            {speaker === null ? null : (
              <span className="text-meta font-medium text-(--text)">
                {speaker}
                {":"}
              </span>
            )}
            <RichTextView html={bodyHtml} label={spaceMessage("comments.bodyLabel")} resolveAsset={resolveAsset} />
          </div>
        ) : null}
      </div>
    </div>
  );
}
