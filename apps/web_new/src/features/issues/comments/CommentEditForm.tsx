// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline comment editor (parity/specs/issues.md §16, ISS-198; CMT-003;
// parity/specs/comments.md §2/§6, CMT-009/011/019). Opens
// prefilled and focused on the stored body; the check (or Enter) saves
// once the draft differs from it, Enter on an empty or unchanged body
// does nothing, and cancel drops the draft without a write. Uploads
// stay on while editing and attach to the comment; saving duplicates
// the images the edit keeps into the new revision. Typing @ opens the
// mention picker; Enter picks its first row and Escape dismisses it.

import { Button, IconButton } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import { Check, ImagePlus, X } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { CommentRichTextEditor, descriptionAssetSrc, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { COMMENT_IMAGE_UPLOADED_STATUS, isCommentBodyEmpty } from "./model.js";
import {
  COMMENT_UPDATE_FAILED_TITLE,
  commentFailureDetail,
  duplicateKeptCommentImages,
  useUploadCommentImage,
} from "./api.js";
import { CommentMentionSuggestions, useCommentMentionPicker } from "./MentionSuggestions.js";

export const EDIT_COMMENT_LABEL = "Edit comment";
export const SAVE_COMMENT_LABEL = "Save comment";
export const CANCEL_COMMENT_LABEL = "Cancel editing";

export interface CommentEditFormProps {
  workspaceSlug: string;
  projectId: string;
  comment: IssueComment;
  /** Persist the new body; rejects through the caller's notice. */
  onSave: (html: string) => Promise<unknown>;
  onCancel: () => void;
}

export function CommentEditForm({
  workspaceSlug,
  projectId,
  comment,
  onSave,
  onCancel,
}: CommentEditFormProps): React.ReactElement {
  const [html, setHtml] = React.useState(comment.comment_html);
  const [saving, setSaving] = React.useState(false);
  const [uploads, setUploads] = React.useState(0);
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const empty = isCommentBodyEmpty(html);
  // A pristine draft (no change from the stored body) cannot be saved:
  // only a content change arms the check (CMT-003).
  const pristine = html === comment.comment_html;
  const busy = saving || uploads > 0;
  const upload = useUploadCommentImage({ workspaceSlug, projectId });
  // Stored bodies keep bare asset ids; the editor paints the resolved
  // picture and carries the id back out, so opening and saving never
  // rewrites a kept image into a plain tag (CMT-009).
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId, apiBaseUrl: getAppClient().baseUrl }),
    [workspaceSlug, projectId]
  );

  // `liveHtml` is the draft as the editing surface holds it right now;
  // Enter lands before React renders the keystroke before it, so the
  // snapshot can trail the draft (ISS-197, CMT-003).
  const save = async (liveHtml?: string | null): Promise<void> => {
    const draft = liveHtml ?? html;
    if (busy || isCommentBodyEmpty(draft) || draft === comment.comment_html) return;
    setSaving(true);
    let next: string;
    try {
      next = await duplicateKeptCommentImages(
        getAppClient(),
        { workspaceSlug, projectId, commentId: comment.id },
        comment.comment_html,
        draft
      );
    } catch (error) {
      // The duplicate runs before the mutation owns the save, so this
      // refusal needs its own notice; the draft stays for a retry.
      notifyError(COMMENT_UPDATE_FAILED_TITLE, commentFailureDetail(error));
      setSaving(false);
      return;
    }
    try {
      await onSave(next);
    } catch {
      // The mutation raised its own notice; the draft stays for a retry.
    } finally {
      setSaving(false);
    }
  };

  const mentions = useCommentMentionPicker({
    workspaceSlug,
    projectId,
    getHandle: () => handleRef.current,
    busy,
    empty,
    submit: (liveHtml) => void save(liveHtml),
  });

  const uploadImage = (file: File): void => {
    setUploads((count) => count + 1);
    void upload
      .mutateAsync({ file, commentId: comment.id })
      .then((image) => {
        handleRef.current?.insertImage(image.src, {
          storedSrc: image.assetId,
          status: COMMENT_IMAGE_UPLOADED_STATUS,
        });
      })
      .catch(() => undefined)
      .finally(() => {
        setUploads((count) => count - 1);
      });
  };

  return (
    <form
      className="flex flex-col gap-(--space-2)"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div
        onKeyDown={(event) => {
          // The editing surface owns plain Enter, at key time and from the
          // live document: it consumes the key itself and a pick or no-op
          // is already decided by then (CMT-003/011, ISS-197). This
          // wrapper only adds the popup's Escape. The check button and
          // the form's submit stay the ways to save without the keyboard.
          if (event.key !== "Escape" || !mentions.mentionOpen) return;
          event.preventDefault();
          mentions.dismiss();
        }}
      >
        <React.Suspense fallback={<div className="min-h-24 rounded-(--radius-control) bg-(--subtle)" />}>
          <CommentRichTextEditor
            initialHtml={comment.comment_html}
            onChange={setHtml}
            label={EDIT_COMMENT_LABEL}
            autoFocus
            onReady={(handle) => {
              handleRef.current = handle;
            }}
            onMentionQuery={mentions.onMentionQuery}
            handleKeyDown={mentions.handleEditorKeyDown}
            resolveAsset={resolveAsset}
          />
        </React.Suspense>
        {mentions.mentionOpen ? <CommentMentionSuggestions people={mentions.people} onPick={mentions.pick} /> : null}
      </div>
      <div className="flex items-center justify-end gap-(--space-2)">
        <label className="cursor-pointer rounded-(--radius-control) p-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)">
          <span className="sr-only">Add an image to the comment</span>
          <input
            type="file"
            accept="image/*"
            className="sr-only"
            aria-label="Add an image to the comment"
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
          label={SAVE_COMMENT_LABEL}
          size="small"
          className="border border-(--success) text-(--success)"
          disabled={busy || empty || pristine}
          onClick={() => void save()}
        >
          <Check size={16} strokeWidth={2} />
        </IconButton>
        <Button size="small" variant="danger" aria-label={CANCEL_COMMENT_LABEL} disabled={saving} onClick={onCancel}>
          <X size={14} strokeWidth={2} aria-hidden="true" />
        </Button>
      </div>
    </form>
  );
}
