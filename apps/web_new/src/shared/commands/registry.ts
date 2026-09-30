// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command registry (Architecture > shared/commands). Screens register the
// commands they own while mounted; the palette reads a snapshot. Shortcuts
// are single chords ("mod+k", "mod+shift+p"); sequences arrive with Power K.

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

/** Snapshot of every registered command, sorted by section then title. */
export function listCommands(): PidashCommand[] {
  return [...commands.values()].sort(
    (a, b) => (a.section ?? "General").localeCompare(b.section ?? "General") || a.title.localeCompare(b.title)
  );
}

export function findCommand(id: string): PidashCommand | undefined {
  return commands.get(id);
}

/** Drop every registration. Sign-out and tests only. */
export function clearCommands(): void {
  commands.clear();
}

interface Chord {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

/** Parse "mod+shift+k" into its parts. Returns null when malformed. */
export function parseShortcut(shortcut: string): Chord | null {
  const parts = shortcut
    .toLowerCase()
    .split("+")
    .map((part) => part.trim());
  const key = parts.pop();
  if (!key || key.length !== 1) return null;
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
