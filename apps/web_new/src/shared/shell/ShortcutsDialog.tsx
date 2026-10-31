// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shortcut reference dialog (shell spec, SHELL-094). Lists every bound
// command — chords and two-key sequences — grouped by family with a
// name filter and platform-adapted glyphs. Openable from the help
// command, its own chord, or the sidebar help menu (NEWFRONT-287 wires
// that menu to the store action). DOM contract for the area-gate driver
// (NEWFRONT-295): role=dialog titled "Keyboard shortcuts"; the filter
// input placeholder is "Search for shortcuts"; family headings are plain
// text; rows show the command title beside its binding chips.

import { Dialog, Input, Kbd } from "@pidash/kit";
import * as React from "react";

import { listBoundCommands, type PidashCommand } from "../commands/index.js";
import { SequenceBadge } from "./paletteRows.js";
import { useShortcutsDialog } from "./shortcutsDialogStore.js";

/** True on macOS, where the mod key renders as ⌘. */
export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined" || typeof navigator.platform !== "string") return false;
  return navigator.platform.toUpperCase().includes("MAC");
}

/**
 * Display glyphs for one chord ("mod+shift+c" → ["⌘", "⇧", "C"] on macOS,
 * ["Ctrl", "Shift", "C"] elsewhere). Unknown parts render uppercased, so a
 * malformed chord still shows something instead of vanishing (SHELL-094).
 */
export function formatChordKeys(shortcut: string, mac: boolean): string[] {
  return shortcut.split("+").map((part) => {
    const raw = part.trim();
    const key = raw.toLowerCase();
    switch (key) {
      case "mod":
      case "meta":
      case "cmd":
        return mac ? "⌘" : "Ctrl";
      case "ctrl":
        return mac ? "⌃" : "Ctrl";
      case "alt":
      case "option":
        return mac ? "⌥" : "Alt";
      case "shift":
        return mac ? "⇧" : "Shift";
      case "":
        return "Space";
      default:
        return raw.toUpperCase();
    }
  });
}

/** One bound-command family in the dialog. */
export interface ShortcutGroup {
  section: string;
  commands: PidashCommand[];
}

/**
 * Groups bound commands under family headings, dropping rows whose title
 * misses the filter (SHELL-094). Input order is kept, so callers pass the
 * palette-ordered snapshot and families render in palette order.
 */
export function groupBoundCommands(commands: PidashCommand[], query: string): ShortcutGroup[] {
  const needle = query.trim().toLowerCase();
  const groups: ShortcutGroup[] = [];
  for (const command of commands) {
    if (needle && !command.title.toLowerCase().includes(needle)) continue;
    const section = command.section ?? "General";
    const last = groups[groups.length - 1];
    if (last && last.section === section) {
      last.commands.push(command);
    } else {
      groups.push({ section, commands: [command] });
    }
  }
  return groups;
}

export function ShortcutsDialog(): React.ReactElement {
  const open = useShortcutsDialog((state) => state.open);
  const closeShortcuts = useShortcutsDialog((state) => state.closeShortcuts);
  const [filter, setFilter] = React.useState("");
  React.useEffect(() => {
    if (!open) setFilter("");
  }, [open]);
  // Fresh snapshot per opening: registrations change as screens mount.
  const groups = React.useMemo(() => (open ? groupBoundCommands(listBoundCommands(), filter) : []), [open, filter]);
  const mac = isMacPlatform();

  return (
    <Dialog title="Keyboard shortcuts" open={open} onOpenChange={(next) => (next ? undefined : closeShortcuts())}>
      <Input
        label="Filter shortcuts"
        placeholder="Search for shortcuts"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
        autoFocus
      />
      {groups.length === 0 ? (
        <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">No matching shortcuts.</p>
      ) : (
        <div className="mt-(--space-4) max-h-80 overflow-auto">
          {groups.map((group) => (
            <div key={group.section}>
              <p className="text-caption px-(--space-2) pt-(--space-3) pb-(--space-1) text-(--text-muted)">
                {group.section}
              </p>
              <ul>
                {group.commands.map((command) => (
                  <li
                    key={command.id}
                    className="text-body flex w-full items-center justify-between gap-(--space-2) px-(--space-2) py-(--space-2) text-left text-(--text)"
                  >
                    <span className="min-w-0 flex-1 truncate">{command.title}</span>
                    <span className="flex shrink-0 items-center gap-(--space-2)">
                      {command.sequence ? <SequenceBadge sequence={command.sequence} /> : null}
                      {command.shortcut ? <Kbd keys={formatChordKeys(command.shortcut, mac)} /> : null}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </Dialog>
  );
}
