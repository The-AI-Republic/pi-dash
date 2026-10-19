// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment composer (parity/specs/issues.md §16, ISS-197). Enter posts a
// non-empty body; modifier-plus-Enter (and Shift+Enter) insert a line
// break. One post at a time: the second Enter while a write is out is a
// no-op. On success the editor clears and the images uploaded while
// typing attach to the new comment. On published projects the toolbar
// carries the internal/external switch, internal by default.

import { Button, IconButton, Tooltip } from "@pidash/kit";
import { Globe, ImagePlus, Lock } from "lucide-react";
import * as React from "react";
import type { CommentAccess } from "@pidash/api-client";

import { CommentRichTextEditor, type RichTextEditorHandle } from "../../../shared/editor/index.js";
import { isCommentBodyEmpty } from "./model.js";
import { useUploadCommentImage } from "./api.js";

export const COMPOSER_LABEL = "Add comment";
export const COMPOSER_PLACEHOLDER = "Add comment";
export const COMPOSER_SUBMIT_LABEL = "Comment";
export const COMPOSER_IMAGE_LABEL = "Add an image to the comment";
export const COMPOSER_INTERNAL_LABEL = "Private comment";
export const COMPOSER_EXTERNAL_LABEL = "Public comment";

export interface CommentComposerProps {
  workspaceSlug: string;
  projectId: string;
  /** Published projects show the access switch (ISS-200). */
  showAccess: boolean;
  /** Post the body with its access choice and parked image assets. */
  onCreate: (input: { html: string; access: CommentAccess; assetIds: string[] }) => Promise<unknown>;
}

export function CommentComposer({
  workspaceSlug,
  projectId,
  showAccess,
  onCreate,
}: CommentComposerProps): React.ReactElement {
  const [html, setHtml] = React.useState("<p></p>");
  const [access, setAccess] = React.useState<CommentAccess>("INTERNAL");
  const [sending, setSending] = React.useState(false);
  const [resetToken, setResetToken] = React.useState(0);
  const assetsRef = React.useRef<string[]>([]);
  const handleRef = React.useRef<RichTextEditorHandle | null>(null);
  const upload = useUploadCommentImage({ workspaceSlug, projectId });
  const empty = isCommentBodyEmpty(html);

  const post = async (): Promise<void> => {
    if (sending || empty) return;
    setSending(true);
    const assets = assetsRef.current;
    try {
      await onCreate({ html, access, assetIds: assets });
    } catch {
      // The create raised its own notice; the draft stays for a retry.
      setSending(false);
      return;
    }
    assetsRef.current = [];
    setHtml("<p></p>");
    setResetToken((value) => value + 1);
    setSending(false);
  };

  return (
    <form
      role="group"
      aria-label={COMPOSER_LABEL}
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-2)"
      onSubmit={(event) => {
        event.preventDefault();
        void post();
      }}
      onKeyDown={(event) => {
        if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey) return;
        // IME composition commits with Enter: leave that Enter to the input.
        if (event.nativeEvent.isComposing) return;
        event.preventDefault();
        void post();
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
        />
      </React.Suspense>
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
              void upload
                .mutateAsync({ file })
                .then((image) => {
                  assetsRef.current = [...assetsRef.current, image.assetId];
                  // The editor may have unmounted while the upload was out.
                  handleRef.current?.insertImage(image.src);
                })
                .catch(() => undefined);
            }}
          />
          <ImagePlus size={16} strokeWidth={1.5} aria-hidden="true" />
        </label>
        <Button type="submit" size="small" variant="primary" loading={sending} disabled={empty}>
          {COMPOSER_SUBMIT_LABEL}
        </Button>
      </div>
    </form>
  );
}
