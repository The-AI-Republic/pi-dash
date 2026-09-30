// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Command palette (Architecture > shared/shell). Reads the shared command
// registry, filters by title, and runs the chosen command. Global shortcut
// handling (mod+k) lives in AppShell; this dialog only owns its own list
// keys while open.

import { Dialog, Input, Kbd } from "@pidash/kit";
import * as React from "react";

import { listCommands, useCommandPalette, type PidashCommand } from "../commands/index.js";

function filterCommands(commands: PidashCommand[], query: string): PidashCommand[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return commands;
  return commands.filter(
    (command) =>
      command.title.toLowerCase().includes(needle) ||
      (command.hint ?? "").toLowerCase().includes(needle) ||
      (command.section ?? "").toLowerCase().includes(needle)
  );
}

export function CommandPalette(): React.ReactElement {
  const open = useCommandPalette((state) => state.open);
  const query = useCommandPalette((state) => state.query);
  const closePalette = useCommandPalette((state) => state.closePalette);
  const setQuery = useCommandPalette((state) => state.setQuery);
  const [activeIndex, setActiveIndex] = React.useState(0);

  const matches = React.useMemo(() => filterCommands(listCommands(), query), [query, open]);
  const flat = matches;

  React.useEffect(() => {
    if (open) setActiveIndex(0);
  }, [open, query]);

  const runCommand = React.useCallback(
    (command: PidashCommand) => {
      closePalette();
      void command.run();
    },
    [closePalette]
  );

  const onListKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((index) => Math.min(index + 1, Math.max(flat.length - 1, 0)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      const command = flat[activeIndex];
      if (command) runCommand(command);
    }
  };

  const indexed = React.useMemo(() => matches.map((command, index) => ({ command, index })), [matches]);
  const grouped = React.useMemo(() => {
    const groups = new Map<string, Array<{ command: PidashCommand; index: number }>>();
    for (const entry of indexed) {
      const section = entry.command.section ?? "General";
      const entries = groups.get(section) ?? [];
      entries.push(entry);
      groups.set(section, entries);
    }
    return [...groups.entries()].map(([section, entries]) => ({ section, entries }));
  }, [indexed]);

  return (
    <Dialog title="Commands" open={open} onOpenChange={(next) => (next ? undefined : closePalette())}>
      <div onKeyDown={onListKeyDown}>
        <Input
          label="Filter commands"
          placeholder="Type a command…"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          autoFocus
        />
        <div role="listbox" aria-label="Commands" className="mt-(--space-4) max-h-80 overflow-auto">
          {matches.length === 0 ? (
            <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">No matching commands.</p>
          ) : (
            grouped.map((group) => (
              <div key={group.section}>
                <p className="text-caption px-(--space-2) pt-(--space-3) pb-(--space-1) text-(--text-muted)">
                  {group.section}
                </p>
                <ul>
                  {group.entries.map(({ command, index }) => {
                    const active = index === activeIndex;
                    return (
                      <li key={command.id}>
                        <button
                          type="button"
                          role="option"
                          aria-selected={active}
                          onClick={() => runCommand(command)}
                          onMouseEnter={() => setActiveIndex(index)}
                          className={
                            active
                              ? "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
                              : "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text)"
                          }
                        >
                          <span className="min-w-0 flex-1 truncate">{command.title}</span>
                          {command.shortcut ? <Kbd keys={command.shortcut.split("+")} /> : null}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ))
          )}
        </div>
      </div>
    </Dialog>
  );
}
