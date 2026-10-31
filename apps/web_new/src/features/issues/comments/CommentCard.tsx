// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One comment card in the issue thread (parity/specs/issues.md §16,
// ISS-198–202). Author line with the age and the edited marker, the
// stored body rendered read-only, and the card menu: edit and delete for
// the author only, copy link on non-intake surfaces, the access switch
// on published projects and fold for everyone. A folded card keeps its
// header and swaps the body for an expand toggle; the deep link target
// (the card id) is what the page scrolls to and highlights.

import { Avatar, IconButton, Menu, Tooltip, type MenuEntry } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import { Copy, Globe, Lock, MoreHorizontal, Pencil, Trash2, UnfoldVertical, FoldVertical } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { descriptionAssetSrc, RichTextView } from "../../../shared/editor/index.js";
import { CommentReactionPicker, CommentReactions } from "../reactions/CommentReactions.js";
import { timeAgo } from "../../../shared/lib/timeAgo.js";
import {
  commentAuthorAvatar,
  commentAuthorName,
  commentMenuEntries,
  isCommentBodyEmpty,
  isCommentFolded,
  type CommentMenuEntry,
} from "./model.js";
import { CommentEditForm } from "./CommentEditForm.js";

/** Element id a comment's deep link points at. */
export function commentDomId(commentId: string): string {
  return `comment-${commentId}`;
}

/** How long the deep-link highlight stays after the card scrolls into view. */
export const COMMENT_HIGHLIGHT_MS = 8000;

const ENTRY_ICONS: Record<CommentMenuEntry["key"], React.ReactNode> = {
  edit: <Pencil size={14} strokeWidth={1.5} />,
  "copy-link": <Copy size={14} strokeWidth={1.5} />,
  access: <Globe size={14} strokeWidth={1.5} />,
  fold: <FoldVertical size={14} strokeWidth={1.5} />,
  delete: <Trash2 size={14} strokeWidth={1.5} />,
};

export interface CommentAccessFlags {
  /** Published project: the card shows the access mark and the switch. */
  toggle: boolean;
  /** Intake items carry no deep link. */
  copyLink: boolean;
}

export interface CommentCardProps {
  workspaceSlug: string;
  projectId: string;
  comment: IssueComment;
  /** Signed-in user, for authorship and nothing else. */
  viewerId: string | null;
  /** True while the thread is archived: no menu, no edit, no reactions. */
  readOnly: boolean;
  access: CommentAccessFlags;
  onEdit: (commentId: string, html: string) => Promise<unknown>;
  onDelete: (commentId: string) => void;
  onToggleAccess: (comment: IssueComment) => void;
  onToggleFold: (comment: IssueComment) => void;
  onCopyLink: (comment: IssueComment) => void;
}

/** Menu entries for one card, with icons attached. */
export function commentMenuItems(
  comment: IssueComment,
  flags: { viewerId: string | null; accessToggle: boolean; copyLink: boolean },
  handlers: {
    onEdit: () => void;
    onCopyLink: () => void;
    onToggleAccess: () => void;
    onToggleFold: () => void;
    onDelete: () => void;
  }
): MenuEntry[] {
  const run: Record<CommentMenuEntry["key"], () => void> = {
    edit: handlers.onEdit,
    "copy-link": handlers.onCopyLink,
    access: handlers.onToggleAccess,
    fold: handlers.onToggleFold,
    delete: handlers.onDelete,
  };
  return commentMenuEntries(comment, flags).map((entry) => ({
    label: entry.label,
    onSelect: run[entry.key],
    icon:
      entry.key === "fold" && isCommentFolded(comment) ? (
        <UnfoldVertical size={14} strokeWidth={1.5} />
      ) : (
        ENTRY_ICONS[entry.key]
      ),
  }));
}

