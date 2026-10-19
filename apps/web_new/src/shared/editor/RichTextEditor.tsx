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

import { loadCommentExtensions, loadDescriptionExtensions } from "./extensions.js";
import { isStoredHtmlEmpty } from "./sanitize.js";

/** One Tiptap extension instance, derived from the loader's own type. */
type EditorExtension =
  | Awaited<ReturnType<typeof loadDescriptionExtensions>>[number]
  | Awaited<ReturnType<typeof loadCommentExtensions>>[number];

/** The slice of the Tiptap editor the composer needs (image inserts). */
export interface RichTextEditorHandle {
  insertImage: (src: string) => void;
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
}

interface TiptapChain {
  focus: () => { setImage: (options: { src: string }) => { run: () => void } };
}

interface EditorLike {
  chain: () => TiptapChain;
}

function insertImage(editor: EditorLike, src: string): void {
  editor.chain().focus().setImage({ src }).run();
}

function toHandle(editor: EditorLike): RichTextEditorHandle {
  return { insertImage: (src: string) => insertImage(editor, src) };
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
  }: BodyProps): React.ReactElement {
    const [empty, setEmpty] = React.useState(() => isStoredHtmlEmpty(initialHtml));
    const onChangeRef = React.useRef(onChange);
    onChangeRef.current = onChange;
    const readyRef = React.useRef(onEditorReady);
    readyRef.current = onEditorReady;
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
          attributes: { "aria-label": label, class: "rich-text-content min-h-24 outline-none" },
        },
        onUpdate: ({ editor: current }) => {
          setEmpty(current.isEmpty);
          onChangeRef.current(current.getHTML());
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

  function TiptapEditor({ onReady, ...rest }: RichTextEditorProps): React.ReactElement {
    const handleRef = React.useRef(onReady);
    handleRef.current = onReady;
    const onEditorReady = React.useCallback((editor: EditorLike) => {
      handleRef.current?.(toHandle(editor));
    }, []);
    return <EditorBody {...rest} extensions={extensions} onEditorReady={onEditorReady} />;
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

/**
 * The comment editor: the same lazy surface with inline images (ISS-197).
 * Each editor piece owns its own loader, so a surface never suspends a
 * second time for an extension set after its chunk arrives.
 */
export const CommentRichTextEditor = /* #__PURE__ */ React.lazy(() => loadEditor(loadCommentExtensions));
