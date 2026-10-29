// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment composer (parity/specs/issues.md §16, ISS-197;
// parity/specs/comments.md §2/§6, CMT-001/002/009/011/019). Enter posts
// a non-empty body; Shift+Enter inserts a line break and
// Ctrl/Cmd+Enter are inert. One post at a time: a second Enter while
// a write is out is a no-op, as is Enter mid-upload. Every attempted
// post clears the draft, success or failure (§11.3). On success the
// images uploaded while typing attach to the new comment. On published
// projects the toolbar carries the internal/external switch, internal
// by default. Typing @ opens the mention picker; Enter picks its first
// row and Escape dismisses it.

import { Button, IconButton, Tooltip } from "@pidash/kit";
import { Globe, ImagePlus, Lock, MessageSquarePlus } from "lucide-react";
import * as React from "react";
import type { CommentAccess } from "@pidash/api-client";

import { CommentRichTextEditor, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { COMMENT_IMAGE_UPLOADED_STATUS, isCommentBodyEmpty } from "./model.js";
import { useUploadCommentImage } from "./api.js";
import { CommentMentionSuggestions, useCommentMentionPicker } from "./MentionSuggestions.js";

export const COMPOSER_LABEL = "Add comment";
export const COMPOSER_PLACEHOLDER = "Add comment";
export const COMPOSER_SUBMIT_LABEL = "Comment";
export const COMPOSER_COMMENT_AND_RUN_LABEL = "Comment & Run";
export const COMPOSER_IMAGE_LABEL = "Add an image to the comment";
export const COMPOSER_INTERNAL_LABEL = "Private comment";
export const COMPOSER_EXTERNAL_LABEL = "Public comment";

/** One composer post: the body with its access choice and parked image assets. */
export interface ComposerPost {
  html: string;
  access: CommentAccess;
  assetIds: string[];
}

export interface CommentComposerProps {
  workspaceSlug: string;
  projectId: string;
  /** Published projects show the access switch (ISS-200). */
  showAccess: boolean;
  /** Post the body with its access choice and parked image assets. */
  onCreate: (input: ComposerPost) => Promise<unknown>;
  /**
   * Comment & Run (ISS-193): post the draft, then dispatch a continuation
   * run. Absent on surfaces without runs; disabled until dispatch can act.
   */
  commentAndRun?: { submit: (input: ComposerPost) => Promise<unknown>; disabled: boolean } | undefined;
}

export function CommentComposer({
  workspaceSlug,
  projectId,
  showAccess,
  onCreate,
  commentAndRun,
}: CommentComposerProps): React.ReactElement {
  const [html, setHtml] = React.useState("<p></p>");
  const [access, setAccess] = React.useState<CommentAccess>("INTERNAL");
  const [sending, setSending] = React.useState(false);
  const [uploads, setUploads] = React.useState(0);
  const [resetToken, setResetToken] = React.useState(0);
  const assetsRef = React.useRef<string[]>([]);
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const upload = useUploadCommentImage({ workspaceSlug, projectId });
  const empty = isCommentBodyEmpty(html);
  const busy = sending || uploads > 0;

  const mentions = useCommentMentionPicker({
    workspaceSlug,
    projectId,
    getHandle: () => handleRef.current,
    busy,
    empty,
    submit: (liveHtml) => void post(onCreate, liveHtml ?? undefined),
  });

  function clearDraft(): void {
    assetsRef.current = [];
    mentions.reset();
    setHtml("<p></p>");
    setResetToken((value) => value + 1);
  }

  // `liveHtml` is the draft as the editing surface holds it right now.
  // Enter arrives before React renders the keystroke that filled the
  // draft, so the `html` snapshot can still be the empty one while the
  // editor already holds words (ISS-197).
  async function post(via: (input: ComposerPost) => Promise<unknown>, liveHtml?: string): Promise<void> {
    if (busy) return;
    const body = liveHtml ?? html;
    if (isCommentBodyEmpty(body)) return;
    setSending(true);
    try {
      await via({ html: body, access, assetIds: assetsRef.current });
    } catch {
      // A refused post still clears the draft: parity pins the cleared
      // composer after a failed create (comments.md §11.3), the failure
      // notice already carries the outcome, and parked images must not
      // leak into the next, unrelated comment.
    }
    clearDraft();
    setSending(false);
  }

  const uploadImage = (file: File): void => {
    setUploads((count) => count + 1);
    void upload
      .mutateAsync({ file })
      .then((image) => {
        assetsRef.current = [...assetsRef.current, image.assetId];
        // The editor may have unmounted while the upload was out.
        handleRef.current?.insertImage(image.src, { storedSrc: image.assetId, status: COMMENT_IMAGE_UPLOADED_STATUS });
      })
      .catch(() => undefined)
      .finally(() => {
        setUploads((count) => count - 1);
      });
  };

  return (
    <form
      role="group"
      aria-label={COMPOSER_LABEL}
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-2)"
      onSubmit={(event) => {
        event.preventDefault();
        void post(onCreate);
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          if (!mentions.mentionOpen) return;
          event.preventDefault();
          mentions.dismiss();
          return;
        }
        if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey) return;
        // Inside the editing surface the surface owns plain Enter, at key
        // time and from the live document: it consumes the key itself and
        // a pick or no-op is already decided by then (CMT-011, ISS-197).
        // An Enter from anywhere else in the form (a toolbar control, the
        // file picker) still posts from the newest draft we can read.
        if (event.defaultPrevented) return;
        // IME composition commits with Enter: leave that Enter to the input.
        if (event.nativeEvent.isComposing) return;
        event.preventDefault();
        void post(onCreate, handleRef.current?.getHtml());
      }}
    >
      <React.Suspense fallback={<div className="min-h-16" />}>
        <CommentRichTextEditor
          key={resetToken}
          initialHtml="<p></p>"
          onChange={setHtml}
          label={COMPOSER_LABEL}
          placeholder={COMPOSER_PLACEHOLDER}
          onReady={(handle) => {
            handleRef.current = handle;
          }}
          onMentionQuery={mentions.onMentionQuery}
          handleKeyDown={mentions.handleEditorKeyDown}
        />
      </React.Suspense>
      {mentions.mentionOpen ? <CommentMentionSuggestions people={mentions.people} onPick={mentions.pick} /> : null}
      <div className="flex items-center justify-end gap-(--space-2)">
        {showAccess ? (
          <div
            role="radiogroup"
            aria-label="Comment visibility"
            className="mr-auto flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) p-(--space-1)"
          >
            <Tooltip content={COMPOSER_INTERNAL_LABEL}>
              <IconButton
                label={COMPOSER_INTERNAL_LABEL}
                role="radio"
                aria-checked={access === "INTERNAL"}
                size="small"
                className={access === "INTERNAL" ? "bg-(--subtle) text-(--text)" : undefined}
                onClick={() => setAccess("INTERNAL")}
              >
                <Lock size={12} strokeWidth={2} />
              </IconButton>
            </Tooltip>
            <Tooltip content={COMPOSER_EXTERNAL_LABEL}>
              <IconButton
                label={COMPOSER_EXTERNAL_LABEL}
                role="radio"
                aria-checked={access === "EXTERNAL"}
                size="small"
                className={access === "EXTERNAL" ? "bg-(--subtle) text-(--text)" : undefined}
                onClick={() => setAccess("EXTERNAL")}
              >
                <Globe size={12} strokeWidth={2} />
              </IconButton>
            </Tooltip>
          </div>
        ) : null}
        <label className="cursor-pointer rounded-(--radius-control) p-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)">
          <input
            type="file"
            accept="image/*"
            className="sr-only"
            aria-label={COMPOSER_IMAGE_LABEL}
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file === undefined) return;
              uploadImage(file);
            }}
          />
          <ImagePlus size={16} strokeWidth={1.5} aria-hidden="true" />
        </label>
        {commentAndRun === undefined ? null : (
          <Button
            type="button"
            size="small"
            variant="primary"
            loading={sending}
            disabled={commentAndRun.disabled || empty || busy}
            onClick={() => void post(commentAndRun.submit)}
          >
            <span className="flex items-center gap-(--space-1)">
              <MessageSquarePlus size={14} strokeWidth={1.5} aria-hidden="true" />
              <span>{COMPOSER_COMMENT_AND_RUN_LABEL}</span>
            </span>
          </Button>
        )}
        <Button type="submit" size="small" variant="primary" loading={sending} disabled={empty || busy}>
          {COMPOSER_SUBMIT_LABEL}
        </Button>
      </div>
    </form>
  );
}
