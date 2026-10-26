// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The editable body's transient surfaces (parity/specs/pages.md
// §6.1–§6.4, PAGE-037–044). It watches the caret's text for the slash and
// @ triggers, opens the dragged block's menu, the emoji picker and the
// link editor, and forwards every choice to the editor handle. Keeping
// this out of the lazy editor means the Tiptap chunk gains no React state.

import * as React from "react";

import { searchMentionPeople } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import type { PageEditorHandle, PageHoveredBlock, PageTextRange } from "../lib/pageEditorAdapter.js";
import { pageAssistantBelowHtml } from "../lib/pageAssistant.js";
import { PAGE_BODY_EMOJIS, slashQueryAt, type PageBlockMenuAction, type PageCatalogEntry } from "../lib/pageBlocks.js";
import { mentionQueryAt } from "../../../shared/editor/mention.js";
import type { PageEmbedAttributes } from "../lib/pageBlocks.js";
import {
  searchPageEntityChoices,
  pageEmbedFor,
  pageEntityReferenceFor,
  type PageEntityChoice,
} from "../lib/pageEmbeds.js";
import {
  PageBlockHandle,
  PageEmojiPicker,
  PageEntityPicker,
  PageMentionSuggestions,
  PageSelectionBubble,
  PageSlashPalette,
  type PageMentionPerson,
} from "./PageBodyControls.js";
import { PageAssistantHandle, PageAssistantMenu } from "./PageAssistantMenu.js";

export const PAGE_BODY_INTERACTION_LABEL = "Page body actions";

export interface PageBodyInteractionLayerProps {
  workspaceSlug: string;
  projectId: string;
  getHandle: () => PageEditorHandle | null;
  /** Fires after each editor update, with the current HTML. */
  html: string;
  /** Open the file picker for the catalogue's Image entry. */
  onPickImage: () => void;
  /** Report a new document so the pane's asset rows refresh (PAGE-039). */
  onDocumentChange: (html: string) => void;
  /** Edition resolver test hook: embed kind this build may render. */
  embed?: PageEmbedAttributes | null;
  /**
   * The assistant handle may mount: the cloud flag, edit permission and
   * a configured model all hold (PAGE-075). The community build passes
   * false and renders no assistant surface.
   */
  assistantReady: boolean;
}

/** The link bubble and its prefill, keyed by selection change. */
interface LinkState {
  href: string;
  open: boolean;
}

