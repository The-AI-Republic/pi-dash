// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editable page canvas (parity/specs/pages.md §5, PAGE-026–032). One
// component owns the page's editing pieces and the seams between them:
// the title field hands its Enter key to the body's start, the sticky bar
// drives the editor's commands, and the editor reports its formatting
// state back to the bar. The read-only body is the Suspense fallback, so
// a slow editor chunk never leaves the canvas blank.

import * as React from "react";

import {
  EMPTY_FORMATTING_STATE,
  type PageBlockKind,
  type PageFormatCommand,
  type PageFormattingState,
} from "../../../shared/editor/index.js";
import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { getPageBodyDraft, setPageBodyDraft } from "../lib/pageBodyDraft.js";
import { pageKeyCommand } from "../lib/pageToolbar.js";
import { PageBodyEditor, type PageEditorHandle } from "./PageBodyEditor.js";
import { PageBodySkeleton } from "./PageDetailLoading.js";
import { PageDetailTitle } from "./PageDetailTitle.js";
import { PageFormattingBar } from "./PageFormattingBar.js";

export const PAGE_BODY_LABEL = "Page body";
export const PAGE_BODY_PLACEHOLDER = "Start writing, or press / for commands";

export interface PageEditorSurfaceProps {
  pageId: string;
  /** Stored record name. */
  name: string;
  /** Stored body HTML; a draft typed this session wins over it. */
  descriptionHtml: string;
  /** Persist the record name (title saves outside the body path). */
  onRename: (name: string) => void;
  /** Whether the sticky bar renders (read-only pages never mount here). */
  showBar: boolean;
  /**
   * The cold-document skeleton holds the body area until the page's
   * document is known (PAGE-079); the title and bar render throughout.
   */
  bodyPending?: boolean | undefined;
}

export function PageEditorSurface({
  pageId,
  name,
  descriptionHtml,
  onRename,
  showBar,
  bodyPending = false,
}: PageEditorSurfaceProps): React.ReactElement {
  const handleRef = React.useRef<PageEditorHandle | null>(null);
  const [state, setState] = React.useState<PageFormattingState>(EMPTY_FORMATTING_STATE);
  const initialHtml = React.useMemo(() => getPageBodyDraft(pageId) ?? descriptionHtml, [pageId, descriptionHtml]);

  const onReady = React.useCallback((handle: PageEditorHandle) => {
    handleRef.current = handle;
    setState(handle.readState());
  }, []);

  const onChange = React.useCallback(
    (html: string) => {
      // The save slice (PAGE-064, NEWFRONT-846) subscribes to this seam.
      setPageBodyDraft(pageId, html);
    },
    [pageId]
  );

  const run = React.useCallback((command: PageFormatCommand) => {
    handleRef.current?.applyCommand(command);
  }, []);

  const pickBlock = React.useCallback((kind: PageBlockKind) => {
    handleRef.current?.applyBlockKind(kind);
  }, []);

  const handleKeyDown = React.useCallback((event: KeyboardEvent): boolean => {
    // Only the chords the editor leaves unbound (the alignments) run
    // here: ProseMirror skips its own key handler once a chord is
    // claimed, and an editor-native command returns null, so no chord
    // ever toggles twice.
    const command = pageKeyCommand(event);
    if (command === null) return false;
    event.preventDefault();
    handleRef.current?.applyCommand(command);
    return true;
  }, []);

  return (
    <>
      <PageDetailTitle
        pageId={pageId}
        name={name}
        editable
        onRename={onRename}
        onJumpToBody={() => handleRef.current?.focusStart()}
      />
      {showBar ? <PageFormattingBar state={state} onCommand={run} onBlockKind={pickBlock} /> : null}
      <div className="editor-container">
        {bodyPending ? (
          <PageBodySkeleton />
        ) : (
          <React.Suspense fallback={<RichTextView html={initialHtml} label={PAGE_BODY_LABEL} />}>
            {/* The editor instance belongs to one page: a navigation to a
                sibling page opens its own document, never the old one. */}
            <PageBodyEditor
              key={pageId}
              initialHtml={initialHtml}
              onChange={onChange}
              onFormattingChange={setState}
              onReady={onReady}
              label={PAGE_BODY_LABEL}
              placeholder={PAGE_BODY_PLACEHOLDER}
              handleKeyDown={handleKeyDown}
            />
          </React.Suspense>
        )}
      </div>
    </>
  );
}
