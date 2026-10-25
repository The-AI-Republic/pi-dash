// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail title surface (parity/specs/pages.md §5.1, PAGE-026 mount for
// this shell; the full editing rules arrive with NEWFRONT-840). The
// editable variant is a single in-place field whose empty state carries
// the `Untitled` placeholder and never collapses; the read-only variant
// is static text with the same fallback.

import * as React from "react";

import { PAGE_UNTITLED, pageDisplayTitle } from "../lib/pageList.js";

export const PAGE_TITLE_PLACEHOLDER = PAGE_UNTITLED;
export const PAGE_TITLE_MAX_LENGTH = 255;

export interface PageDetailTitleProps {
  pageId: string;
  name: string;
  editable: boolean;
  /** Persist the trimmed name; only called for real changes. */
  onRename?: ((name: string) => void) | undefined;
}

export function PageDetailTitle({ pageId, name, editable, onRename }: PageDetailTitleProps): React.ReactElement {
  if (!editable) {
    return (
      <h1
        id={`${pageId}-title`}
        data-testid="page-detail-title"
        className="text-h1 min-h-16 py-(--space-4) text-(--text)"
      >
        {pageDisplayTitle(name)}
      </h1>
    );
  }
  return <EditableTitle pageId={pageId} name={name} {...(onRename === undefined ? {} : { onRename })} />;
}

function EditableTitle({
  pageId,
  name,
  onRename,
}: {
  pageId: string;
  name: string;
  onRename?: ((name: string) => void) | undefined;
}): React.ReactElement {
  const nodeRef = React.useRef<HTMLDivElement>(null);
  const [draft, setDraft] = React.useState(name);
  const [focused, setFocused] = React.useState(false);
  const latestRef = React.useRef(name);

  // Server-side renames (another tab, a restore) flow into the field; a
  // local edit in progress is never overwritten under the caret.
  React.useEffect(() => {
    const node = nodeRef.current;
    latestRef.current = name;
    setDraft(name);
    if (node !== null && node !== document.activeElement && node.textContent !== name) {
      node.textContent = name;
    }
  }, [name]);

  function syncDraft(node: HTMLDivElement): void {
    const text = node.textContent ?? "";
    if (text.length > PAGE_TITLE_MAX_LENGTH) {
      const clipped = text.slice(0, PAGE_TITLE_MAX_LENGTH);
      node.textContent = clipped;
      latestRef.current = clipped;
      setDraft(clipped);
      return;
    }
    latestRef.current = text;
    setDraft(text);
  }

  function commit(): void {
    if (latestRef.current === name) return;
    onRename?.(latestRef.current);
  }

  const empty = draft.trim() === "";
  return (
    <div id={`${pageId}-title`} data-testid="page-detail-title" className="min-h-16 py-(--space-4)">
      <div
        ref={nodeRef}
        role="textbox"
        aria-label="Page title"
        aria-multiline="false"
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
        onInput={(event) => syncDraft(event.currentTarget)}
        onBlur={(event) => {
          setFocused(false);
          syncDraft(event.currentTarget);
          commit();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            event.currentTarget.blur();
          }
        }}
        onPaste={(event) => {
          event.preventDefault();
          const text = event.clipboardData.getData("text/plain").slice(0, PAGE_TITLE_MAX_LENGTH);
          document.execCommand("insertText", false, text);
        }}
        aria-describedby={focused ? `${pageId}-title-count` : undefined}
      />
      {focused ? (
        <span id={`${pageId}-title-count`} aria-live="polite" className="text-caption text-(--text-muted)">
          {draft.length}/{PAGE_TITLE_MAX_LENGTH}
        </span>
      ) : null}
    </div>
  );
}
