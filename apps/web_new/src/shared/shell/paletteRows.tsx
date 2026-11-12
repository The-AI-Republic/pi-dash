// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared palette row rendering (shell spec §8). The modal palette and the
// top-bar search box show the same rows — grouped commands, grouped server
// hits, the no-hits clear row — so the row shape, the command grouping and
// the option button live here, used by both surfaces.

import { Kbd } from "@pidash/kit";
import * as React from "react";

import { formatSequenceKeys, type PidashCommand, type SearchResultHit } from "../commands/index.js";

/** One keyboard-navigable palette row. */
export type PaletteRow =
  | { kind: "command"; command: PidashCommand }
  | { kind: "hit"; hit: SearchResultHit }
  | { kind: "clear-search" };

export interface CommandGroup {
  section: string;
  entries: Array<{ command: PidashCommand; index: number }>;
}

/** Groups commands under section headings while keeping flat row indices. */
export function groupCommands(commands: PidashCommand[]): CommandGroup[] {
  const sections = new Map<string, CommandGroup["entries"]>();
  for (const [index, command] of commands.entries()) {
    const section = command.section ?? "General";
    const entries = sections.get(section) ?? [];
    entries.push({ command, index });
    sections.set(section, entries);
  }
  return [...sections.entries()].map(([section, entries]) => ({ section, entries }));
}

/** Two-key sequence chips ("op" renders O then P; shell spec, SHELL-086/087). */
export function SequenceBadge({ sequence }: { sequence: string }): React.ReactElement {
  const keys = formatSequenceKeys(sequence);
  return (
    <span aria-label={keys.join(" then ")} className="inline-flex shrink-0 items-center gap-(--space-1)">
      {keys.map((key, index) => (
        <React.Fragment key={`${index}-${key}`}>
          {index > 0 ? (
            <span aria-hidden="true" className="text-caption text-(--text-muted)">
              then
            </span>
          ) : null}
          <Kbd keys={[key]} />
        </React.Fragment>
      ))}
    </span>
  );
}

export function PaletteOption({
  active,
  title,
  hint,
  shortcut,
  sequence,
  selected,
  icon,
  onHover,
  onSelect,
}: {
  active: boolean;
  title: string;
  hint?: string | undefined;
  shortcut?: string | undefined;
  sequence?: string | undefined;
  selected?: boolean | undefined;
  icon?: React.ReactNode | undefined;
  onHover: () => void;
  onSelect: () => void;
}): React.ReactElement {
  return (
    <li>
      <button
        type="button"
        role="option"
        aria-selected={active}
        onClick={onSelect}
        onMouseEnter={onHover}
        className={
          active
            ? "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-2) text-left text-(--text)"
            : "text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text)"
        }
      >
        {icon === undefined ? null : (
          <span aria-hidden="true" data-testid="palette-option-icon" className="flex shrink-0 items-center">
            {icon}
          </span>
        )}
        <span data-testid="palette-option-title" className="min-w-0 flex-1 truncate">
          {title}
        </span>
        {hint ? <span className="text-caption max-w-40 truncate text-(--text-muted)">{hint}</span> : null}
        {sequence ? <SequenceBadge sequence={sequence} /> : null}
        {shortcut ? <Kbd keys={shortcut.split("+")} /> : null}
        {selected === true ? (
          <span aria-hidden="true" className="shrink-0 text-(--accent)">
            ✓
          </span>
        ) : null}
      </button>
    </li>
  );
}
