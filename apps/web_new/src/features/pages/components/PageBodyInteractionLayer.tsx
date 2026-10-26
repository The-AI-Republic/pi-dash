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
import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
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
      const href = handle.linkHref();
      setLink((current) => (href === null ? { href: "", open: false } : { href, open: current.open }));
    };
    sync();
    document.addEventListener("selectionchange", sync);
    return () => document.removeEventListener("selectionchange", sync);
  }, [html, getHandle]);

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
      if (action === "Duplicate") handle.duplicateBlock();
      else handle.deleteBlock();
      onDocumentChange(handle.html());
    },
    [getHandle, onDocumentChange]
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

  return (
    <div data-testid="page-body-interactions" aria-label={PAGE_BODY_INTERACTION_LABEL} className="relative">
      <div className="pointer-events-none absolute top-0 -left-8 flex flex-col gap-(--space-1)">
        <div className="pointer-events-auto">
          <PageBlockHandle onSelect={blockAction} />
        </div>
      </div>
      {selectionActive || link.open ? (
        <div className="absolute -top-12 left-0 z-30">
          <PageSelectionBubble
            link={{ ...link, onApply: applyLink, onRemove: removeLink }}
            onCommand={runBubbleCommand}
            onToggleLink={() => setLink((current) => ({ ...current, open: !current.open }))}
          />
        </div>
      ) : null}
      {slashQuery !== null ? (
        <div className="absolute top-0 left-0 z-20">
          <PageSlashPalette query={slashQuery} onPick={pickEntry} onDismiss={() => setSlashQuery(null)} />
        </div>
      ) : null}
      {emojiOpen ? (
        <div className="absolute top-0 left-0 z-20">
          <PageEmojiPicker onPick={pickEmoji} />
        </div>
      ) : null}
      {picker === null ? null : (
        <div className="absolute top-0 left-0 z-20">
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
        <div className="absolute top-0 left-0 z-20">
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
      <span data-testid="page-emoji-count" hidden aria-hidden="true">
        {PAGE_BODY_EMOJIS.length}
      </span>
    </div>
  );
}
