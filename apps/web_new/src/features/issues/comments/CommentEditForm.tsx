// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline comment editor (parity/specs/issues.md §16, ISS-198). Opens
// prefilled and focused on the stored body; the check (or Enter) saves,
// Enter on an empty body does nothing, and cancel drops the draft without
// a write. Uploads stay on while editing and attach to the comment.

import { Button, IconButton } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import { Check, ImagePlus, X } from "lucide-react";
import * as React from "react";

import { CommentRichTextEditor, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { isCommentBodyEmpty } from "./model.js";
import { useUploadCommentImage } from "./api.js";

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
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const empty = isCommentBodyEmpty(html);
  const upload = useUploadCommentImage({ workspaceSlug, projectId });

  const save = async (): Promise<void> => {
    if (saving || empty) return;
    setSaving(true);
    try {
      await onSave(html);
    } catch {
      // The mutation raised its own notice; the draft stays for a retry.
    } finally {
      setSaving(false);
    }
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
          if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !empty) {
            event.preventDefault();
            void save();
          }
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
          />
        </React.Suspense>
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
              void upload
                .mutateAsync({ file, commentId: comment.id })
                .then((image) => {
                  handleRef.current?.insertImage(image.src);
                })
                .catch(() => undefined);
            }}
          />
          <ImagePlus size={16} strokeWidth={1.5} aria-hidden="true" />
        </label>
        <IconButton
          label={SAVE_COMMENT_LABEL}
          size="small"
          className="border border-(--success) text-(--success)"
          disabled={saving || empty}
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
