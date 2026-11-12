// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Command registry (Architecture > shared/commands). Screens register the
// commands they own while mounted; the palette reads a snapshot. Shortcuts
// are single chords ("mod+k", "mod+shift+p"); two-key sequences ("op", "gh")
// fire globally outside inputs (shell spec, SHELL-086/087). Sub-pages
// (entity pickers, preference lists) register beside their commands and
// render through the palette's generic page view (shell spec, SHELL-082).

import type * as React from "react";

/**
 * Contextual marker (shell spec, SHELL-090/091). Commands registered by
 * an entity screen carry the entity family plus the chip label and
 * subject name. The palette filters marked commands out while the
 * context is dismissed and renders the chip from the first marked
 * command.
 */
export interface PaletteCommandContext {
  /** Entity family that registered the command. */
  kind: "work-item" | "cycle" | "module" | "page";
  /** Short entity label for the chip ("Work item"). */
  indicator: string;
  /** Subject name for the chip (route key or entity name). */
  subject: string;
}

export interface PidashCommand {
  /** Stable id, namespaced by owner ("shell.sign-out", "issues.go-to-list"). */
  id: string;
  /** Shown in the palette. */
  title: string;
  /** Secondary hint, e.g. where the command navigates. */
  hint?: string | undefined;
  /** Group heading in the palette. Defaults to "General". */
  section?: string | undefined;
  /** Single-chord shortcut, e.g. "mod+k". Rendered as a Kbd hint. */
  shortcut?: string | undefined;
  /**
   * Two-key sequence fired globally outside inputs, e.g. "op" opens the
   * project picker (SHELL-086/087). Lowercase letters; matched
   * case-insensitively. A command carries at most one of `shortcut` and
   * `sequence` today; the shortcuts dialog lists either (SHELL-094).
   */
  sequence?: string | undefined;
  /**
   * Alternate match terms for the blank-query substring filter (SHELL-085),
   * e.g. ["logout", "exit"] for sign-out. Never shown, only matched.
   */
  keywords?: string[] | undefined;
  /**
   * Sub-page to open when the command activates, instead of running `run`.
   * The page id must be registered with registerPalettePages by the same
   * owner; sibling palette slices register their pickers on top of this.
   */
  page?: string | undefined;
  /** Set when an entity screen registered the command for its subject. */
  context?: PaletteCommandContext | undefined;
  run: () => void | Promise<void>;
}

const commands = new Map<string, PidashCommand>();

/**
 * Register commands for the lifetime of the caller. Returns an unregister
 * function; React screens call it as their effect cleanup.
 */
export function registerCommands(entries: PidashCommand[]): () => void {
  for (const entry of entries) {
    commands.set(entry.id, entry);
  }
  return () => {
    for (const entry of entries) {
      if (commands.get(entry.id) === entry) {
        commands.delete(entry.id);
      }
    }
  };
}

/**
 * Palette section order (shell spec, SHELL-087): contextual groups first,
 * help last. Matched case-insensitively against the display headings;
 * unknown sections sort alphabetically after the pinned ones.
 */
const SECTION_ORDER = [
  "contextual",
  "create",
  "navigate",
  "general",
  "settings",
  "account",
  "miscellaneous",
  "preferences",
  "help",
];

function sectionRank(section: string): number {
  const rank = SECTION_ORDER.indexOf(section.toLowerCase());
  return rank === -1 ? SECTION_ORDER.length : rank;
}

/**
 * Snapshot of every registered command: context-marked commands first
 * (SHELL-087/090: the entity group appears ahead of the generic groups),
 * then palette section order, then title.
 */
export function listCommands(): PidashCommand[] {
  return [...commands.values()].sort(
    (a, b) =>
      Number(b.context !== undefined) - Number(a.context !== undefined) ||
      sectionRank(a.section ?? "General") - sectionRank(b.section ?? "General") ||
      (a.section ?? "General").localeCompare(b.section ?? "General") ||
      a.title.localeCompare(b.title)
  );
}

export function findCommand(id: string): PidashCommand | undefined {
  return commands.get(id);
}

/**
 * The registered command bound to a two-key sequence, matched
 * case-insensitively (SHELL-086/087). Undefined when no live command
 * claims the sequence — unregistered entries never fire (SHELL-092).
 */
export function findCommandBySequence(sequence: string): PidashCommand | undefined {
  const needle = sequence.toLowerCase();
  for (const command of commands.values()) {
    if (command.sequence !== undefined && command.sequence.toLowerCase() === needle) {
      return command;
    }
  }
  return undefined;
}

/**
 * Every registered command with a chord or a sequence, in palette order.
 * The shortcuts dialog lists exactly these (SHELL-094).
 */
export function listBoundCommands(): PidashCommand[] {
  return listCommands().filter((command) => command.shortcut !== undefined || command.sequence !== undefined);
}

/** Drop every registration. Sign-out and tests only. */
export function clearCommands(): void {
  commands.clear();
}

