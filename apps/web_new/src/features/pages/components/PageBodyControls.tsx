// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body interaction surfaces (parity/specs/pages.md §6.1–§6.4,
// PAGE-037–044): the slash palette, the hovered block's drag handle, the
// inline emoji picker, the person-mention popup and the in-place link
// bubble. They render beside the editor and only call the command handle,
// so the editor itself stays about the document.

import { Avatar } from "@pidash/kit";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  GripVertical,
  Italic,
  Link2,
  Strikethrough,
  Underline,
} from "lucide-react";
import * as React from "react";

import type { PageFormatCommand } from "../../../shared/editor/pageFormatting.js";

import {
  filterSlashEntries,
  PAGE_BLOCK_MENU_LABELS,
  PAGE_BODY_EMOJIS,
  PAGE_CATALOG_GROUPS,
  type PageBlockMenuAction,
  type PageCatalogEntry,
} from "../lib/pageBlocks.js";

export const PAGE_SLASH_LABEL = "Insert block";
export const PAGE_SLASH_PLACEHOLDER = "Search blocks";
export const PAGE_DRAG_LABEL = "Block actions";
export const PAGE_EMOJI_LABEL = "Insert emoji";
export const PAGE_MENTION_LABEL = "Mention suggestions";
export const PAGE_MENTION_USERS = "Users";
export const PAGE_LINK_LABEL = "Link";
export const PAGE_LINK_PLACEHOLDER = "Paste a link";
export const PAGE_LINK_APPLY = "Apply link";
export const PAGE_LINK_REMOVE = "Remove link";

/** One mention suggestion, already mapped from the API shape. */
export interface PageMentionPerson {
  id: string;
  name: string;
  avatarUrl: string | null;
}

export interface PageMentionState {
  query: string;
  people: readonly PageMentionPerson[];
  open: boolean;
  onPick: (person: PageMentionPerson) => void;
  onDismiss: () => void;
}

export interface PageLinkState {
  href: string;
  open: boolean;
  onApply: (href: string) => void;
  onRemove: () => void;
}

/** The palette: one filtered list grouped by the catalogue's headings. */
export function PageSlashPalette({
  query,
  onPick,
  onDismiss,
}: {
  query: string;
  onPick: (entry: PageCatalogEntry) => void;
  onDismiss: () => void;
}): React.ReactElement {
  const entries = filterSlashEntries(query);
  return (
    <div
      id="slash-command"
      data-testid="page-slash-palette"
      className="shadow-popover z-20 flex max-h-80 w-64 flex-col overflow-y-auto rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
    >
      <p className="text-caption px-(--space-2) py-(--space-1) font-medium text-(--text-muted)">{PAGE_SLASH_LABEL}</p>
      {entries.length === 0 ? (
        <p className="text-body px-(--space-2) py-(--space-2) text-(--text-muted)">No blocks match.</p>
      ) : (
        groupEntries(entries).map(([group, rows]) => (
          <div key={group} className="flex flex-col">
            <p className="text-caption px-(--space-2) py-(--space-1) font-medium text-(--text-muted)">{group}</p>
            {rows.map((entry, index) => (
              <button
                key={`${group}-${entry.label}-${index}`}
                type="button"
                onMouseDown={(event) => {
                  event.preventDefault();
                  onPick(entry);
                }}
                className="text-body flex w-full items-center rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
              >
                <p>{entry.label}</p>
              </button>
            ))}
          </div>
        ))
      )}
      <button type="button" className="sr-only" tabIndex={-1} onClick={onDismiss} aria-label="Close block menu" />
    </div>
  );
}

/** The inline selection bubble's commands, in the oracle's reading order. */
export const PAGE_BUBBLE_COMMANDS: readonly {
  command: PageFormatCommand;
  label: string;
  Icon: React.ComponentType<{ size?: number; strokeWidth?: number }>;
}[] = [
  { command: "bold", label: "Bold", Icon: Bold },
  { command: "italic", label: "Italic", Icon: Italic },
  { command: "underline", label: "Underline", Icon: Underline },
  { command: "strike", label: "Strikethrough", Icon: Strikethrough },
  { command: "alignLeft", label: "Left align", Icon: AlignLeft },
  { command: "alignCenter", label: "Center align", Icon: AlignCenter },
  { command: "alignRight", label: "Right align", Icon: AlignRight },
];

/** The floating formatting/link control shown while editing a selection. */
export function PageSelectionBubble({
  link,
  onCommand,
  onToggleLink,
}: {
  link: PageLinkState;
  onCommand: (command: PageFormatCommand) => void;
  onToggleLink: () => void;
}): React.ReactElement {
  return (
    <div className="horizontal-scrollbar shadow-popover absolute top-0 left-0 z-30 flex max-w-full flex-col gap-(--space-1) rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)">
      <div className="flex items-center gap-px">
        {PAGE_BUBBLE_COMMANDS.map(({ command, label, Icon }) => (
          <button
            key={command}
            type="button"
            aria-label={label}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => onCommand(command)}
            className="grid size-7 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            <Icon size={14} strokeWidth={1.75} aria-hidden="true" />
          </button>
        ))}
        <button
          type="button"
          onMouseDown={(event) => event.preventDefault()}
          onClick={onToggleLink}
          className="text-body flex h-7 items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <Link2 size={14} strokeWidth={1.75} aria-hidden="true" />
          {PAGE_LINK_LABEL}
        </button>
      </div>
      <PageLinkBubble state={link} />
    </div>
  );
}

