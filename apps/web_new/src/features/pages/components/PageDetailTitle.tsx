// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Title surface (parity/specs/pages.md §5.1, PAGE-026). The title is an
// in-place single-line editor at the top of the canvas: typing past the
// 255-character cap is refused (the paste path included), a live count
// appears only while the field holds focus, and Enter leaves the field
// for the top of the body instead of adding a line. Renames persist over
// the record endpoint on a short debounce, outside the body save path.
// Read-only pages keep the same editor shape (the oracle and the spec
// both read `.page-title-editor` for a title): the same single heading,
// with editing off, so the title reads as static text, the empty heading
// still paints `Untitled`, and there is no count and no caret.

import * as React from "react";

import {
  PAGE_TITLE_MAX_LENGTH,
  PAGE_TITLE_PLACEHOLDER,
  clipTitle,
  pastedTitleOf,
  titleIsEmpty,
  titleRoom,
} from "../lib/pageTitle.js";

export { PAGE_TITLE_MAX_LENGTH, PAGE_TITLE_PLACEHOLDER };

/** How long a keystroke burst waits before its rename is sent. */
export const PAGE_TITLE_SAVE_DEBOUNCE_MS = 120;

/** The heading an empty title renders: a placeholder, never a literal. */
export const PAGE_TITLE_EMPTY_LABEL = PAGE_TITLE_PLACEHOLDER;

export interface PageDetailTitleProps {
  pageId: string;
  name: string;
  editable: boolean;
  /** Persist the changed name; only called when it actually differs. */
  onRename?: ((name: string) => void) | undefined;
  /** Enter pressed in the field: the caret belongs at the body's top. */
  onJumpToBody?: (() => void) | undefined;
  /** The draft changed by typing (the canvas keeps the mark pinned). */
  onDraftChange?: ((name: string) => void) | undefined;
}

export function PageDetailTitle(props: PageDetailTitleProps): React.ReactElement {
  return <TitleShell {...props} />;
}

/**
 * The name a record carries in the title's single-line shape: a blank
 * name (spaces only) is no title at all and reads as the empty heading,
 * anything else keeps its text up to the cap.
 */
export function titleTextOf(name: string): string {
  return titleIsEmpty(name) ? "" : clipTitle(name);
}

