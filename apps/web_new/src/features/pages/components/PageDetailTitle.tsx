// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail title surface (parity/specs/pages.md §5.1, PAGE-026). The title
// is an in-place field: input past the 255-character cap is refused, a
// live count shows only while the field has focus, and Enter leaves the
// field for the top of the body instead of inserting a newline. Typing
// persists over the record endpoint (the shell's rename mutation) after a
// short debounce so a keystroke burst does not race its own writes; blur
// and Enter flush at once. Read-only pages show static text with the
// `Untitled` fallback and never carry an editing surface.

import * as React from "react";

import { PAGE_UNTITLED, pageDisplayTitle } from "../lib/pageList.js";

export const PAGE_TITLE_PLACEHOLDER = PAGE_UNTITLED;
export const PAGE_TITLE_MAX_LENGTH = 255;
export const PAGE_TITLE_SAVE_DEBOUNCE_MS = 120;

/** The name a draft may carry: input past the cap is dropped. */
export function clipTitle(text: string): string {
  return text.length > PAGE_TITLE_MAX_LENGTH ? text.slice(0, PAGE_TITLE_MAX_LENGTH) : text;
}

export interface PageDetailTitleProps {
  pageId: string;
  name: string;
  editable: boolean;
  /** Persist the changed name; only called when it actually differs. */
  onRename?: ((name: string) => void) | undefined;
  /** Enter pressed in the field: the caret belongs at the body's top. */
  onJumpToBody?: (() => void) | undefined;
}

export function PageDetailTitle({
  pageId,
  name,
  editable,
  onRename,
  onJumpToBody,
}: PageDetailTitleProps): React.ReactElement {
  if (!editable) {
    return (
      <h1
        id={`${pageId}-title`}
        data-testid="page-detail-title"
        data-readonly="true"
        className="text-h1 min-h-16 py-(--space-4) text-(--text)"
      >
        {pageDisplayTitle(name)}
      </h1>
    );
  }
  return (
    <EditableTitle
      pageId={pageId}
      name={name}
      {...(onRename === undefined ? {} : { onRename })}
      {...(onJumpToBody === undefined ? {} : { onJumpToBody })}
    />
  );
}

function EditableTitle({
  pageId,
  name,
  onRename,
  onJumpToBody,
}: {
  pageId: string;
  name: string;
  onRename?: ((name: string) => void) | undefined;
  onJumpToBody?: (() => void) | undefined;
}): React.ReactElement {
  const nodeRef = React.useRef<HTMLDivElement>(null);
  const [draft, setDraft] = React.useState(() => clipTitle(name));
  const [focused, setFocused] = React.useState(false);
  // The name the server last acknowledged: a commit is only sent when the
  // draft has moved away from it.
  const savedRef = React.useRef(name);
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const debounceRef = React.useRef(onRename);
  debounceRef.current = onRename;

  const clearTimer = React.useCallback((): void => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const flush = React.useCallback((): void => {
    clearTimer();
    if (draft === savedRef.current) return;
    savedRef.current = draft;
    debounceRef.current?.(draft);
  }, [clearTimer, draft]);

  // Server-side renames (another tab, a restore) flow into the field; a
  // local edit in progress is never overwritten under the caret.
  React.useEffect(() => {
    const node = nodeRef.current;
    savedRef.current = name;
    if (node !== null && node === document.activeElement) return;
    setDraft(clipTitle(name));
    if (node !== null && node.textContent !== name) node.textContent = name;
  }, [name]);

  React.useEffect(() => clearTimer, [clearTimer]);

  function applyDraft(next: string): void {
    setDraft(next);
    clearTimer();
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      // Read the newest draft through the ref-free path: the closure's
      // value is the one the field now shows.
      if (next === savedRef.current) return;
      savedRef.current = next;
      debounceRef.current?.(next);
    }, PAGE_TITLE_SAVE_DEBOUNCE_MS);
  }

  function syncFromNode(node: HTMLDivElement): void {
    const text = node.textContent ?? "";
    const clipped = clipTitle(text);
    if (clipped !== text) {
      // Refuse the overflowing input in the field itself, not only in the
      // draft: the caret must never sit after unaccepted characters.
      node.textContent = clipped;
      const selection = window.getSelection();
      if (selection !== null) {
        const range = document.createRange();
        range.selectNodeContents(node);
        range.collapse(false);
        selection.removeAllRanges();
        selection.addRange(range);
      }
    }
    applyDraft(clipped);
  }

  const empty = draft.trim() === "";
  return (
    <div id={`${pageId}-title`} data-testid="page-detail-title" className="page-title-editor min-h-16 py-(--space-4)">
      <h1
        ref={nodeRef}
        role="textbox"
        aria-label="Page title"
        aria-multiline="false"
        aria-describedby={focused ? `${pageId}-title-count` : undefined}
        tabIndex={0}
        contentEditable
        suppressContentEditableWarning
        data-placeholder={PAGE_TITLE_PLACEHOLDER}
        data-empty={empty ? "true" : undefined}
        className={
          empty
            ? "text-h1 is-empty min-h-12 break-words whitespace-pre-wrap text-(--text) outline-none empty:before:text-(--text-muted) empty:before:content-[attr(data-placeholder)]"
            : "text-h1 min-h-12 break-words whitespace-pre-wrap text-(--text) outline-none"
        }
        onFocus={() => setFocused(true)}
        onInput={(event) => syncFromNode(event.currentTarget)}
        onBlur={(event) => {
          setFocused(false);
          syncFromNode(event.currentTarget);
          flush();
        }}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return;
          // Enter never inserts a line break in a one-line title; it hands
          // the caret to the body's first block (PAGE-026).
          event.preventDefault();
          flush();
          onJumpToBody?.();
        }}
        onPaste={(event) => {
          event.preventDefault();
          const current = event.currentTarget.textContent ?? "";
          const room = PAGE_TITLE_MAX_LENGTH - current.length;
          const text = clipTitle(event.clipboardData.getData("text/plain")).slice(0, Math.max(0, room));
          document.execCommand("insertText", false, text);
        }}
      ></h1>
      {focused ? (
        <span
          id={`${pageId}-title-count`}
          data-testid="page-title-count"
          aria-live="polite"
          className="text-caption text-(--text-muted)"
        >
          {draft.length}/{PAGE_TITLE_MAX_LENGTH}
        </span>
      ) : null}
    </div>
  );
}
