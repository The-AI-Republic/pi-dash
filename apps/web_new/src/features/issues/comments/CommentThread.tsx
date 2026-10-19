// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comments block on the issue detail (parity/specs/issues.md §16,
// ISS-197–202): the composer, then the comment cards in creation order.
// Archived issues keep the cards and drop the composer and menu. The
// block also owns the deep-link behaviour: an address carrying
// `#comment-<id>` scrolls that card into view on arrival and keeps a
// highlight on it for a few seconds.

import { Skeleton, cn } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  COMMENT_LINK_COPIED_TITLE,
  COMMENT_LINK_COPY_FAILED_TITLE,
  accessPatch,
  foldPatch,
  useCreateComment,
  useDeleteComment,
  useIssueComments,
  useUpdateComment,
} from "./api.js";
import { CommentCard, COMMENT_HIGHLIGHT_MS, commentDomId } from "./CommentCard.js";
import { CommentComposer } from "./CommentComposer.js";
import { commentDeepLink, isCommentFolded, type CommentMenuFlags } from "./model.js";

export const COMMENTS_HEADING = "Comments";
export const COMMENT_LINK_COPY_FAILED_DETAIL = "The browser refused the clipboard write.";
export const HASH_PREFIX = "#comment-";

/** Comment id carried by a `#comment-<id>` address, or null. */
export function commentIdFromHash(hash: string): string | null {
  if (!hash.startsWith(HASH_PREFIX)) return null;
  const id = hash.slice(HASH_PREFIX.length).trim();
  return id === "" ? null : id;
}

/** Window `location` shape the block reads (tests pass a stub). */
export interface CommentLocation {
  href: string;
  hash: string;
  pathname: string;
}

/** Absolute deep link for a comment on the current page. */
export function commentLinkFor(location: CommentLocation, commentId: string): string {
  const base = location.href.slice(0, location.href.length - location.hash.length);
  return commentDeepLink(base, commentId);
}

export interface CommentThreadProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Archived issues: cards stay, composer and menu go. */
  archived: boolean;
  /** Published project: the access switch and marks apply (ISS-200). */
  published: boolean;
  /** Intake items carry no deep links (ISS-202). */
  isIntake?: boolean;
}

function useCommentHighlight(comments: IssueComment[]): string | null {
  const [activeId, setActiveId] = React.useState<string | null>(() =>
    typeof window === "undefined" ? null : commentIdFromHash(window.location.hash)
  );
  React.useEffect(() => {
    if (typeof window === "undefined") return;
    const sync = (): void => {
      setActiveId(commentIdFromHash(window.location.hash));
    };
    window.addEventListener("hashchange", sync);
    return () => {
      window.removeEventListener("hashchange", sync);
    };
  }, []);
  React.useEffect(() => {
    if (activeId === null) return;
    const target = document.getElementById(commentDomId(activeId));
    target?.scrollIntoView?.({ block: "center" });
    const timer = setTimeout(() => {
      setActiveId(null);
    }, COMMENT_HIGHLIGHT_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [activeId, comments]);
  return activeId;
}

export function CommentThread({
  workspaceSlug,
  projectId,
  issueId,
  archived,
  published,
  isIntake = false,
}: CommentThreadProps): React.ReactElement {
  const scope = { workspaceSlug, projectId, issueId };
  const me = useMe();
  const viewerId = me.data?.id ?? null;
  const comments = useIssueComments(scope);
  const create = useCreateComment(scope);
  const remove = useDeleteComment(scope);
  const update = useUpdateComment(scope, "Couldn't update the comment", "Comment updated successfully");
  const highlighted = useCommentHighlight(comments.data ?? []);
  const flags: CommentMenuFlags = { viewerId, accessToggle: published, copyLink: !isIntake };

  const copyLink = async (comment: IssueComment): Promise<void> => {
    try {
      await navigator.clipboard.writeText(
        commentLinkFor(
          { href: window.location.href, hash: window.location.hash, pathname: window.location.pathname },
          comment.id
        )
      );
      notifySuccess(COMMENT_LINK_COPIED_TITLE);
    } catch (error) {
      notifyError(COMMENT_LINK_COPY_FAILED_TITLE, (error as Error).message || COMMENT_LINK_COPY_FAILED_DETAIL);
    }
  };

  return (
    <section aria-label={COMMENTS_HEADING} className="flex flex-col gap-(--space-4)">
      <h2 className="text-h4 text-(--text)">{COMMENTS_HEADING}</h2>
      {archived ? null : (
        <CommentComposer
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          showAccess={published}
          onCreate={(input) => create.mutateAsync(input)}
        />
      )}
      {comments.isPending ? (
        <div className="flex flex-col gap-(--space-4)" aria-busy="true">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      ) : comments.isError ? (
        <p className="text-body text-(--danger)">Couldn&apos;t load the comments.</p>
      ) : (
        <div className="flex flex-col gap-(--space-6)">
          {(comments.data ?? []).map((comment) => (
            <div
              key={comment.id}
              className={cn(
                "rounded-(--radius-control) p-(--space-2) transition-colors duration-500",
                highlighted === comment.id ? "bg-(--accent-subtle) ring-1 ring-(--accent)" : ""
              )}
            >
              <CommentCard
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                comment={comment}
                viewerId={viewerId}
                readOnly={archived}
                access={{ toggle: flags.accessToggle, copyLink: flags.copyLink }}
                onEdit={(commentId, html) => update.mutateAsync({ commentId, patch: { comment_html: html } })}
                onDelete={(commentId) => remove.mutate(commentId)}
                onToggleAccess={(row) => update.mutate({ commentId: row.id, patch: accessPatch(row) })}
                onToggleFold={(row) =>
                  update.mutate({ commentId: row.id, patch: foldPatch(row, !isCommentFolded(row)) })
                }
                onCopyLink={(row) => void copyLink(row)}
              />
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
