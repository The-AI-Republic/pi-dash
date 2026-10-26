// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline public comment editor (parity/specs/space.md §3.10, SPC-056).
// Opens prefilled and focused on the stored body; the check (or Enter)
// saves once the draft differs from it, Enter on an empty or unchanged
// body does nothing, and cancel drops the draft without a write.
// Uploads stay on while editing and bind to the comment; mentions work
// as in the composer. The row reconciles from the server around this
// form (SPC-056): a refused save closes the form and shows the stored
// text with an error notice, owned by the caller.

import { Button, IconButton } from "@pidash/kit";
import type { SpaceComment } from "@pidash/api-client";
import { Check, ImagePlus, X } from "lucide-react";
import * as React from "react";

import { CommentRichTextEditor, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { spaceRuntime } from "../api/client.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { useUploadSpaceCommentImage } from "./api.js";
import { isSpaceCommentBodyEmpty, SPACE_COMMENT_IMAGE_UPLOADED_STATUS, spaceCommentEnterAction } from "./model.js";
import { SpaceMentionSuggestions, useSpaceMentionPicker } from "./SpaceMentionSuggestions.js";

export interface SpaceCommentEditFormProps {
  anchor: string;
  comment: SpaceComment;
  /** Resolve stored image asset ids through the public anchor route. */
  resolveAsset: (src: string) => string;
  /** Persist the new body; rejects through the caller's notice. */
  onSave: (html: string) => Promise<unknown>;
  onCancel: () => void;
}

export function SpaceCommentEditForm({
  anchor,
  comment,
  resolveAsset,
  onSave,
  onCancel,
}: SpaceCommentEditFormProps): React.ReactElement {
  const client = spaceRuntime.client;
  const [html, setHtml] = React.useState(comment.comment_html);
  const [saving, setSaving] = React.useState(false);
  const [uploads, setUploads] = React.useState(0);
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const upload = useUploadSpaceCommentImage(client, anchor);
  const empty = isSpaceCommentBodyEmpty(html);
  // A pristine draft (no change from the stored body) cannot be saved:
  // only a content change arms the check.
  const pristine = html === comment.comment_html;
  const busy = saving || uploads > 0;
  const mentions = useSpaceMentionPicker({
    client,
    anchor,
    getHandle: () => handleRef.current,
    busy,
    empty,
  });

  const save = async (): Promise<void> => {
    if (busy || empty || pristine) return;
    setSaving(true);
    try {
      await onSave(html);
    } catch {
      // The caller raised its own notice; the draft stays for a retry.
    } finally {
      setSaving(false);
    }
  };

  const uploadImage = (file: File): void => {
    setUploads((count) => count + 1);
    void upload
      .mutateAsync({ file, commentId: comment.id })
      .then((image) => {
        // The editor may have unmounted while the upload was out.
        handleRef.current?.insertImage(image.src, {
          storedSrc: image.assetId,
          status: SPACE_COMMENT_IMAGE_UPLOADED_STATUS,
        });
      })
      .catch(() => undefined)
      .finally(() => {
        setUploads((count) => count - 1);
      });
  };

  return (
    <form
      data-testid="space-comment-edit"
      className="flex flex-col gap-(--space-2)"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            if (mentions.mentionOpen) {
              event.preventDefault();
              mentions.dismiss();
            }
            return;
          }
          // Enter with a pristine draft saves nothing: the chain below
          // reaches save(), which holds pristine as well as busy and
          // empty.
          if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey) return;
          if (event.defaultPrevented) {
            // The editing surface already swallowed this Enter (a pick
            // or a no-op); only a pick still needs its insertion.
            if (spaceCommentEnterAction({ mentionOpen: mentions.mentionOpen, busy, empty }) === "pick-mention") {
              mentions.pickFirst();
            }
            return;
          }
          // IME composition commits with Enter: leave that Enter to the input.
          if (event.nativeEvent.isComposing) return;
          event.preventDefault();
          void save();
        }}
      >
        <React.Suspense fallback={<div className="min-h-24 rounded-(--radius-control) bg-(--subtle)" />}>
          <CommentRichTextEditor
            initialHtml={comment.comment_html}
            onChange={setHtml}
            label={spaceMessage("comments.editLabel")}
            autoFocus
            onReady={(handle) => {
              handleRef.current = handle;
            }}
            onMentionQuery={mentions.onMentionQuery}
            handleKeyDown={mentions.handleEditorKeyDown}
            resolveAsset={resolveAsset}
          />
        </React.Suspense>
        {mentions.mentionOpen ? <SpaceMentionSuggestions people={mentions.people} onPick={mentions.pick} /> : null}
      </div>
      <div className="flex items-center justify-end gap-(--space-2)">
        <label className="cursor-pointer rounded-(--radius-control) p-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)">
          <span className="sr-only">{spaceMessage("comments.composerImage")}</span>
          <input
            type="file"
            accept="image/*"
            data-testid="space-comment-edit-image"
            className="sr-only"
            aria-label={spaceMessage("comments.composerImage")}
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file === undefined) return;
              uploadImage(file);
            }}
          />
          <ImagePlus size={16} strokeWidth={1.5} aria-hidden="true" />
        </label>
        <IconButton
          label={spaceMessage("comments.editSave")}
          size="small"
          className="border border-(--success) text-(--success)"
          disabled={busy || empty || pristine}
          onClick={() => void save()}
        >
          <Check size={16} strokeWidth={2} />
        </IconButton>
        <Button
          size="small"
          variant="danger"
          aria-label={spaceMessage("comments.editCancel")}
          disabled={saving}
          onClick={onCancel}
        >
          <X size={14} strokeWidth={2} aria-hidden="true" />
        </Button>
      </div>
    </form>
  );
}
