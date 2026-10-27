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
  healMissingPageEmbeds,
  pageDocumentEmpty,
  pageEditorHandle,
  type PageEditorHandle,
  type PageEditorRef,
} from "../lib/pageEditorAdapter.js";
import { loadPageExtensions } from "../../../shared/editor/pageExtensions.js";
import { prepareEditorHtml } from "../../../shared/editor/sanitize.js";
import { PAGE_LIVE_BODY_FIELD, type PageLiveDocBinding } from "../lib/pageLiveBackend.js";

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
  /** Workspace slug: mention chips link to member profiles (PAGE-041). */
  workspaceSlug?: string | undefined;
  /** Placeholder shown while the document is empty. */
  placeholder?: string | undefined;
  /** Handles the editor's own keyboard conveniences. */
  handleKeyDown?: ((event: KeyboardEvent) => boolean) | undefined;
  /**
   * Rewrites pasted markup before it lands (PAGE-039). A picture from
   * another page is cloned first; a null answer means the paste is
   * refused (the caller has already reported why) and nothing is inserted.
   */
  onPasteHtml?: ((html: string) => Promise<string | null>) | undefined;
  /** Rewrites a plain-text entity address into a preview node (PAGE-043). */
  onPasteText?: ((text: string) => Promise<string | null>) | undefined;
  /** Whether a plain-text paste is an entity address worth resolving. */
  claimsPasteText?: ((text: string) => boolean) | undefined;
  /** The live document to bind (PAGE-065); absent keeps the standalone editor. */
  collab?: PageLiveDocBinding | null | undefined;
  /** Seed empty shared fields from the stored markup once the server state is known. */
  seedDocument?: boolean | undefined;
  /**
   * The stored structured snapshot. A body whose markup lost its embed
   * tags still carries them here; each missing embed is restored once,
   * at its snapshot position, after the server state is known (PAGE-040).
   */
  descriptionJson?: Record<string, unknown> | null | undefined;
}

/** Tiptap's EditorContent takes its own editor type; this is ours. */
type TiptapEditor = Parameters<typeof import("@tiptap/react").EditorContent>[0]["editor"];

/**
 * Load the page editor: the Tiptap chunk plus the page extension set.
 * The live variant also loads the collaboration binding and drops the
 * standalone undo history (collaborative editing has no undo); the
 * document it binds arrives per mount through the collab prop.
 */