/** One activatable row on a palette sub-page (a picker hit, an option). */
export interface PalettePageOption {
  /** Stable id within the page. */
  id: string;
  /** Shown in the palette. */
  title: string;
  /** Secondary hint, e.g. the entity's project. */
  hint?: string | undefined;
  /** Marks the current selection (check shown; pickers re-read on refresh). */
  selected?: boolean | undefined;
  /**
   * Decorative mark ahead of the title (a status dot, an entity glyph).
   * Navigation lists use it to show an entity's state without repeating
   * it in words; selection pickers leave it out.
   */
  icon?: React.ReactNode | undefined;
  /**
   * Runs on activation. The page stays open afterwards unless the option
   * closes it through the palette store itself, so multi-change pickers
   * (SHELL-090) keep their state across picks.
   */
  run: () => void | Promise<void>;
}

/** What a sub-page shows for the current query (shell spec, SHELL-093). */
export interface PalettePageState {
  /** "loading" spins; "ready" lists options or the empty line. Failures
   * surface as ready-with-no-options, never as an error (SHELL-093). */
  status: "loading" | "ready";
  /** Options matching the query, in display order. */
  options: PalettePageOption[];
  /** Plain line shown when ready with zero options, e.g. "No labels found". */
  emptyText: string;
}

/**
 * A palette sub-page: an entity picker or option list behind a command.
 * The owner registers the page beside its command and filters/loads inside
 * usePageState with its own data hooks; the palette renders the result
 * through the generic page view (spinner, options, empty line).
 */
export interface PalettePage {
  /** Stable id, namespaced by owner ("issues.project-picker"). */
  id: string;
  /** Input placeholder while on this page; differs from the root prompt. */
  placeholder: string;
  usePageState: (query: string) => PalettePageState;
}

const pages = new Map<string, PalettePage>();

/**
 * Register sub-pages for the lifetime of the caller. Returns an unregister
 * function; React screens call it as their effect cleanup.
 */
export function registerPalettePages(entries: PalettePage[]): () => void {
  for (const entry of entries) {
    pages.set(entry.id, entry);
  }
  return () => {
    for (const entry of entries) {
      if (pages.get(entry.id) === entry) {
        pages.delete(entry.id);
      }
    }
  };
}

export function findPalettePage(id: string): PalettePage | undefined {
  return pages.get(id);
}

/** Drop every page registration. Sign-out and tests only. */
export function clearPalettePages(): void {
  pages.clear();
}

/**
 * Substring filter over page option titles plus hints (SHELL-085/087).
 * A blank query returns the options unchanged.
 */
export function filterPageOptions(options: PalettePageOption[], query: string): PalettePageOption[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...options];
  return options.filter(
    (option) => option.title.toLowerCase().includes(needle) || (option.hint ?? "").toLowerCase().includes(needle)
  );
}

/**
 * Substring filter over command titles plus alternate keywords (SHELL-085).
 * The hint (navigation destination) matches too; a blank query returns
 * the snapshot unchanged.
 */
export function filterCommands(commands: PidashCommand[], query: string): PidashCommand[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [...commands];
  return commands.filter(
    (command) =>
      command.title.toLowerCase().includes(needle) ||
      (command.hint ?? "").toLowerCase().includes(needle) ||
      (command.keywords ?? []).some((keyword) => keyword.toLowerCase().includes(needle))
  );
}

interface Chord {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

/**
 * Named non-printable keys a chord may end in (matched against
 * KeyboardEvent.key case-insensitively, e.g. "mod+backspace" for the
 * work-item Delete entry, SHELL-090).
 */
const NAMED_KEYS = new Set([
  "backspace",
  "delete",
  "enter",
  "escape",
  "tab",
  "arrowup",
  "arrowdown",
  "arrowleft",
  "arrowright",
]);

/**
 * Parse "mod+shift+k" into its parts. The key is one printable
 * character or a named non-printable key; anything else is malformed
 * and returns null.
 */
export function parseShortcut(shortcut: string): Chord | null {
  const parts = shortcut
    .toLowerCase()
    .split("+")
    .map((part) => part.trim());
  const key = parts.pop();
  if (!key || (key.length !== 1 && !NAMED_KEYS.has(key))) return null;
  const rest = new Set(parts);
  if (rest.size !== parts.length) return null;
  for (const part of rest) {
    if (part !== "mod" && part !== "shift" && part !== "alt" && part !== "ctrl") return null;
  }
  return {
    mod: rest.has("mod") || rest.has("ctrl"),
    shift: rest.has("shift"),
    alt: rest.has("alt"),
    key,
  };
}

/**
 * Whether a keyboard event fires the chord. "mod" is Cmd on macOS and Ctrl
 * elsewhere; comparison is case-insensitive so "K" still matches "mod+k".
 */
export function matchesShortcut(
  event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; altKey: boolean },
  shortcut: string
): boolean {
  const chord = parseShortcut(shortcut);
  if (!chord) return false;
  const mod = event.ctrlKey || event.metaKey;
  return (
    mod === chord.mod &&
    event.shiftKey === chord.shift &&
    event.altKey === chord.alt &&
    event.key.toLowerCase() === chord.key
  );
}
