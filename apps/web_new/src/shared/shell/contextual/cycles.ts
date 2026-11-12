// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle contextual palette entries (shell spec, SHELL-091). Pure
// selector: the favorite toggle plus copy-URL, gated on project
// standing (admin or member, not archived). Archived cycles expose
// neither entry (parity/specs/cycles.md §11, CYC-030). The toggle
// persists through user-favorites so the gate proves it against the
// server; full cycle ownership stays in the cycles area.

import type { PidashCommand } from "../../commands/registry.js";
import { canEditInProject, CONTEXT_INDICATORS, CONTEXT_SECTIONS, type ProjectStanding } from "./types.js";

export interface CycleContextInput {
  cycleId: string;
  /** Cycle name shown in the chip. */
  cycleName: string;
  archived: boolean;
  favorite: boolean;
  standing: ProjectStanding;
  toggleFavorite: () => void;
  copyUrl: () => void;
}

export function selectCycleContext(input: CycleContextInput): PidashCommand[] {
  if (input.archived) return [];
  const section = CONTEXT_SECTIONS.cycle;
  const context = { kind: "cycle" as const, indicator: CONTEXT_INDICATORS.cycle, subject: input.cycleName };
  const prefix = `context.cycle.${input.cycleId}`;
  const commands: PidashCommand[] = [];
  if (canEditInProject(input.standing)) {
    commands.push({
      id: `${prefix}.favorite`,
      title: input.favorite ? "Remove from favorites" : "Add to favorites",
      section,
      shortcut: "shift+f",
      context,
      run: () => input.toggleFavorite(),
    });
  }
  commands.push({
    id: `${prefix}.copy-url`,
    title: "Copy URL",
    section,
    shortcut: "mod+shift+,",
    context,
    run: () => input.copyUrl(),
  });
  return commands;
}
