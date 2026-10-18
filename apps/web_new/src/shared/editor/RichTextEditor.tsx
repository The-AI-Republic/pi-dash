// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy rich-text editor (UI kit and editor page). Tiptap arrives only
// through dynamic import() (Stack: heavy dependencies never join the
// eager closure), so this module exposes a React.lazy component: the
// caller renders it inside Suspense, typically when the user starts
// editing a RichTextView. The extension set covers the description
// core (StarterKit paragraphs, headings, lists, code blocks plus
// links); mentions, images, task lists and highlighting arrive with
// the slices that own those rows (ISS-127, ISS-197).

import * as React from "react";

import { loadDescriptionExtensions } from "./extensions.js";
import { isStoredHtmlEmpty } from "./sanitize.js";

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
}

async function loadEditor(): Promise<{ default: React.ComponentType<RichTextEditorProps> }> {
  const [{ EditorContent, useEditor }, extensions] = await Promise.all([
    import("@tiptap/react"),
    loadDescriptionExtensions(),
  ]);

  function TiptapEditor({
    initialHtml,
    onChange,
    editable = true,
    placeholder,
    label,
    autoFocus = false,
    className,
  }: RichTextEditorProps): React.ReactElement {
    const [empty, setEmpty] = React.useState(() => isStoredHtmlEmpty(initialHtml));
    const onChangeRef = React.useRef(onChange);
    onChangeRef.current = onChange;
    const editor = useEditor(
      {
        extensions,
        content: initialHtml,
        editable,
        autofocus: autoFocus ? "end" : false,
        editorProps: {
          attributes: { "aria-label": label, class: "rich-text-content min-h-24 outline-none" },
        },
        onUpdate: ({ editor: current }) => {
          setEmpty(current.isEmpty);
          onChangeRef.current(current.getHTML());
        },
      },
      []
    );
    React.useEffect(() => {
      editor?.setEditable(editable);
    }, [editor, editable]);
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

  return { default: TiptapEditor };
}

/**
 * The lazy Tiptap editor. Suspends while the editor chunk loads, so the
 * caller must render it under Suspense with a view or skeleton fallback.
 * Pure-annotated: the feature barrel re-exports this module, and without
 * the annotation the bundler keeps it (plus the sanitizer) in every
 * chunk that touches the barrel, including the initial chain.
 */
export const RichTextEditor = /* #__PURE__ */ React.lazy(loadEditor);
