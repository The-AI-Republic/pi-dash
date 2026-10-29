// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One public comment row (parity/specs/space.md §3.9–3.10, SPC-050–057).
// Identity, bot suffix, relative age, optional own-line speaker label,
// rich body and the folded branch, with slots for slice J2's mutation
// controls: the owner menu in the header, the edit form in place of the
// body, and the reaction row below it. Folded rows keep the header and
// the menu while the body and the reactions stay hidden until expanded.

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
  /** Owner-only menu, rendered at the end of the header row. */
  menu?: React.ReactNode | undefined;
  /** Reaction row, rendered below the body while it shows. */
  reactions?: React.ReactNode | undefined;
  /** Edit form, replacing the speaker line and body while editing. */
  editor?: React.ReactNode | undefined;
}

/** Fold toggle text a visitor sees; the accessible name names the moderated comment. */
function foldLabel(expanded: boolean): string {
  return expanded ? spaceMessage("comments.collapse") : spaceMessage("comments.expand");
}

export function SpaceCommentRow({
  comment,
  resolveAsset,
  menu,
  reactions,
  editor,
}: SpaceCommentRowProps): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  const folded = isSpaceCommentFolded(comment);
  const showBody = !folded || expanded;
  const showAux = showBody || editor !== undefined;
  const author = spaceCommentAuthor(comment);
  const speaker = spaceCommentSpeakerLabel(comment);
  const age = useRelativeAge(comment.created_at);
  const bodyHtml = spaceCommentBodyHtml(comment);

  return (
    <div data-testid="space-comment-row" className="relative flex items-start gap-(--space-2)">
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
          <span data-testid="space-comment-author" className="text-meta font-medium text-(--text)">
            {author.name}
          </span>
          <p
            data-testid="space-comment-age"
            className="text-caption m-0 text-(--text-muted)"
            title={comment.created_at}
          >
            {spaceMessage("comments.commented")} {age}
          </p>
          {folded ? (
            <button
              type="button"
              data-testid="space-comment-fold"
              aria-expanded={expanded}
              className="text-caption cursor-pointer rounded-(--radius-control) text-(--text-muted) hover:text-(--text) focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:outline-none"
              onClick={() => {
                setExpanded((value) => !value);
              }}
            >
              {foldLabel(expanded)}
            </button>
          ) : null}
          {menu ?? null}
        </div>
        {editor !== undefined ? (
          editor
        ) : showBody ? (
          <div className="flex min-w-0 flex-col gap-(--space-1)">
            {speaker === null ? null : (
              <span className="text-meta font-medium text-(--text)">
                {speaker}
                {":"}
              </span>
            )}
            <div data-testid="space-comment-body">
              <RichTextView html={bodyHtml} label={spaceMessage("comments.bodyLabel")} resolveAsset={resolveAsset} />
            </div>
          </div>
        ) : null}
        {showAux && reactions !== undefined ? reactions : null}
      </div>
    </div>
  );
}
