// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy rich-text editor (UI kit and editor page). Tiptap arrives only
// through dynamic import() (Stack: heavy dependencies never join the
// eager closure), so this module exposes a React.lazy component: the
// caller renders it inside Suspense, typically when the user starts
// editing a RichTextView. The extension set covers the description
// core (StarterKit paragraphs, headings, lists, code blocks plus
// links) and, for the comment composer, inline images; mentions, task
// lists and highlighting arrive with the slices that own those rows
// (ISS-127).

import * as React from "react";

import { loadCommentExtensions, loadDescriptionExtensions, loadIssueDescriptionExtensions } from "./extensions.js";
import { mentionQueryAt } from "./mention.js";
import { isStoredHtmlEmpty } from "./sanitize.js";

/** One Tiptap extension instance, derived from the loader's own type. */
type EditorExtension =
  | Awaited<ReturnType<typeof loadDescriptionExtensions>>[number]
  | Awaited<ReturnType<typeof loadCommentExtensions>>[number];

/** The slice of the Tiptap editor the composer needs. */
export interface RichTextEditorHandle {
  insertImage: (src: string) => void;
  /** Insert a person mention at the caret (ISS-127). */
  insertMention: (target: { id: string; name: string }) => void;
  /** Text between the paragraph start and the caret, for @-queries. */
  textBeforeCaret: () => string;
}

export interface RichTextEditorProps {
  /** Stored HTML the editor opens with. */
  initialHtml: string;
  /** Fires with the current HTML on every editor transaction. */
  onChange: (html: string) => void;
  /** False renders the content read-only. */
  editable?: boolean | undefined;
  /** Placeholder overlay while the document holds no text. */
  placeholder?: string | undefined;
  /** Accessible label for the editing surface. */
  label: string;
  /** Focus the end of the document once the editor mounts. */
  autoFocus?: boolean | undefined;
  className?: string | undefined;
  /** Receives an insert handle once the editor exists. */
  onReady?: ((handle: RichTextEditorHandle) => void) | undefined;
  /** Fires with the @-query under the caret on every edit (ISS-127). */
  onMentionQuery?: ((query: string | null) => void) | undefined;
  /** Key handling; returning true means the editor consumed the key. */
  handleKeyDown?: ((event: KeyboardEvent) => boolean) | undefined;
  /** Explicit keyboard position inside a dialog's tab order (ISS-125). */
  tabIndex?: number | undefined;
}

interface TiptapChain {
  focus: () => TiptapChain;
  setImage: (options: { src: string }) => { run: () => void };
  insertContent: (value: EditorContent) => { run: () => void };
}

/** Anything Tiptap's insertContent accepts; the mention node is plain JSON. */
type EditorContent = { type: string; attrs: Record<string, string> } | string;

interface EditorSelectionLike {
  $from: {
    parentOffset: number;
    parent: { textBetween: (from: number, to: number, block?: string, leaf?: string) => string };
  };
}

interface EditorLike {
  chain: () => TiptapChain;
  state: { selection: EditorSelectionLike };
}

function insertImage(editor: EditorLike, src: string): void {
  editor.chain().focus().setImage({ src }).run();
}

function insertMention(editor: EditorLike, target: { id: string; name: string }): void {
  editor
    .chain()
    .focus()
    .insertContent({
      type: "mention",
      attrs: { id: target.id, entity_identifier: target.id, entity_name: "user_mention" },
    })
    .run();
}

/** The @-query under the caret, or null (ISS-127). */
function readQuery(editor: EditorLike): string | null {
  const query = mentionQueryAt(textBeforeCaret(editor));
  return query === null ? null : query.text;
}

function textBeforeCaret(editor: EditorLike): string {
  const { $from } = editor.state.selection;
  return $from.parent.textBetween(0, $from.parentOffset, undefined, "\ufffc");
}

function toHandle(editor: EditorLike): RichTextEditorHandle {
  return {
    insertImage: (src: string) => insertImage(editor, src),
    insertMention: (target: { id: string; name: string }) => insertMention(editor, target),
    textBeforeCaret: () => textBeforeCaret(editor),
  };
}

/**
 * Load one editor variant: the Tiptap chunk and its extension set resolve
 * together, so each surface suspends exactly once before it mounts.
 */
