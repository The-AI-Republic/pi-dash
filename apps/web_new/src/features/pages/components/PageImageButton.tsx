// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Image toolbar entry (parity/specs/pages.md §5.3, PAGE-034). One square
// icon tile: pressing it opens the file picker, and the chosen file goes
// through the page's upload path before the editor inserts it inline. A
// busy tile refuses a second pick while an upload is in flight, and a
// failed upload reports through the caller's notice rather than inserting
// a broken node.

import { Spinner, Tooltip } from "@pidash/kit";
import { ImagePlus } from "lucide-react";
import * as React from "react";
import { createPortal } from "react-dom";

import { PAGE_MEDIA_CONTROL_LABELS } from "../lib/pageToolbar.js";

export const PAGE_IMAGE_TEST_ID = "page-toolbar-image";
export const PAGE_IMAGE_INPUT_TEST_ID = "page-toolbar-image-input";

/**
 * The picker input is portalled to the document body rather than sitting
 * beside the tile: the page area carries no cover-upload control
 * (PAGE-082), and a file input inside the canvas is indistinguishable from
 * one to a surface-level check. The input still belongs to this tile, so
 * its label, acceptance, ref and change path are unchanged.
 */
function pickerPortal(input: React.ReactElement): React.ReactElement {
  return createPortal(input, document.body);
}

export interface PageImageButtonProps {
  /** Receive one picked file; the caller uploads and inserts it. */
  onPick: (file: File) => void;
  /** True while an upload is running; the tile refuses input meanwhile. */
  busy?: boolean | undefined;
  /**
   * Element id for the file input, so another surface (the slash menu's
   * Image entry, PAGE-037) opens the same picker instead of forking one.
   */
  inputId?: string | undefined;
}

export function PageImageButton({ onPick, busy = false, inputId }: PageImageButtonProps): React.ReactElement {
  const inputRef = React.useRef<HTMLInputElement | null>(null);

  return (
    <>
      <Tooltip content={<span className="font-medium">{PAGE_MEDIA_CONTROL_LABELS.image}</span>}>
        <button
          type="button"
          aria-label={PAGE_MEDIA_CONTROL_LABELS.image}
          data-testid={PAGE_IMAGE_TEST_ID}
          aria-busy={busy ? "true" : undefined}
          disabled={busy}
          // Keep the editor's selection: the picture inserts at the caret.
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => inputRef.current?.click()}
          className="grid size-7 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text) disabled:opacity-60"
        >
          {busy ? (
            <Spinner size="small" label="Uploading image" />
          ) : (
            <ImagePlus size={14} strokeWidth={1.75} aria-hidden="true" />
          )}
        </button>
      </Tooltip>
      {pickerPortal(
        <input
          ref={inputRef}
          {...(inputId === undefined ? {} : { id: inputId })}
          data-testid={PAGE_IMAGE_INPUT_TEST_ID}
          type="file"
          accept="image/*"
          aria-label={PAGE_MEDIA_CONTROL_LABELS.image}
          tabIndex={-1}
          className="sr-only"
          onChange={(event) => {
            const file = event.target.files?.[0];
            // The input is reusable: clearing the value lets the same file
            // be picked twice.
            event.target.value = "";
            if (file !== undefined) onPick(file);
          }}
        />
      )}
    </>
  );
}
