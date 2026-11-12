// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity block on the issue detail (parity/specs/issues.md §16,
// ISS-194–204): one timeline of property changes and comments, the
// filter and sort controls, and the composer placed by the sort order.
// The comment card wiring (edit, delete, access, fold, deep links) lives
// here too; the feed only decides where each card appears. Archived
// issues keep the timeline and drop the composer, the card menus and
// every reaction surface. An address carrying `#comment-<id>` scrolls
// that card into view on arrival and keeps a highlight on it for a few
// seconds.

import { cn } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useTriggerAgentRun } from "../../runners/index.js";
import { ActivityFeed } from "../activity/ActivityFeed.js";
import { issueKeys } from "../api/keys.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { canLogWorklog } from "../detail/widgets/worklog/eligibility.js";
import { WorklogHeaderButton } from "../detail/widgets/worklog/WorklogHeaderButton.js";
import { isWorklogEnabled } from "../detail/widgets/worklog/worklog.js";
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
import { CommentComposer, type ComposerPost } from "./CommentComposer.js";
import { commentDeepLink, isCommentFolded, type CommentMenuFlags } from "./model.js";

export const COMMENTS_HEADING = "Activity";
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

/** Relative deep link for a comment on the current page (CMT-007). */
export function commentLinkFor(location: CommentLocation, commentId: string): string {
  // The copied link is app-relative with a slash before the hash, exactly
  // what the old app writes (parity/specs/comments.md §5.2); relative
  // links stay valid across origins (dev port, cloud, desktop).
  const path = location.pathname.endsWith("/") ? location.pathname : `${location.pathname}/`;
  return commentDeepLink(path, commentId);
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
  /** Signed-in user, for authorship only; defaults to the session. */
  viewerId?: string | null;
  /** Assignees of the work item, for the log-time rule (ISS-206). */
  assigneeIds?: readonly string[];
  /** Project role (20 admin, 15 member, 5 guest); null when unknown. */
  memberRole?: number | null;
  /**
   * Comment & Run (ISS-193): post the draft, then dispatch a continuation
   * run. Undefined hides the action; a null workspace id (the workspace
   * read still landing) hides it too.
   */
  commentAndRun?: { workspaceId: string | null; disabled: boolean } | undefined;
}

function useCommentHighlight(comments: IssueComment[]): string | null {
  const [activeId, setActiveId] = React.useState<string | null>(() => commentIdFromHash(window.location.hash));
  React.useEffect(() => {
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
  viewerId: viewerOverride,
  assigneeIds = [],
  memberRole = null,
  commentAndRun,
}: CommentThreadProps): React.ReactElement {
  const scope = { workspaceSlug, projectId, issueId };
  const me = useMe();
  const queryClient = useQueryClient();
  const { triggerRun } = useTriggerAgentRun({
    onSettledRefresh: async () => {
      await queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    },
  });
  const viewerId = viewerOverride !== undefined ? viewerOverride : (me.data?.id ?? null);
  const worklogEnabled = isWorklogEnabled();
  const worklogLeading =
    worklogEnabled && canLogWorklog({ memberRole, viewerId, assigneeIds, isIntake, archived }) ? (
      <WorklogHeaderButton workspaceSlug={workspaceSlug} projectId={projectId} issueId={issueId} />
    ) : null;
  const comments = useIssueComments(scope);
  const create = useCreateComment(scope);
  const remove = useDeleteComment(scope);
  const update = useUpdateComment(scope, "Couldn't update the comment", "Comment updated successfully");
  const highlighted = useCommentHighlight(comments.data ?? []);
  const flags: CommentMenuFlags = { viewerId, accessToggle: published, copyLink: !isIntake };

  // The typed comment becomes the run's prompt: post it first, and only a
  // landed post dispatches (a refused post throws before the dispatch).
  const postCommentAndRun = async (workspaceId: string, input: ComposerPost): Promise<void> => {
    await create.mutateAsync(input);
    await triggerRun({ workspaceId, workspaceSlug, projectId, issueId, mode: "comment_and_run" });
  };
  const runWorkspaceId = commentAndRun?.workspaceId ?? null;
  const runDisabled = commentAndRun?.disabled ?? true;
  const runAction =
    runWorkspaceId === null
      ? undefined
      : {
          submit: (input: ComposerPost) => postCommentAndRun(runWorkspaceId, input),
          disabled: runDisabled,
        };

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

  const renderComment = (comment: IssueComment): React.ReactNode => (
    <div
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
        onToggleFold={(row) => update.mutate({ commentId: row.id, patch: foldPatch(row, !isCommentFolded(row)) })}
        onCopyLink={(row) => void copyLink(row)}
      />
    </div>
  );

  return (
    <ActivityFeed
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      issueId={issueId}
      comments={comments.data ?? []}
      commentsPending={comments.isPending}
      commentsError={comments.isError}
      archived={archived}
      worklogEnabled={worklogEnabled}
      headerLeading={worklogLeading}
      composer={
        <CommentComposer
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          showAccess={published}
          onCreate={(input) => create.mutateAsync(input)}
          {...(runAction === undefined ? {} : { commentAndRun: runAction })}
        />
      }
      renderComment={renderComment}
    />
  );
}