async function loadEditor(
  loadExtensions: () => Promise<EditorExtension[]>
): Promise<{ default: React.ComponentType<RichTextEditorProps> }> {
  const [{ EditorContent, useEditor }, extensions] = await Promise.all([import("@tiptap/react"), loadExtensions()]);

  interface BodyProps extends Omit<RichTextEditorProps, "onReady"> {
    extensions: EditorExtension[];
    /** Set by the loader below once an editor instance exists. */
    onEditorReady?: ((editor: EditorLike) => void) | undefined;
  }

  function EditorBody({
    initialHtml,
    onChange,
    editable = true,
    placeholder,
    label,
    autoFocus = false,
    className,
    extensions,
    onEditorReady,
    onMentionQuery,
    handleKeyDown,
    tabIndex,
  }: BodyProps): React.ReactElement {
    const [empty, setEmpty] = React.useState(() => isStoredHtmlEmpty(initialHtml));
    const onChangeRef = React.useRef(onChange);
    onChangeRef.current = onChange;
    const readyRef = React.useRef(onEditorReady);
    readyRef.current = onEditorReady;
    const mentionRef = React.useRef(onMentionQuery);
    mentionRef.current = onMentionQuery;
    const keyRef = React.useRef(handleKeyDown);
    keyRef.current = handleKeyDown;
    const editor = useEditor(
      {
        extensions,
        content: initialHtml,
        editable,
        // Create the editor in the mount effect, never during render.
        // Without this the manager builds one instance for the first
        // paint and then replaces it a tick later, swapping the editable
        // node out from under a just-typed caret (and any element
        // handle taken between the two commits).
        immediatelyRender: false,
        autofocus: autoFocus ? "end" : false,
        editorProps: {
          attributes: {
            "aria-label": label,
            ...(tabIndex === undefined ? {} : { tabindex: String(tabIndex) }),
            class: "rich-text-content min-h-24 outline-none",
          },
          handleKeyDown: (_view, event) => keyRef.current?.(event) ?? false,
        },
        onUpdate: ({ editor: current }) => {
          setEmpty(current.isEmpty);
          onChangeRef.current(current.getHTML());
          const finder = mentionRef.current;
          if (finder !== undefined) finder(readQuery(current));
        },
      },
      [extensions]
    );
    React.useEffect(() => {
      editor?.setEditable(editable);
    }, [editor, editable]);
    React.useEffect(() => {
      if (editor !== null) readyRef.current?.(editor as unknown as EditorLike);
    }, [editor]);
    const frame = className === undefined ? "relative" : `relative ${className}`;
    // One commit without an instance: useEditor publishes it from an
    // effect, so the first render holds layout and the editor follows.
    if (editor === null) return <div aria-hidden="true" className={frame} />;
    return (
      <div className={frame}>
        {placeholder !== undefined && empty ? (
          <div aria-hidden="true" className="text-body pointer-events-none absolute p-(--space-2) text-(--text-muted)">
            {placeholder}
          </div>
        ) : null}
        <EditorContent editor={editor} />
      </div>
    );
  }

  function TiptapEditor({
    onReady,
    onMentionQuery,
    handleKeyDown,
    tabIndex,
    ...rest
  }: RichTextEditorProps): React.ReactElement {
    const handleRef = React.useRef(onReady);
    handleRef.current = onReady;
    // Callers keep the newest handle: an editor instance can be replaced
    // before the body settles, and the live one is the last announced.
    const onEditorReady = React.useCallback((editor: EditorLike) => {
      handleRef.current?.(toHandle(editor));
    }, []);
    return (
      <EditorBody
        {...rest}
        extensions={extensions}
        onEditorReady={onEditorReady}
        onMentionQuery={onMentionQuery}
        handleKeyDown={handleKeyDown}
        tabIndex={tabIndex}
      />
    );
  }

  return { default: TiptapEditor };
}

/**
 * The lazy Tiptap editor. Suspends while the editor chunk loads, so the
 * caller must render it under Suspense with a view or skeleton fallback.
 * Pure-annotated: the feature barrel re-exports this module, and without
 * the annotation the bundler keeps it (plus the sanitizer) in every
 * chunk that touches the barrel, including the initial chain.
 */
export const RichTextEditor = /* #__PURE__ */ React.lazy(() => loadEditor(loadDescriptionExtensions));

/** Work-item description editor: images and mentions (ISS-127). */
export const IssueDescriptionEditor = /* #__PURE__ */ React.lazy(() => loadEditor(loadIssueDescriptionExtensions));

/**
 * The comment editor: the same lazy surface with inline images (ISS-197).
 * Each editor piece owns its own loader, so a surface never suspends a
 * second time for an extension set after its chunk arrives.
 */
export const CommentRichTextEditor = /* #__PURE__ */ React.lazy(() => loadEditor(loadCommentExtensions));
