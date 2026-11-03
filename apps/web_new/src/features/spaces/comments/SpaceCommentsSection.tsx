// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment thread in the issue overlay (parity/specs/space.md
// §3.9–3.10, SPC-049–057). The section renders only while the
// publisher's comments flag is on, in every presentation. Signed-in
// visitors outside a frame get the composer; anonymous visitors
// outside a frame get a sign-in invitation; embedded visitors get
// neither. Each row manages its own write controls (owner menu, inline
// edit, reactions) through the item below.

import { Button, Skeleton } from "@pidash/kit";
import type { SpaceComment } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceCommentsQueryOptions } from "../api/queries.js";
import { defaultSpaceSignInRedirect } from "../engagement/SpaceIssueEngagement.js";
import { spaceReturnPath, spaceSignInHref, type SpaceBoardSearch } from "../lib/address.js";
import { useSpaceEmbedded } from "../lib/spaceEmbed.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { SpaceCommentItem } from "./SpaceCommentItem.js";

export interface SpaceCommentsSectionProps {
  anchor: string;
  issueId: string;
  /** Board address position carried by the anonymous invitation. */
  search: SpaceBoardSearch;
  /** Whether the publisher enabled comments on this board. */
  enabled: boolean;
  /** Signed-in visitor id; null while anonymous or still loading. */
  viewerId: string | null;
  /** Embedded hosts hide the invitation and the composer. */
  embedded?: boolean | undefined;
  /** Resolve stored image asset ids through the public anchor route. */
  resolveAsset: (src: string) => string;
  /** The composer below the thread while signed in outside a frame. */
  composer?: React.ReactNode | undefined;
  /** Whether the publisher enabled reactions on this board. */
  reactionsEnabled?: boolean | undefined;
  /** Anonymous activations leave here for sign-in. */
  onRequireSignIn?: (() => void) | undefined;
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
  reactionsEnabled = false,
  onRequireSignIn,
}: SpaceCommentsSectionProps): React.ReactElement | null {
  const framed = useSpaceEmbedded();
  const inEmbed = embedded || framed;
  const requireSignIn = React.useCallback(() => {
    if (onRequireSignIn !== undefined) {
      onRequireSignIn();
    } else {
      defaultSpaceSignInRedirect(anchor, search);
    }
  }, [anchor, onRequireSignIn, search]);
  const showInvite = viewerId === null && !inEmbed;
  const showComposer = viewerId !== null && !inEmbed;
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
            <SpaceCommentItem
              key={comment.id}
              anchor={anchor}
              issueId={issueId}
              comment={comment}
              viewerId={viewerId}
              embedded={inEmbed}
              reactionsEnabled={reactionsEnabled}
              onRequireSignIn={requireSignIn}
              resolveAsset={resolveAsset}
            />
          ))}
        </div>
      )}
      {showInvite ? (
        <a
          href={spaceCommentsSignInHref(anchor, search)}
          data-testid="space-comments-signin"
          className="text-body self-start rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)"
        >
          {spaceMessage("comments.signInInvitation")}
        </a>
      ) : null}
      {showComposer ? (composer ?? null) : null}
    </section>
  );
}

/** Accessible current position for the invitation's return address. */
export function spaceCommentReturnPath(anchor: string, search: SpaceBoardSearch): string {
  return spaceReturnPath(anchor, search);
}