export function PageBodyInteractionLayer({
  workspaceSlug,
  projectId,
  getHandle,
  html,
  onPickImage,
  onDocumentChange,
  embed = null,
  assistantReady,
}: PageBodyInteractionLayerProps): React.ReactElement {
  const [slashQuery, setSlashQuery] = React.useState<string | null>(null);
  const [emojiOpen, setEmojiOpen] = React.useState(false);
  const [mentionQuery, setMentionQuery] = React.useState<string | null>(null);
  const [mentionPeople, setMentionPeople] = React.useState<PageMentionPerson[]>([]);
  const [link, setLink] = React.useState<LinkState>({ href: "", open: false });
  const [selectionActive, setSelectionActive] = React.useState(false);
  const [picker, setPicker] = React.useState<"embed" | "entity" | null>(null);
  const [pickerQuery, setPickerQuery] = React.useState("");
  const [pickerChoices, setPickerChoices] = React.useState<PageEntityChoice[]>([]);
  const [anchor, setAnchor] = React.useState<{ top: number; left: number }>({ top: 0, left: 0 });
  const [handleOpen, setHandleOpen] = React.useState(false);
  // The assistant's snapshotted selection: text for the task prompt plus
  // the range the reply writes back to once the menu closes (PAGE-075).
  const [assistant, setAssistant] = React.useState<{ selection: string; range: PageTextRange } | null>(null);
  const handleRef = React.useRef<HTMLDivElement | null>(null);
  const bubbleRef = React.useRef<HTMLDivElement | null>(null);
  const [hoveredBlock, setHoveredBlock] = React.useState<PageHoveredBlock | null>(null);

  // The triggers live in the text before the caret; the layer reads them
  // after every editor update rather than parsing the document HTML.
  React.useEffect(() => {
    const sync = (): void => {
      const handle = getHandle();
      if (handle === null) return;
      const before = handle.textBeforeCaret();
      setSlashQuery(slashQueryAt(before));
      setMentionQuery(mentionQueryAt(before)?.text ?? null);
      setSelectionActive(handle.hasSelection());
      // The surfaces open beside the caret, measured against the editor's
      // own box, so they never fall to the end of a long body (PAGE-037).
      const caret = handle.caretAnchor();
      if (caret !== null) {
        setAnchor({ top: Math.max(0, Math.round(caret.top)), left: Math.max(0, Math.round(caret.left)) });
      }
      const href = handle.linkHref();
      setLink((current) => (href === null ? { href: "", open: false } : { href, open: current.open }));
    };
    sync();
    document.addEventListener("selectionchange", sync);
    return () => document.removeEventListener("selectionchange", sync);
  }, [html, getHandle]);

  // The drag handle follows the pointer, so its actions always target the
  // block being hovered rather than whichever block holds the caret
  // (PAGE-037). The handle itself sits in the gutter outside the editor's
  // box, where the editor cannot place a point, so a pointer over the
  // handle or its open menu keeps the block it was opened for.
  React.useEffect(() => {
    const onPointerMove = (event: MouseEvent): void => {
      const handle = getHandle();
      const block = handle?.blockAtPoint(event.clientX, event.clientY) ?? null;
      if (block !== null) {
        setHoveredBlock(block);
        return;
      }
      const surface = handleRef.current;
      if (surface !== null && event.target instanceof Node && surface.contains(event.target)) return;
      setHoveredBlock(null);
      setHandleOpen(false);
    };
    document.addEventListener("mousemove", onPointerMove);
    return () => document.removeEventListener("mousemove", onPointerMove);
  }, [getHandle]);

  // A click inside a link moves the caret into it without selecting text,
  // so the bubble's own Link control never appeared. Clicking a link opens
  // its editable fields in place instead (PAGE-043); a real text selection
  // still gets the formatting bubble with its manual Link toggle.
  React.useEffect(() => {
    const onClick = (event: MouseEvent): void => {
      const handle = getHandle();
      if (handle === null) return;
      // The bubble's own controls toggle it; their clicks must not reopen
      // the fields they just closed.
      const bubble = bubbleRef.current;
      if (bubble !== null && event.target instanceof Node && bubble.contains(event.target)) return;
      const href = handle.linkHref();
      if (href === null || handle.hasSelection()) return;
      setLink({ href, open: true });
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, [getHandle]);

  // Escape dismisses whichever transient surface is open — the slash
  // palette, emoji picker, mention popup, entity/embed picker, the
  // block-actions menu or the assistant panel — without touching the
  // document (PAGE-037/042/075).
  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      const anyOpen =
        slashQuery !== null ||
        emojiOpen ||
        mentionQuery !== null ||
        picker !== null ||
        handleOpen ||
        assistant !== null;
      if (!anyOpen) return;
      event.preventDefault();
      setSlashQuery(null);
      setEmojiOpen(false);
      setMentionQuery(null);
      setMentionPeople([]);
      setPicker(null);
      setPickerChoices([]);
      setHandleOpen(false);
      setAssistant(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [slashQuery, emojiOpen, mentionQuery, picker, handleOpen, assistant]);

  // Mention suggestions are project-scoped and debounced by React Query's
  // own cache; an empty query still lists the project's members.
  React.useEffect(() => {
    if (mentionQuery === null) {
      setMentionPeople([]);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      void searchMentionPeople(getAppClient(), workspaceSlug, { query: mentionQuery, projectId, count: 5 })
        .then((people) => {
          if (!live) return;
          setMentionPeople(
            people.map((person) => ({
              id: person.member__id,
              name: person.member__display_name,
              avatarUrl: person.member__avatar_url,
            }))
          );
        })
        .catch(() => {
          if (live) setMentionPeople([]);
        });
    }, 120);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [mentionQuery, workspaceSlug, projectId]);

  // The picker lists project work items; its read is debounced like the
  // mention search, and its empty query lists the project's first page.
  React.useEffect(() => {
    if (picker === null) {
      setPickerChoices([]);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      void searchPageEntityChoices(getAppClient(), workspaceSlug, projectId, pickerQuery, ["issue"])
        .then((choices) => {
          if (live) setPickerChoices(choices);
        })
        .catch(() => {
          if (live) setPickerChoices([]);
        });
    }, 120);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [picker, pickerQuery, workspaceSlug, projectId]);

  const pickEntry = React.useCallback(
    (entry: PageCatalogEntry) => {
      const handle = getHandle();
      setSlashQuery(null);
      if (handle === null) return;
      // The palette only exists for a live query, so that query is always
      // erased before the chosen entry's effect lands in the body.
      handle.deleteBeforeCaret((slashQuery ?? "").length + 1);
      // A cloud registry snippet inserts its markup where the trigger was
      // (PAGE-078); the community catalogue carries no such entries. The
      // palette only exists at a collapsed caret, which carries no
      // selection range, so the snippet lands at the caret position.
      if (entry.action.type === "snippet") {
        const caret = handle.caretPosition();
        if (caret === null) return;
        handle.insertAt(caret, entry.action.html);
        onDocumentChange(handle.html());
        return;
      }
      if (entry.action.type === "block" && (entry.action.block === "embed" || entry.action.block === "entity")) {
        setPicker(entry.action.block);
        setPickerQuery("");
        return;
      }
      if (entry.action.type === "color") {
        if (entry.action.key === null) handle.clearColor(entry.action.kind);
        else handle.applyColor(entry.action.kind, entry.action.key);
        onDocumentChange(handle.html());
        return;
      }
      if (entry.action.type === "command") {
        const command = entry.action.command;
        if (command === "inlineCode") handle.applyCommand("code");
        else if (command === "underline") handle.applyCommand("underline");
        else if (command === "alignLeft") handle.applyCommand("alignLeft");
        else if (command === "alignCenter") handle.applyCommand("alignCenter");
        else if (command === "alignRight") handle.applyCommand("alignRight");
        else if (command === "link") setLink((current) => ({ ...current, open: true }));
        else {
          handle.insertText("@");
          setMentionQuery("");
        }
        onDocumentChange(handle.html());
        return;
      }
      if (entry.action.block === "image") {
        onPickImage();
        return;
      }
      if (entry.action.block === "emoji") {
        setEmojiOpen(true);
        return;
      }
      handle.applyCatalogBlock(entry.action.block);
      onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange, onPickImage, slashQuery]
  );

  const pickEntity = React.useCallback(
    (choice: PageEntityChoice) => {
      const handle = getHandle();
      if (handle === null || picker === null) return;
      if (picker === "embed") handle.insertEmbed(pageEmbedFor(choice, workspaceSlug));
      else handle.insertEntityReference(pageEntityReferenceFor(choice, workspaceSlug));
      setPicker(null);
      setPickerChoices([]);
      onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange, picker, workspaceSlug]
  );

  const pickEmoji = React.useCallback(
    (glyph: string) => {
      getHandle()?.insertEmoji(glyph);
      setEmojiOpen(false);
      const handle = getHandle();
      if (handle !== null) onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange]
  );

  const pickMention = React.useCallback(
    (person: PageMentionPerson) => {
      const handle = getHandle();
      if (handle !== null && mentionQuery !== null) handle.deleteBeforeCaret(mentionQuery.length + 1);
      handle?.insertMention({ id: person.id, name: person.name });
      setMentionQuery(null);
      setMentionPeople([]);
      if (handle !== null) onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange, mentionQuery]
  );

  const blockAction = React.useCallback(
    (action: PageBlockMenuAction) => {
      const handle = getHandle();
      if (handle === null) return;
      const target = hoveredBlock;
      if (action === "Duplicate") handle.duplicateBlock(target);
      else if (action === "Delete") handle.deleteBlock(target);
      else if (target !== null) handle.moveBlock(target, action === "Move up" ? "up" : "down");
      setHandleOpen(false);
      onDocumentChange(handle.html());
    },
    [getHandle, hoveredBlock, onDocumentChange]
  );

  const applyLink = React.useCallback(
    (href: string) => {
      const handle = getHandle();
      if (handle === null) return;
      handle.setLink(href);
      setLink({ href, open: false });
      onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange]
  );

  const removeLink = React.useCallback(() => {
    const handle = getHandle();
    if (handle === null) return;
    handle.unsetLink();
    setLink({ href: "", open: false });
    onDocumentChange(handle.html());
  }, [getHandle, onDocumentChange]);

  const runBubbleCommand = React.useCallback(
    (command: Parameters<PageEditorHandle["applyCommand"]>[0]) => {
      const handle = getHandle();
      if (handle === null) return;
      handle.applyCommand(command);
      onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange]
  );

  // Snapshot the selection for one assistant turn: the menu reads the
  // focus, so the live selection would be gone by the time the reply
  // lands. An empty selection opens nothing (PAGE-075).
  const askAssistant = React.useCallback(() => {
    const handle = getHandle();
    if (handle === null) return;
    const range = handle.selectionRange();
    const selection = handle.selectedText();
    if (range === null || selection === "") return;
    setAssistant({ selection, range });
  }, [getHandle]);

  const closeAssistant = React.useCallback(() => {
    setAssistant(null);
  }, []);

  // The reply writes through the snapshotted range, then the panel
  // unmounts and its task state resets with it (PAGE-075). The snapshot
  // rides as an argument because the menu only renders while one exists.
  const applyAssistant = React.useCallback(
    (snapshot: { range: PageTextRange }, html: string) => {
      const handle = getHandle();
      if (handle === null) return;
      handle.insertAt(snapshot.range, html);
      onDocumentChange(handle.html());
      setAssistant(null);
    },
    [getHandle, onDocumentChange]
  );

  const appendAssistant = React.useCallback(
    (snapshot: { range: PageTextRange }, html: string) => {
      const handle = getHandle();
      if (handle === null) return;
      handle.insertAt(snapshot.range.to, pageAssistantBelowHtml(html));
      onDocumentChange(handle.html());
      setAssistant(null);
    },
    [getHandle, onDocumentChange]
  );

  // Cloud slash-catalogue entries, after the community set (PAGE-078);
  // the community edition leaves the registry absent and adds nothing.
  const registryEntries: PageCatalogEntry[] = (edition.slots?.["page.slashRegistry"]?.entries ?? []).map((entry) => ({
    label: entry.label,
    group: entry.group,
    action: { type: "snippet", html: entry.html },
  }));

  // The surfaces sit in their own zero-height layer beside the body, never
  // inside the measured `.editor-container`: the pane's live read counts
  // that container's blocks, so palette rows must not become paragraphs.
  // They are positioned in viewport coordinates, so a long body's caret
  // still gets its palette beside it rather than at the document's end.
  const shown = hoveredBlock !== null || handleOpen;
  const popover = { position: "fixed", top: anchor.top + 24, left: anchor.left } as const;
  return (
    <div data-testid="page-body-interactions" aria-label={PAGE_BODY_INTERACTION_LABEL} className="pointer-events-none">
      {shown ? (
        <div
          ref={handleRef}
          className="pointer-events-auto fixed z-20"
          style={{ top: (hoveredBlock?.top ?? 0) - 4, left: (hoveredBlock?.left ?? 0) - 32 }}
        >
          <PageBlockHandle onSelect={blockAction} open={handleOpen} onOpenChange={setHandleOpen} />
          {assistantReady ? <PageAssistantHandle onAsk={askAssistant} /> : null}
        </div>
      ) : null}
      {assistant === null ? null : (
        <div className="pointer-events-auto z-30" style={popover}>
          <PageAssistantMenu
            workspaceSlug={workspaceSlug}
            selection={assistant.selection}
            onApply={(html) => applyAssistant(assistant, html)}
            onAppend={(html) => appendAssistant(assistant, html)}
            onClose={closeAssistant}
          />
        </div>
      )}
      {selectionActive || link.open ? (
        <div
          ref={bubbleRef}
          className="pointer-events-auto fixed z-30"
          style={{ top: Math.max(0, anchor.top - 48), left: anchor.left }}
        >
          <PageSelectionBubble
            link={{ ...link, onApply: applyLink, onRemove: removeLink }}
            onCommand={runBubbleCommand}
            onToggleLink={() => setLink((current) => ({ ...current, open: !current.open }))}
          />
        </div>
      ) : null}
      {slashQuery !== null ? (
        <div className="pointer-events-auto z-20" style={popover}>
          <PageSlashPalette
            query={slashQuery}
            onPick={pickEntry}
            onDismiss={() => setSlashQuery(null)}
            extraEntries={registryEntries}
          />
        </div>
      ) : null}
      {emojiOpen ? (
        <div className="pointer-events-auto z-20" style={popover}>
          <PageEmojiPicker onPick={pickEmoji} onDismiss={() => setEmojiOpen(false)} />
        </div>
      ) : null}
      {picker === null ? null : (
        <div className="pointer-events-auto z-20" style={popover}>
          <PageEntityPicker
            state={{
              query: pickerQuery,
              choices: pickerChoices,
              onQueryChange: setPickerQuery,
              onPick: pickEntity,
              onDismiss: () => setPicker(null),
            }}
          />
        </div>
      )}
      {mentionQuery !== null ? (
        <div className="pointer-events-auto z-20" style={popover}>
          <PageMentionSuggestions
            state={{
              query: mentionQuery,
              people: mentionPeople,
              open: mentionQuery !== null,
              onPick: pickMention,
              onDismiss: () => setMentionQuery(null),
            }}
          />
        </div>
      ) : null}
      {embed === null ? null : <span data-testid="page-embed-slot" hidden aria-hidden="true" />}
      {assistantReady ? <span data-testid="page-assistant-mount" hidden aria-hidden="true" /> : null}
      <span data-testid="page-emoji-count" hidden aria-hidden="true">
        {PAGE_BODY_EMOJIS.length}
      </span>
    </div>
  );
}