function TitleShell({
  pageId,
  name,
  editable,
  onRename,
  onJumpToBody,
  onDraftChange,
}: PageDetailTitleProps): React.ReactElement {
  const [draft, setDraft] = React.useState(() => titleTextOf(name));
  const [focused, setFocused] = React.useState(false);
  // The name the server last accepted; a commit only leaves when the
  // draft has moved away from it.
  const savedRef = React.useRef(draft);
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const renameRef = React.useRef(onRename);
  renameRef.current = onRename;
  const jumpRef = React.useRef(onJumpToBody);
  jumpRef.current = onJumpToBody;
  const draftChangeRef = React.useRef(onDraftChange);
  draftChangeRef.current = onDraftChange;
  const focusedRef = React.useRef(focused);
  focusedRef.current = focused;

  const clearTimer = React.useCallback((): void => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const commit = React.useCallback(
    (next: string): void => {
      clearTimer();
      const value = titleTextOf(next);
      if (value === savedRef.current) return;
      savedRef.current = value;
      renameRef.current?.(value);
    },
    [clearTimer]
  );

  const schedule = React.useCallback(
    (next: string): void => {
      const value = titleTextOf(next);
      setDraft(value);
      draftChangeRef.current?.(value);
      clearTimer();
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        commit(value);
      }, PAGE_TITLE_SAVE_DEBOUNCE_MS);
    },
    [clearTimer, commit]
  );

  // A rename from elsewhere (another tab, a restore) lands in the field
  // unless the operator is typing in it right now. Read-only pages have
  // no draft of their own, so the record always wins there.
  React.useEffect(() => {
    const value = titleTextOf(name);
    savedRef.current = value;
    if (focusedRef.current && editable) return;
    setDraft(value);
  }, [name, editable]);

  React.useEffect(() => clearTimer, [clearTimer]);

  const rootClass = "page-title-editor min-h-16 py-(--space-4)";
  return (
    <div
      id={`${pageId}-title`}
      data-testid="page-detail-title"
      data-readonly={editable ? undefined : "true"}
      className={rootClass}
    >
      <React.Suspense fallback={<TitleHeading value={draft} editable={editable} />}>
        <LazyTitleField
          pageId={pageId}
          value={draft}
          editable={editable}
          onDraftChange={editable ? schedule : undefined}
          onCommit={editable ? commit : undefined}
          onFocusChange={editable ? setFocused : undefined}
          onJumpToBody={editable ? () => jumpRef.current?.() : undefined}
        />
      </React.Suspense>
      {editable && focused ? (
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

/**
 * The heading face both the editor and its pre-chunk fallback share: one
 * line of text, and the placeholder attribute the empty state paints.
 */
export function TitleHeading({ value, editable }: { value: string; editable: boolean }): React.ReactElement {
  const empty = titleIsEmpty(value);
  return (
    <h1
      role="textbox"
      aria-label="Page title"
      aria-multiline="false"
      tabIndex={editable ? 0 : -1}
      data-placeholder={PAGE_TITLE_PLACEHOLDER}
      {...(empty ? { "data-empty": "true" } : {})}
      className={
        empty
          ? "text-h1 is-empty min-h-12 py-0 break-words text-(--text) outline-none before:text-(--text-muted) before:content-[attr(data-placeholder)]"
          : "text-h1 min-h-12 py-0 break-words text-(--text) outline-none"
      }
    >
      {empty ? null : value}
    </h1>
  );
}

export interface TitleFieldProps {
  pageId: string;
  value: string;
  editable: boolean;
  /** Every accepted edit, already inside the cap; read-only has none. */
  onDraftChange?: ((next: string) => void) | undefined;
  /** Enter (or blur) asks for an immediate save; read-only has none. */
  onCommit?: ((next: string) => void) | undefined;
  /** Only editable fields report focus: a read-only title has no count. */
  onFocusChange?: ((focused: boolean) => void) | undefined;
  /** Editable fields only: a read-only title never hands the caret on. */
  onJumpToBody?: (() => void) | undefined;
}

/** The heading node the title's editor holds: one level-one heading. */
export function titleDocument(value: string): Record<string, unknown> {
  return {
    type: "doc",
    content: [
      value === ""
        ? { type: "heading", attrs: { level: 1 } }
        : { type: "heading", attrs: { level: 1 }, content: [{ type: "text", text: value }] },
    ],
  };
}

/** The text a title document holds, with no block separators. */
export function titleTextBetween(doc: {
  content: { size: number };
  textBetween: (from: number, to: number, blockSeparator: string, leafText: string) => string;
}): string {
  return doc.textBetween(0, doc.content.size, "", "");
}

type TitlePackages = {
  EditorContent: typeof import("@tiptap/react").EditorContent;
  useEditor: typeof import("@tiptap/react").useEditor;
  extensions: import("@tiptap/core").AnyExtension[];
};

/**
 * The title chunk: Tiptap, the single-heading document, the placeholder
 * and the shape guard. React.lazy caches this factory's result, so the
 * chunk is loaded and its extensions built exactly once per session.
 */
async function loadTitlePackages(): Promise<TitlePackages> {
  return (async () => {
    const [
      { EditorContent, useEditor },
      { default: StarterKit },
      { Extension },
      { Plugin, PluginKey, TextSelection },
      { Decoration, DecorationSet },
    ] = await Promise.all([
      import("@tiptap/react"),
      import("@tiptap/starter-kit"),
      import("@tiptap/core"),
      import("@tiptap/pm/state"),
      import("@tiptap/pm/view"),
    ]);
    type NodeType = import("@tiptap/pm/model").NodeType;
    // One heading, nothing else: the rest of the description kit stays
    // out of the title's chunk's document schema.
    const kitOptions = {
      document: { content: "heading" },
      heading: { levels: [1] },
      bold: false,
      italic: false,
      strike: false,
      code: false,
      codeBlock: false,
      blockquote: false,
      bulletList: false,
      orderedList: false,
      listItem: false,
      horizontalRule: false,
      dropcursor: false,
      gapcursor: false,
      hardBreak: false,
      history: false,
    } as unknown as import("@tiptap/starter-kit").StarterKitOptions;
    // The empty heading paints its placeholder from the document itself: a
    // node decoration carries the attribute and the paint class, so the
    // h1 an empty document renders always has both, however the editor
    // mounted and whichever field the check runs in.
    const placeholder = Extension.create({
      name: "pageTitlePlaceholder",
      addProseMirrorPlugins() {
        return [
          new Plugin({
            key: new PluginKey("pageTitlePlaceholder"),
            props: {
              decorations(state) {
                if (!titleIsEmpty(titleTextBetween(state.doc))) return null;
                // The title document is one heading, so the whole content
                // is the node the placeholder belongs to.
                return DecorationSet.create(state.doc, [
                  Decoration.node(0, state.doc.content.size, {
                    "data-placeholder": PAGE_TITLE_PLACEHOLDER,
                    "data-empty": "true",
                    class: TITLE_EMPTY_HEADING_CLASS,
                  }),
                ]);
              },
            },
          }),
        ];
      },
    });
    // The title is always exactly one level-one heading holding at most
    // the capped number of characters. Deleting the last character, a
    // select-all replacement, a paste or any other edit that would leave
    // a paragraph, a second block or text past the cap is folded back
    // into the single heading here, so the field can never grow a line or
    // hold a name that could not save (PAGE-026).
    const shape = Extension.create({
      name: "pageTitleShape",
      addProseMirrorPlugins() {
        return [
          new Plugin({
            key: new PluginKey("pageTitleShape"),
            appendTransaction: (transactions, _old, state) => {
              if (!transactions.some((transaction) => transaction.docChanged)) return null;
              const singleHeading = state.doc.childCount === 1 && state.doc.firstChild?.type.name === "heading";
              const atCap = titleTextBetween(state.doc).length <= PAGE_TITLE_MAX_LENGTH;
              if (singleHeading && atCap) return null;
              const headingType = state.schema.nodes.heading as NodeType;
              const folded = titleShapeOf(state.doc, state.selection.from);
              const text = titleTextOf(folded.text);
              const heading = headingType.create({ level: 1 }, text === "" ? undefined : state.schema.text(text));
              const transaction = state.tr.replaceWith(0, state.doc.content.size, heading);
              // The caret lands where it sat in the folded text, never
              // past the end of the one heading.
              transaction.setSelection(TextSelection.create(transaction.doc, Math.min(folded.caret, text.length) + 1));
              return transaction;
            },
          }),
        ];
      },
    });
    const extensions = [
      StarterKit.configure(kitOptions),
      placeholder,
      shape,
    ] as unknown as import("@tiptap/core").AnyExtension[];
    return { EditorContent, useEditor, extensions };
  })();
}

/** Attributes the ProseMirror surface carries for the title. */
export const TITLE_EDITOR_ATTRIBUTES = {
  role: "textbox",
  "aria-label": "Page title",
  "aria-multiline": "false",
  class: "text-h1 min-h-12 py-0 break-words whitespace-pre-wrap text-(--text) outline-none",
} as const;

/** Classes the decorated empty heading carries: the placeholder paint. */
export const TITLE_EMPTY_HEADING_CLASS = "is-empty before:text-(--text-muted) before:content-[attr(data-placeholder)]";

/**
 * The title's single heading, read out of a document that drifted from
 * that shape: the text of every block joined in order (the editing
 * surface never separates the title's blocks), and the caret's offset
 * inside that text.
 */
export function titleShapeOf(
  doc: {
    childCount: number;
    forEach: (visit: (child: { nodeSize: number; textContent: string }, offset: number) => void) => void;
  },
  from: number
): { text: string; caret: number } {
  let text = "";
  let caret: number | null = null;
  doc.forEach((child, offset) => {
    const start = offset + 1;
    const end = offset + child.nodeSize - 1;
    if (caret === null && from >= start && from <= end) caret = text.length + (from - start);
    text += child.textContent;
  });
  const inText = caret === null ? text.length : Math.min(caret, text.length);
  return { text, caret: inText };
}

const LazyTitleField = /* #__PURE__ */ React.lazy(async () => {
  const { EditorContent, useEditor, extensions } = await loadTitlePackages();

  function TitleField({
    pageId,
    value,
    editable,
    onDraftChange,
    onCommit,
    onFocusChange,
    onJumpToBody,
  }: TitleFieldProps): React.ReactElement {
    const valueRef = React.useRef(value);
    valueRef.current = value;
    const draftRef = React.useRef(onDraftChange);
    draftRef.current = onDraftChange;
    const commitRef = React.useRef(onCommit);
    commitRef.current = onCommit;
    const focusRef = React.useRef(onFocusChange);
    focusRef.current = onFocusChange;
    const jumpRef = React.useRef(onJumpToBody);
    jumpRef.current = onJumpToBody;
    const [empty, setEmpty] = React.useState(() => titleIsEmpty(value));

    const editor = useEditor(
      {
        extensions,
        content: titleDocument(value),
        immediatelyRender: false,
        editorProps: {
          attributes: TITLE_EDITOR_ATTRIBUTES,
          handleKeyDown: (_view, event) => {
            if (event.key !== "Enter") return false;
            // Enter never adds a line to a one-line title; it commits and
            // hands the caret to the body's first block (PAGE-026).
            event.preventDefault();
            commitRef.current?.(valueRef.current);
            jumpRef.current?.();
            return true;
          },
          handlePaste: (view, event) => {
            const text = pastedTitleOf(event.clipboardData?.getData("text/plain"));
            // Both a blank paste and one with no room left are refused; a
            // paste with room inserts only the plain text that fits.
            event.preventDefault();
            const room = titleRoom(titleTextBetween(view.state.doc).length);
            if (text === "" || room === 0) return true;
            view.dispatch(view.state.tr.insertText(text.slice(0, room)));
            return true;
          },
        },
        onUpdate: ({ editor: current }) => {
          const text = titleTextBetween(current.state.doc);
          setEmpty(titleIsEmpty(text));
          draftRef.current?.(text);
        },
        onTransaction: ({ editor: current }) => {
          setEmpty(titleIsEmpty(titleTextBetween(current.state.doc)));
        },
        onFocus: () => focusRef.current?.(true),
        onBlur: () => {
          focusRef.current?.(false);
          commitRef.current?.(valueRef.current);
        },
      },
      [extensions]
    );

    React.useEffect(() => {
      editor?.setEditable(editable);
    }, [editor, editable]);

    React.useEffect(() => {
      if (editor === null || editor.isFocused) return;
      if (titleTextBetween(editor.state.doc) === value) return;
      editor.commands.setContent(titleDocument(value), false);
    }, [editor, value]);

    React.useEffect(() => {
      setEmpty(titleIsEmpty(value));
    }, [value]);

    if (editor === null) return <TitleHeading value={value} editable={editable} />;
    return (
      <div data-title-editor-id={pageId} data-empty={empty ? "true" : undefined}>
        <EditorContent editor={editor} />
      </div>
    );
  }

  return { default: TitleField };
});
