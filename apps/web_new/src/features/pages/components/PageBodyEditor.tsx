// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body editing surface (parity/specs/pages.md §5.3, §10.1,
// PAGE-026–032). It is the page's own lazy editor: the page extension set
// (description core plus underline, alignment and task lists) and the
// Tiptap chunk resolve together through import(), so the initial load
// never carries them. It publishes the command handle the sticky bar and
// the title's Enter key drive, and reports the formatting state on every
// transaction — caret moves included — so the bar reflects the block
// under the caret rather than a stale selection. Persistence of the body
// belongs to the sync slice (PAGE-064); this surface only reports HTML.

import * as React from "react";

import { formattingStatesEqual, type PageFormattingState } from "../../../shared/editor/pageFormatting.js";
import {
  editorFormattingState,
  pageDocumentEmpty,
  pageEditorHandle,
  type PageEditorHandle,
  type PageEditorRef,
} from "../lib/pageEditorAdapter.js";
import { loadPageExtensions } from "../../../shared/editor/pageExtensions.js";
import { prepareEditorHtml } from "../../../shared/editor/sanitize.js";

export type { PageEditorHandle } from "../lib/pageEditorAdapter.js";

export interface PageBodyEditorProps {
  /** Stored HTML the body opens with. */
  initialHtml: string;
  /** Resolve a stored image asset id to its served source (PAGE-034). */
  resolveAsset?: ((src: string) => string) | undefined;
  /** Fires with the body's HTML on every edit. */
  onChange: (html: string) => void;
  /** Fires whenever the formatting state at the caret/selection changes. */
  onFormattingChange: (state: PageFormattingState) => void;
  /** Receives the command handle once an editor instance exists. */
  onReady: (handle: PageEditorHandle) => void;
  /** Accessible name of the editing region. */
  label: string;
  /** Placeholder shown while the document is empty. */
  placeholder?: string | undefined;
  /** Handles the editor's own keyboard conveniences. */
  handleKeyDown?: ((event: KeyboardEvent) => boolean) | undefined;
}

/** Tiptap's EditorContent takes its own editor type; this is ours. */
type TiptapEditor = Parameters<typeof import("@tiptap/react").EditorContent>[0]["editor"];

/** Load the page editor: the Tiptap chunk plus the page extension set. */
async function loadPageEditor(): Promise<{ default: React.ComponentType<PageBodyEditorProps> }> {
  const [{ EditorContent, useEditor }, extensions] = await Promise.all([import("@tiptap/react"), loadPageExtensions()]);

  function EditorBody({
    initialHtml,
    resolveAsset,
    onChange,
    onFormattingChange,
    onReady,
    label,
    placeholder,
    handleKeyDown,
  }: PageBodyEditorProps): React.ReactElement {
    const shownHtml = React.useMemo(() => prepareEditorHtml(initialHtml, resolveAsset), [initialHtml, resolveAsset]);
    const onChangeRef = React.useRef(onChange);
    onChangeRef.current = onChange;
    const formattingRef = React.useRef(onFormattingChange);
    formattingRef.current = onFormattingChange;
    const keyRef = React.useRef(handleKeyDown);
    keyRef.current = handleKeyDown;
    const readyRef = React.useRef(onReady);
    readyRef.current = onReady;
    const lastStateRef = React.useRef<PageFormattingState | null>(null);
    const [empty, setEmpty] = React.useState(() => plainTextOf(shownHtml) === "");

    const editor = useEditor(
      {
        extensions,
        content: shownHtml,
        editable: true,
        // The instance is created from the mount effect, never during
        // render, so the first paint never hands out a node that is
        // replaced a tick later.
        immediatelyRender: false,
        editorProps: {
          attributes: { "aria-label": label, class: "rich-text-content min-h-24 outline-none" },
          handleKeyDown: (_view, event) => keyRef.current?.(event) ?? false,
        },
        onUpdate: ({ editor: current }) => {
          onChangeRef.current(current.getHTML());
          setEmpty(pageDocumentEmpty(current as unknown as PageEditorRef));
        },
        // Every transaction, caret moves included: the bar tracks the
        // block under the caret (PAGE-028). A state equal to the last one
        // reported does not notify, so a plain caret move inside one
        // paragraph repaints nothing.
        onTransaction: ({ editor: current }) => {
          const next = editorFormattingState(current as unknown as PageEditorRef);
          const previous = lastStateRef.current;
          lastStateRef.current = next;
          if (previous !== null && formattingStatesEqual(previous, next)) return;
          formattingRef.current(next);
        },
      },
      [extensions]
    );

    React.useEffect(() => {
      if (editor !== null) readyRef.current(pageEditorHandle(editor as unknown as PageEditorRef));
    }, [editor]);

    if (editor === null) return <div aria-hidden="true" className="relative" />;
    return (
      <div className="relative">
        {placeholder !== undefined && empty ? (
          <div aria-hidden="true" className="text-body pointer-events-none absolute p-(--space-2) text-(--text-muted)">
            {placeholder}
          </div>
        ) : null}
        <EditorContent editor={editor as unknown as TiptapEditor} />
      </div>
    );
  }

  return { default: EditorBody };
}

/** Text-only reading of stored HTML, for the empty placeholder. */
function plainTextOf(html: string): string {
  return html.replace(/<[^>]*>/g, "").trim();
}

/**
 * The page's lazy body editor. Suspends while the editor chunk loads, so
 * callers render it under Suspense with the read-only body as fallback.
 */
export const PageBodyEditor = /* #__PURE__ */ React.lazy(loadPageEditor);