export function CommentCard({
  workspaceSlug,
  projectId,
  comment,
  viewerId,
  readOnly,
  access,
  onEdit,
  onDelete,
  onToggleAccess,
  onToggleFold,
  onCopyLink,
}: CommentCardProps): React.ReactElement {
  // Comment images stored by the older client keep an asset id; they
  // resolve through the issue's project like the description does
  // (ISS-127).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId, apiBaseUrl: getAppClient().baseUrl }),
    [workspaceSlug, projectId]
  );
  const [editing, setEditing] = React.useState(false);
  const [expanded, setExpanded] = React.useState(false);
  const folded = isCommentFolded(comment);
  const showBody = !folded || expanded;
  const name = commentAuthorName(comment);
  const avatar = commentAuthorAvatar(comment);
  const empty = isCommentBodyEmpty(comment.comment_html);
  const edited = comment.edited_at !== null;

  const items = readOnly
    ? []
    : commentMenuItems(
        comment,
        { viewerId, accessToggle: access.toggle, copyLink: access.copyLink },
        {
          onEdit: () => {
            setEditing(true);
          },
          onCopyLink: () => onCopyLink(comment),
          onToggleAccess: () => onToggleAccess(comment),
          onToggleFold: () => onToggleFold(comment),
          onDelete: () => onDelete(comment.id),
        }
      );

  return (
    <article id={commentDomId(comment.id)} className="flex flex-col gap-(--space-2)">
      <header className="flex items-start gap-(--space-2)">
        <Avatar name={name} {...(avatar === undefined ? {} : { src: avatar })} size="small" className="mt-0.5" />
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-(--space-2)">
          <span className="text-meta font-medium text-(--text)">{name}</span>
          <Tooltip content={`${new Date(comment.created_at).toLocaleString()}${edited ? " (edited)" : ""}`}>
            <button
              type="button"
              className="text-caption text-(--text-muted) hover:text-(--text)"
              aria-label={`Commented ${timeAgo(comment.created_at)}${edited ? ", edited" : ""}`}
            >
              {timeAgo(comment.created_at)}
              {edited ? " (edited)" : ""}
            </button>
          </Tooltip>
          {folded && !editing ? (
            <button
              type="button"
              aria-expanded={expanded}
              className="text-caption text-(--text-muted) hover:text-(--text)"
              onClick={() => setExpanded((value) => !value)}
            >
              {expanded ? "Click to collapse" : "Folded comment · Click to expand"}
            </button>
          ) : null}
        </div>
        {access.toggle ? (
          <span className="shrink-0 pt-(--space-1) text-(--text-muted)">
            <Tooltip content={comment.access === "INTERNAL" ? "Private comment" : "Public comment"}>
              <span aria-label={comment.access === "INTERNAL" ? "Private comment" : "Public comment"}>
                {comment.access === "INTERNAL" ? (
                  <Lock size={12} strokeWidth={1.5} />
                ) : (
                  <Globe size={12} strokeWidth={1.5} />
                )}
              </span>
            </Tooltip>
          </span>
        ) : null}
        {readOnly ? null : (
          <CommentReactionPicker
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            issueId={comment.issue}
            commentId={comment.id}
          />
        )}
        {readOnly || items.length === 0 ? null : (
          <Menu
            trigger={
              <IconButton label="Comment actions" size="small">
                <MoreHorizontal size={16} strokeWidth={1.5} />
              </IconButton>
            }
            items={items}
          />
        )}
      </header>
      {editing ? (
        <CommentEditForm
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          comment={comment}
          onSave={async (html) => {
            await onEdit(comment.id, html);
            setEditing(false);
          }}
          onCancel={() => setEditing(false)}
        />
      ) : showBody ? (
        <>
          {empty ? (
            <p className="text-body text-(--text-muted)">This comment has no text.</p>
          ) : (
            <RichTextView
              html={comment.comment_html}
              label={`Comment by ${name}`}
              className="pl-(--space-8)"
              resolveAsset={resolveAsset}
              workspaceSlug={workspaceSlug}
            />
          )}
          {/* Folded cards and archived issues hide the whole cluster (ISS-203). */}
          {readOnly ? null : (
            <CommentReactions
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              issueId={comment.issue}
              commentId={comment.id}
              className="pl-(--space-8)"
            />
          )}
        </>
      ) : null}
    </article>
  );
}
