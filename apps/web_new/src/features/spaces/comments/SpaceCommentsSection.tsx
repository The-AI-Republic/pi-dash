// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment thread in the issue overlay (parity/specs/space.md
// §3.9, SPC-049–052). The section renders only while the publisher's
// comments flag is on, in every presentation. Signed-in visitors get a
// composer slot; anonymous visitors outside a frame get a sign-in
// invitation; embedded visitors get neither. The thread itself is public
// read-only and leaves edit/delete/reactions to slice J2.

import { Button, Skeleton } from "@pidash/kit";
import type { SpaceComment } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceCommentsQueryOptions } from "../api/queries.js";
import { spaceReturnPath, spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { SpaceCommentRow } from "./SpaceCommentRow.js";

export interface SpaceCommentsSectionProps {
  anchor: string;
  issueId: string;
  /** Board address position carried by the anonymous invitation. */
  search: SpaceBoardSearch;
  /** Whether the publisher enabled comments on this board. */
  enabled: boolean;
  /** Signed-in visitor id; null while anonymous or still loading. */
  viewerId: string | null;
  /** Embedded hosts hide the invitation (and later the composer). */
  embedded?: boolean | undefined;
  /** Resolve stored image asset ids through the public anchor route. */
  resolveAsset: (src: string) => string;
  /** J2 composition point for the signed-in composer. */
  composer?: React.ReactNode | undefined;
}

/** The sign-in target keeps the full current board position (SPC-049). */
export function spaceCommentsSignInHref(anchor: string, search: SpaceBoardSearch): string {
  return spaceSignInHref(anchor, search);
}

export function SpaceCommentsSection({
  anchor,
  issueId,
  search,
  enabled,
  viewerId,
  embedded = false,
  resolveAsset,
  composer,
}: SpaceCommentsSectionProps): React.ReactElement | null {
  const framed = useSpaceEmbedded();
  const inEmbed = embedded || framed;
  const query = spaceCommentsQueryOptions(spaceRuntime.client, anchor, issueId);
  const comments = useQuery({
    ...query,
    enabled: enabled && query.enabled !== false,
  });
  if (!enabled) return null;

  const rows: readonly SpaceComment[] = comments.data ?? [];
  return (
    <section data-testid="space-comments" className="flex flex-col gap-(--space-3)">
      <h3 className="text-emphasis m-0">{spaceMessage("comments.heading")}</h3>
      {comments.isError ? (
        <div data-testid="space-comments-error" className="flex flex-col items-start gap-(--space-2)">
          <p className="text-body m-0 text-(--text)">{spaceMessage("comments.loadFailure")}</p>
          <Button
            variant="secondary"
            size="small"
            onClick={() => {
              void comments.refetch();
            }}
          >
            {spaceMessage("comments.retry")}
          </Button>
        </div>
      ) : comments.isPending ? (
        <div data-testid="space-comments-loading" role="status" aria-label={spaceMessage("comments.loading")}>
          <Skeleton width="100%" height="12px" />
        </div>
      ) : rows.length === 0 ? (
        <p data-testid="space-comments-empty" className="text-body m-0 text-(--text-muted)">
          {spaceMessage("comments.empty")}
        </p>
      ) : (
        <div className="flex flex-col gap-(--space-4)">
          {rows.map((comment) => (
            <SpaceCommentRow key={comment.id} comment={comment} resolveAsset={resolveAsset} />
          ))}
        </div>
      )}
      {viewerId === null ? (
        inEmbed ? null : (
          <a
            href={spaceCommentsSignInHref(anchor, search)}
            data-testid="space-comments-signin"
            className="text-body self-start rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)"
          >
            {spaceMessage("comments.signInInvitation")}
          </a>
        )
      ) : (
        (composer ?? null)
      )}
    </section>
  );
}

/** Accessible current position for the invitation's return address. */
export function spaceCommentReturnPath(anchor: string, search: SpaceBoardSearch): string {
  return spaceReturnPath(anchor, search);
}