function groupEntries(entries: readonly PageCatalogEntry[]): [string, PageCatalogEntry[]][] {
  const groups = new Map<string, PageCatalogEntry[]>();
  for (const entry of entries) {
    const rows = groups.get(entry.group) ?? [];
    rows.push(entry);
    groups.set(entry.group, rows);
  }
  return [...groups.entries()];
}

/** The hover drag handle and its Delete/Duplicate menu. */
export function PageBlockHandle({ onSelect }: { onSelect: (action: PageBlockMenuAction) => void }): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <div className="relative">
      <button
        id="drag-handle"
        type="button"
        aria-label={PAGE_DRAG_LABEL}
        aria-expanded={open}
        data-testid="page-block-handle"
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen((value) => !value)}
        className="grid size-6 place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
      >
        <GripVertical size={14} strokeWidth={1.75} aria-hidden="true" />
      </button>
      {open ? (
        <div
          role="menu"
          className="shadow-popover absolute top-6 left-0 z-30 flex w-32 flex-col rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
        >
          {PAGE_BLOCK_MENU_LABELS.map((label) => (
            <button
              key={label}
              type="button"
              role="menuitem"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                setOpen(false);
                onSelect(label);
              }}
              className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
            >
              {label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/** The inline body emoji picker (PAGE-042). */
export function PageEmojiPicker({ onPick }: { onPick: (glyph: string) => void }): React.ReactElement {
  return (
    <div
      data-testid="page-emoji-picker"
      aria-label={PAGE_EMOJI_LABEL}
      className="shadow-popover z-20 grid w-64 grid-cols-8 gap-px rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-2)"
    >
      {PAGE_BODY_EMOJIS.map((glyph) => (
        <button
          key={glyph}
          id={`emoji-item-${PAGE_BODY_EMOJIS.indexOf(glyph)}`}
          type="button"
          onMouseDown={(event) => {
            event.preventDefault();
            onPick(glyph);
          }}
          className="text-body grid size-7 place-items-center rounded-(--radius-control) hover:bg-(--subtle)"
        >
          {glyph}
        </button>
      ))}
    </div>
  );
}

/** The person suggestion popup, with the custom avatar-and-name rows. */
export function PageMentionSuggestions({ state }: { state: PageMentionState }): React.ReactElement | null {
  if (!state.open || state.people.length === 0) return null;
  return (
    <div
      data-testid="page-mention-suggestions"
      className="shadow-popover z-20 flex w-64 flex-col rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-1)"
    >
      <h6 className="text-caption px-(--space-2) py-(--space-1) font-medium text-(--text-muted)">
        {PAGE_MENTION_USERS}
      </h6>
      <ul aria-label={PAGE_MENTION_LABEL} className="flex flex-col gap-px">
        {state.people.map((person, index) => (
          <li key={person.id}>
            <button
              id={`mention-item-${index}`}
              type="button"
              onMouseDown={(event) => {
                event.preventDefault();
                state.onPick(person);
              }}
              className="text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
            >
              <Avatar
                name={person.name}
                {...(person.avatarUrl === null ? {} : { src: person.avatarUrl })}
                size="small"
              />
              <span className="min-w-0 flex-1 truncate">{person.name}</span>
            </button>
          </li>
        ))}
      </ul>
      <button type="button" className="sr-only" tabIndex={-1} onClick={state.onDismiss} aria-label="Dismiss mentions" />
    </div>
  );
}

/** In-place link editing (PAGE-043). */
export function PageLinkBubble({ state }: { state: PageLinkState }): React.ReactElement | null {
  const [value, setValue] = React.useState(state.href);
  React.useEffect(() => setValue(state.href), [state.href]);
  if (!state.open) return null;
  return (
    <div className="flex w-72 max-w-full flex-col gap-(--space-2) p-(--space-1)" data-testid="page-link-bubble">
      <input
        type="url"
        aria-label={PAGE_LINK_PLACEHOLDER}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            state.onApply(value);
          }
        }}
        placeholder={PAGE_LINK_PLACEHOLDER}
        className="text-body w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text)"
      />
      <div className="flex items-center gap-(--space-1)">
        <button
          type="button"
          onClick={() => state.onApply(value)}
          className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-2) py-(--space-1) font-medium text-(--on-accent)"
        >
          {PAGE_LINK_APPLY}
        </button>
        <button
          type="button"
          onClick={state.onRemove}
          className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
        >
          {PAGE_LINK_REMOVE}
        </button>
      </div>
    </div>
  );
}

/** The catalogue's media entry index, for tests asserting order. */
export const PAGE_SLASH_MEDIA_LABELS = ["Image", "Callout", "Divider", "Emoji"] as const;
export { PAGE_CATALOG_GROUPS };
