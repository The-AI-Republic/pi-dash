// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat composer input (home assistant card and the assistant chat). A
// contenteditable Tiptap surface in place of a native textarea, so the
// composer can grow inline chips (mentions, slash commands, attachments)
// without another rewrite. Today it is plain text only: no marks, no
// blocks, and the draft in and out is a string (see chatInputText.ts).
// Unmodified Enter submits, Shift+Enter breaks the line, an Enter chord
// does nothing (AGT-049). The box grows with its content up to maxHeight
// and then scrolls inside itself.
//
// Tiptap arrives only through import() (Stack: heavy dependencies never
// join the eager closure). Until the chunk lands a static look-alike of
// the box holds its place, so the layout never jumps.

import { cn } from "@pidash/kit";
import * as React from "react";

import { CHAT_INPUT_BLOCK_SEPARATOR, chatInputDoc, chatInputKeyAction } from "./chatInputText.js";

export interface ChatInputProps {
  /** The draft, as plain text with `\n` line breaks. */
  value: string;
  /** Fires with the new draft on every edit. */
  onChange: (value: string) => void;
  /** An unmodified Enter. The caller decides whether the draft can go. */
  onSubmit: () => void;
  /** Accessible name of the editing surface. */
  label: string;
  placeholder?: string | undefined;
  /** Locks the box: read-only and dimmed. */
  disabled?: boolean | undefined;
  /** Focus the end of the draft once the editor mounts. */
  autoFocus?: boolean | undefined;
  /** Growth cap in pixels; beyond it the draft scrolls inside the box. */
  maxHeight?: number | undefined;
  /** Classes for the box (padding, border, background, type). */
  className?: string | undefined;
}

interface FrameProps {
  className?: string | undefined;
  maxHeight?: number | undefined;
  disabled?: boolean | undefined;
  value: string;
  placeholder?: string | undefined;
  onMouseDown?: ((event: React.MouseEvent<HTMLDivElement>) => void) | undefined;
  children?: React.ReactNode;
}

/** The box shared by the live editor and its loading stand-in. */
function Frame({
  className,
  maxHeight,
  disabled,
  value,
  placeholder,
  onMouseDown,
  children,
}: FrameProps): React.ReactElement {
  return (
    <div
      data-chat-input=""
      data-draft={value}
      data-disabled={disabled === true ? "true" : undefined}
      onMouseDown={onMouseDown}
      style={maxHeight === undefined ? undefined : { maxHeight }}
      className={cn("text-body overflow-y-auto", disabled === true && "cursor-not-allowed opacity-60", className)}
    >
      <div className="relative">
        {placeholder !== undefined && value === "" ? (
          <div aria-hidden="true" className="pointer-events-none absolute inset-x-0 top-0 truncate text-(--text-muted)">
            {placeholder}
          </div>
        ) : null}
        {children}
      </div>
    </div>
  );
}

/** What the box shows while the editor chunk loads: the draft, inert. */
function ChatInputPlaceholder(props: ChatInputProps): React.ReactElement {
  return (
    <Frame {...props}>
      <div aria-hidden="true" className="min-h-[1lh] break-words whitespace-pre-wrap text-(--text)">
        {props.value}
      </div>
    </Frame>
  );
}

async function loadChatInput(): Promise<{ default: React.ComponentType<ChatInputProps> }> {
  const [{ EditorContent, useEditor }, { default: StarterKit }] = await Promise.all([
    import("@tiptap/react"),
    import("@tiptap/starter-kit"),
  ]);
  // Plain text: document, paragraph, text, hard break and undo history.
  // Pasted rich content collapses to its text because the schema has
  // nowhere else to put it.
  const extensions = [
    StarterKit.configure({
      blockquote: false,
      bold: false,
      bulletList: false,
      code: false,
      codeBlock: false,
      dropcursor: false,
      gapcursor: false,
      heading: false,
      horizontalRule: false,
      italic: false,
      listItem: false,
      orderedList: false,
      strike: false,
    }),
  ];

  function ChatInputEditor(props: ChatInputProps): React.ReactElement {
    const { value, label, placeholder, disabled = false, autoFocus = false } = props;
    // The editor is built once; everything it reads later goes through refs.
    const latest = React.useRef(props);
    latest.current = props;

    const editor = useEditor(
      {
        extensions,
        content: chatInputDoc(value),
        editable: !disabled,
        // Build in the mount effect, never during render (see RichTextEditor).
        immediatelyRender: false,
        autofocus: autoFocus ? "end" : false,
        editorProps: {
          attributes: () => {
            const current = latest.current;
            return {
              role: "textbox",
              "aria-multiline": "true",
              "aria-label": current.label,
              ...(current.placeholder === undefined ? {} : { "aria-placeholder": current.placeholder }),
              ...(current.disabled === true ? { "aria-disabled": "true" } : {}),
              class: "min-h-[1lh] break-words whitespace-pre-wrap text-(--text) outline-none",
            };
          },
          handleKeyDown: (view, event) => {
            const action = chatInputKeyAction({
              key: event.key,
              shiftKey: event.shiftKey,
              ctrlKey: event.ctrlKey,
              metaKey: event.metaKey,
              altKey: event.altKey,
              isComposing: event.isComposing || view.composing,
            });
            // Shift+Enter falls through to the hard-break keymap.
            if (action === "pass" || action === "newline") return false;
            event.preventDefault();
            if (action === "submit") latest.current.onSubmit();
            return true;
          },
        },
        onUpdate: ({ editor: current }) => {
          latest.current.onChange(current.getText({ blockSeparator: CHAT_INPUT_BLOCK_SEPARATOR }));
        },
      },
      []
    );

    // A draft set from outside (chip, dictation, clear after send) replaces
    // the document; the caret goes to its end.
    React.useEffect(() => {
      if (editor === null) return;
      if (editor.getText({ blockSeparator: CHAT_INPUT_BLOCK_SEPARATOR }) === value) return;
      editor.commands.setContent(chatInputDoc(value), false);
      editor.commands.setTextSelection(editor.state.doc.content.size - 1);
    }, [editor, value]);

    // Lock and unlock without emitting an edit; the attribute function
    // re-reads aria-disabled on the view update this causes.
    React.useEffect(() => {
      editor?.setEditable(!disabled, false);
    }, [editor, disabled]);

    // Label or placeholder changes refresh the surface's attributes.
    React.useEffect(() => {
      if (editor !== null) editor.view.setProps({});
    }, [editor, label, placeholder]);

    // One commit without an instance: hold the box with the stand-in.
    if (editor === null) return <ChatInputPlaceholder {...props} />;
    return (
      <Frame
        {...props}
        onMouseDown={(event) => {
          // A press on the box's padding focuses the draft's end, like a
          // textarea, instead of doing nothing.
          if (disabled || (event.target as HTMLElement).closest(".ProseMirror") !== null) return;
          event.preventDefault();
          editor.commands.focus("end");
        }}
      >
        <EditorContent editor={editor} />
      </Frame>
    );
  }

  return { default: ChatInputEditor };
}

/** Pure-annotated so the bundler can drop it from chunks that never render it. */
const LazyChatInput = /* #__PURE__ */ React.lazy(loadChatInput);

/** The chat composer's editing surface; suspends internally while Tiptap loads. */
export function ChatInput(props: ChatInputProps): React.ReactElement {
  return (
    <React.Suspense fallback={<ChatInputPlaceholder {...props} />}>
      <LazyChatInput {...props} />
    </React.Suspense>
  );
}