async function loadPageEditor(
  workspaceSlug: string,
  collaborative: boolean
): Promise<{ default: React.ComponentType<PageBodyEditorProps> }> {
  const [{ EditorContent, useEditor }, extensions, collaboration] = await Promise.all([
    import("@tiptap/react"),
    loadPageExtensions(workspaceSlug, collaborative ? { history: false } : {}),
    collaborative ? import("@tiptap/extension-collaboration") : Promise.resolve(null),
  ]);

  function EditorBody({
    initialHtml,
    resolveAsset,
    onChange,
    onFormattingChange,
    onReady,
    label,
    placeholder,
    handleKeyDown,
    onPasteHtml,
    onPasteText,
    claimsPasteText,
    collab,
    seedDocument,
    descriptionJson,
  }: PageBodyEditorProps): React.ReactElement {
    const shownHtml = React.useMemo(() => prepareEditorHtml(initialHtml, resolveAsset), [initialHtml, resolveAsset]);
    const onChangeRef = React.useRef(onChange);
    onChangeRef.current = onChange;
    const formattingRef = React.useRef(onFormattingChange);
    formattingRef.current = onFormattingChange;
    const keyRef = React.useRef(handleKeyDown);
    keyRef.current = handleKeyDown;
    const pasteRef = React.useRef(onPasteHtml);
    pasteRef.current = onPasteHtml;
    const textPasteRef = React.useRef(onPasteText);
    textPasteRef.current = onPasteText;
    const claimTextRef = React.useRef(claimsPasteText);
    claimTextRef.current = claimsPasteText;
    const readyRef = React.useRef(onReady);
    readyRef.current = onReady;
    const lastStateRef = React.useRef<PageFormattingState | null>(null);
    const [empty, setEmpty] = React.useState(() => plainTextOf(shownHtml) === "");
    // The live variant mounts only with a binding (the outer component
    // keys the variant on the prop), so the cast below cannot fail.
    const boundExtensions = React.useMemo(
      () =>
        collaboration === null
          ? extensions
          : [
              ...extensions,
              collaboration.Collaboration.configure({
                document: (collab as PageLiveDocBinding).doc as import("yjs").Doc,
                field: PAGE_LIVE_BODY_FIELD,
              }),
            ],
      [collab]
    );

    const editor = useEditor(
      {
        extensions: boundExtensions,
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
      [boundExtensions]
    );

    // The binding renders the shared field over the initial markup,
    // so an editor the server left empty is seeded from stored markup
    // once the server state is known (PAGE-065). An editor showing
    // shared content — or the operator's own first keystrokes — is
    // never seeded over. Layout timing: the seed lands before the
    // first paint, never as an empty flash.
    React.useLayoutEffect(() => {
      if (editor === null || collaboration === null || seedDocument !== true) return;
      if (!pageDocumentEmpty(editor as unknown as PageEditorRef)) return;
      editor.commands.setContent(shownHtml);
    }, [editor, seedDocument, shownHtml]);

    // The snapshot heal runs after the seed, on the same signal: the
    // standalone editor heals as soon as it exists, the live editor once
    // the server state is known, and either way exactly once per mount
    // (PAGE-040). Running after the seed keeps the seed from wiping a
    // restored embed on an empty shared field.
    const healedRef = React.useRef(false);
    React.useLayoutEffect(() => {
      if (editor === null || healedRef.current) return;
      if (collaboration !== null && seedDocument !== true) return;
      healedRef.current = true;
      healMissingPageEmbeds(editor as unknown as PageEditorRef, descriptionJson ?? null);
    }, [editor, seedDocument, descriptionJson, collaboration]);

    React.useEffect(() => {
      if (editor !== null) readyRef.current(pageEditorHandle(editor as unknown as PageEditorRef));
    }, [editor]);

    // A paste carrying stored markup may point at a picture owned by
    // another page. The handler rewrites it first, then inserts the result
    // as an ordinary document edit (PAGE-039). Text-only pastes keep the
    // editor's native Markdown handling.
    React.useEffect(() => {
      if (editor === null) return;
      const dom = editor.view.dom;
      const onPaste = (event: ClipboardEvent): void => {
        handlePagePaste(
          event,
          pasteRef.current,
          textPasteRef.current,
          (html) => editor.commands.insertContent(html),
          claimTextRef.current
        );
      };
      // Capture runs before the editor's own paste handler, and a handled
      // paste stops there: otherwise the original markup lands first and
      // the rewritten clone is inserted a second time (PAGE-039).
      dom.addEventListener("paste", onPaste, true);
      return () => dom.removeEventListener("paste", onPaste, true);
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

/** The clipboard flavours a page paste reads (a browser ClipboardEvent). */
export interface PagePasteEvent {
  clipboardData?: { getData: (type: string) => string } | null | undefined;
  preventDefault: () => void;
  stopPropagation: () => void;
}

/**
 * Decide one paste (PAGE-037, PAGE-039). Stored markup goes through the
 * caller's rewrite first; Markdown text becomes blocks; anything else is
 * left to the editor's native paste. A handled paste is claimed
 * synchronously so the editor never inserts the unrewritten form.
 */
export function handlePagePaste(
  event: PagePasteEvent,
  paste: ((html: string) => Promise<string | null>) | undefined,
  pasteText: ((text: string) => Promise<string | null>) | undefined,
  insert: (html: string) => void,
  claimsText?: ((text: string) => boolean) | undefined
): boolean {
  const html = event.clipboardData?.getData("text/html") ?? "";
  if (html !== "") {
    if (paste === undefined) return false;
    event.preventDefault();
    event.stopPropagation();
    void paste(html).then((rewritten) => {
      if (rewritten !== null) insert(rewritten);
    });
    return true;
  }
  const text = event.clipboardData?.getData("text/plain") ?? "";
  if (looksLikeMarkdown(text)) {
    event.preventDefault();
    event.stopPropagation();
    void markdownToEditorHtml(text).then((markup) => {
      if (markup !== "") insert(markup);
    });
    return true;
  }
  if (pasteText === undefined || claimsText === undefined || !claimsText(text)) return false;
  event.preventDefault();
  event.stopPropagation();
  void pasteText(text).then((markup) => {
    if (markup !== null) insert(markup);
    else {
      // The resolver declined (an unknown key): put the original text back
      // so a normal address is never swallowed.
      insert(plainTextHtml(text));
    }
  });
  return true;
}

/** Whether a plain-text paste carries Markdown structure worth parsing. */
export function looksLikeMarkdown(text: string): boolean {
  return /^#{1,6} |^[-*+] |^\d+\. |^> |^```|\*\*|__|\[[^\]]+\]\([^)]+\)/m.test(text);
}

/** Render pasted Markdown to stored editor markup, never to raw HTML. */
async function markdownToEditorHtml(text: string): Promise<string> {
  const { markdownHtml } = await import("../../../shared/markdown/index.js");
  return markdownHtml(text);
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Plain text as paragraph markup, preserving line breaks. */
function plainTextHtml(text: string): string {
  return text
    .split(/\r?\n/)
    .map((line) => `<p>${escapeHtml(line)}</p>`)
    .join("");
}

/** Text-only reading of stored HTML, for the empty placeholder. */
function plainTextOf(html: string): string {
  return html.replace(/<[^>]*>/g, "").trim();
}

/**
 * The page's lazy body editor. Suspends while the editor chunk loads, so
 * callers render it under Suspense with the read-only body as fallback.
 */
const PAGE_EDITORS = new Map<string, React.LazyExoticComponent<React.ComponentType<PageBodyEditorProps>>>();

/**
 * One lazy editor per workspace and mode: the extension set owns its
 * mention links, and the live variant owns the collaboration binding.
 */
function pageEditorFor(
  workspaceSlug: string,
  collaborative: boolean
): React.LazyExoticComponent<React.ComponentType<PageBodyEditorProps>> {
  const key = collaborative ? `${workspaceSlug}:live` : workspaceSlug;
  let editor = PAGE_EDITORS.get(key);
  if (editor === undefined) {
    editor = React.lazy(() => loadPageEditor(workspaceSlug, collaborative));
    PAGE_EDITORS.set(key, editor);
  }
  return editor;
}

export function PageBodyEditor({ workspaceSlug = "", collab, ...props }: PageBodyEditorProps): React.ReactElement {
  const Editor = pageEditorFor(workspaceSlug, collab !== null && collab !== undefined);
  return <Editor collab={collab} {...props} />;
}
