// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One managed public comment row (parity/specs/space.md §3.10,
// SPC-055–057). The read-only row carries the identity, body and fold
// state; this layer owns the write controls around it: the owner-only
// menu (signed-in author, never in a frame) with exactly Edit and
// Delete, the inline edit form, and the grouped reaction row. Edits
// swap instantly and reconcile from a fresh thread on failure; deletes
// leave at once and restore on refusal; reactions toggle one write at
// a time with anonymous attempts leaving for sign-in.

import { IconButton, Menu } from "@pidash/kit";
import type { SpaceComment } from "@pidash/api-client";
import { MoreHorizontal, Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { useDeleteSpaceComment, useSpaceCommentReactions, useUpdateSpaceComment } from "./api.js";
import { canShowSpaceCommentMenu, spaceCommentAuthor } from "./model.js";
import { SpaceCommentEditForm } from "./SpaceCommentEditForm.js";
import { SpaceCommentReactions } from "./SpaceCommentReactions.js";
import { SpaceCommentRow } from "./SpaceCommentRow.js";

export interface SpaceCommentItemProps {
  anchor: string;
  issueId: string;
  comment: SpaceComment;
  /** Signed-in visitor id; null while anonymous or still loading. */
  viewerId: string | null;
  /** Embedded hosts hide the menu; reactions show counts only. */
  embedded: boolean;
  /** Whether the publisher enabled reactions on this board. */
  reactionsEnabled: boolean;
  /** Anonymous activations leave here for sign-in. */
  onRequireSignIn: () => void;
  /** Resolve stored image asset ids through the public anchor route. */
  resolveAsset: (src: string) => string;
}

export function SpaceCommentItem({
  anchor,
  issueId,
  comment,
  viewerId,
  embedded,
  reactionsEnabled,
  onRequireSignIn,
  resolveAsset,
}: SpaceCommentItemProps): React.ReactElement {
  const client = spaceRuntime.client;
  const [editing, setEditing] = React.useState(false);
  const update = useUpdateSpaceComment(client, anchor, issueId);
  const remove = useDeleteSpaceComment(client, anchor, issueId);
  const reactions = useSpaceCommentReactions(client, anchor, issueId, comment, onRequireSignIn);
  const author = spaceCommentAuthor(comment);

  const save = async (html: string): Promise<void> => {
    try {
      await update.mutateAsync({ commentId: comment.id, html });
    } finally {
      // Success shows the confirmed row; a failure reconciles to the
      // stored text, so the form closes either way (SPC-056).
      setEditing(false);
    }
  };

  const menu = canShowSpaceCommentMenu(comment, viewerId, embedded) ? (
    <Menu
      trigger={
        <IconButton label={spaceMessage("comments.menuLabel")} size="small" data-testid="space-comment-menu">
          <MoreHorizontal size={16} strokeWidth={1.5} />
        </IconButton>
      }
      items={[
        {
          label: spaceMessage("comments.menuEdit"),
          icon: <Pencil size={14} strokeWidth={1.5} />,
          onSelect: () => {
            setEditing(true);
          },
        },
        {
          label: spaceMessage("comments.menuDelete"),
          icon: <Trash2 size={14} strokeWidth={1.5} />,
          onSelect: () => {
            remove.mutate(comment.id);
          },
        },
      ]}
    />
  ) : undefined;

  return (
    <SpaceCommentRow
      comment={comment}
      resolveAsset={resolveAsset}
      {...(menu === undefined ? {} : { menu })}
      {...(reactionsEnabled
        ? {
            reactions: (
              <SpaceCommentReactions
                groups={reactions.groups}
                authorName={author.name}
                embedded={embedded}
                onToggle={(code) => {
                  void reactions.react(code);
                }}
                onPick={(code) => {
                  void reactions.react(code);
                }}
              />
            ),
          }
        : {})}
      {...(editing
        ? {
            editor: (
              <SpaceCommentEditForm
                anchor={anchor}
                comment={comment}
                resolveAsset={resolveAsset}
                onSave={save}
                onCancel={() => {
                  setEditing(false);
                }}
              />
            ),
          }
        : {})}
    />
  );
}
