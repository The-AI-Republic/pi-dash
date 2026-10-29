// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment composer (parity/specs/space.md §3.10, SPC-053/054).
// A rich editor with a toolbar, formatting-keeping paste and an image
// command. Enter posts a non-empty body while signed in; Shift+Enter
// inserts a line break. One post at a time, and submitting mid-upload
// is blocked until the upload settles: the comment payload carries the
// image nodes and the confirmed assets link to the new comment after
// the create. A refused post keeps the draft and the thread for a
// retry; a refused upload drops only its own node. Typing @ opens the
// board-member picker; Enter picks its first row, Escape dismisses it.

import { Button } from "@pidash/kit";
import { ImagePlus } from "lucide-react";
import * as React from "react";

import { CommentRichTextEditor, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { spaceRuntime } from "../api/client.js";
import { spaceMessage } from "../lib/spaceMessages.js";
import { useCreateSpaceComment, useUploadSpaceCommentImage } from "./api.js";
import { isSpaceCommentBodyEmpty, SPACE_COMMENT_IMAGE_UPLOADED_STATUS } from "./model.js";
import { SpaceMentionSuggestions, useSpaceMentionPicker } from "./SpaceMentionSuggestions.js";

export interface SpaceCommentComposerProps {
  anchor: string;
  issueId: string;
}

export function SpaceCommentComposer({ anchor, issueId }: SpaceCommentComposerProps): React.ReactElement {
  const client = spaceRuntime.client;
  const [html, setHtml] = React.useState("<p></p>");
  const [sending, setSending] = React.useState(false);
  const [uploads, setUploads] = React.useState(0);
  const [resetToken, setResetToken] = React.useState(0);
  const assetsRef = React.useRef<string[]>([]);
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const create = useCreateSpaceComment(client, anchor, issueId);
  const upload = useUploadSpaceCommentImage(client, anchor);
  const empty = isSpaceCommentBodyEmpty(html);
  const busy = sending || uploads > 0;
  const mentions = useSpaceMentionPicker({
    client,
    anchor,
    getHandle: () => handleRef.current,
    busy,
    empty,
    submit: (liveHtml) => void post(liveHtml ?? undefined),
  });

  const clearDraft = (): void => {
    assetsRef.current = [];
    mentions.reset();
    setHtml("<p></p>");
    setResetToken((value) => value + 1);
  };

  // `liveHtml` is the draft as the editing surface holds it right now;
  // Enter lands before React renders the keystroke before it (ISS-197).
  const post = async (liveHtml?: string): Promise<void> => {
    const body = liveHtml ?? html;
    if (busy || isSpaceCommentBodyEmpty(body)) return;
    setSending(true);
    try {
      await create.mutateAsync({ html: body, assetIds: assetsRef.current });
      clearDraft();
    } catch {
      // The failure notice already carries the outcome; the draft and
      // the thread stay for a retry (SPC-053).
    } finally {
      setSending(false);
    }
  };

  const uploadImage = (file: File): void => {
    setUploads((count) => count + 1);
    void upload
      .mutateAsync({ file })
      .then((image) => {
        assetsRef.current = [...assetsRef.current, image.assetId];
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
      role="group"
      aria-label={spaceMessage("comments.composerLabel")}
      data-testid="space-comment-composer"
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-2)"
      onSubmit={(event) => {
        event.preventDefault();
        void post();
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
        // a pick or no-op is already decided by then (SPC-053, ISS-197).
        // An Enter from elsewhere in the form still posts.
        if (event.defaultPrevented) return;
        // IME composition commits with Enter: leave that Enter to the input.
        if (event.nativeEvent.isComposing) return;
        event.preventDefault();
        void post(handleRef.current?.getHtml());
      }}
    >
      <React.Suspense fallback={<div className="min-h-16" />}>
        <CommentRichTextEditor
          key={resetToken}
          initialHtml="<p></p>"
          onChange={setHtml}
          label={spaceMessage("comments.composerLabel")}
          placeholder={spaceMessage("comments.composerLabel")}
          onReady={(handle) => {
            handleRef.current = handle;
          }}
          onMentionQuery={mentions.onMentionQuery}
          handleKeyDown={mentions.handleEditorKeyDown}
        />
      </React.Suspense>
      {mentions.mentionOpen ? <SpaceMentionSuggestions people={mentions.people} onPick={mentions.pick} /> : null}
      <div className="flex items-center justify-end gap-(--space-2)">
        <label className="cursor-pointer rounded-(--radius-control) p-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)">
          <input
            type="file"
            accept="image/*"
            data-testid="space-comment-composer-image"
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
        <Button
          type="submit"
          size="small"
          variant="primary"
          loading={sending}
          disabled={empty || busy}
          data-testid="space-comment-composer-submit"
        >
          {spaceMessage("comments.composerSubmit")}
        </Button>
      </div>
    </form>
  );
}
