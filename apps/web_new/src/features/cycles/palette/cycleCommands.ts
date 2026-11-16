// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles palette command (parity/specs/cycles.md §6, CYC-014). Pure
// builder: the entries component registers exactly what this returns, so
// an entry the viewer may not use is absent rather than disabled
// (SHELL-092). Creation needs the project context, the cycles switch on,
// and project member-or-better standing. The shell owns the key, the
// palette frame and the search box; nothing here listens for a keypress.

import type { PidashCommand } from "../../../shared/commands/index.js";

/** Palette command that opens the cycles area's create dialog. */
export const CYCLE_CREATE_COMMAND_ID = "cycles.create-cycle";

/** Creation title the oracle matches (`nc` sequence beside it). */
export const CYCLE_CREATE_TITLE = "New cycle";

/** Two-key sequence opening the create dialog (CYC-014). */
export const CYCLE_CREATE_SEQUENCE = "nc";

export interface CycleCreateCommandContext {
  /** Project name shown as the entry's hint. */
  projectName: string;
  /** The project's cycles switch as the loaded project row reports it. */
  cyclesEnabled: boolean;
  /** Project admin or member standing; a guest reads false. */
  canCreate: boolean;
  /** Opens the create dialog for the project. */
  onCreate: () => void;
}

/**
 * The creation entry, or null when the switch is off or the viewer is a
 * guest. There is no project-less variant: the command only exists on a
 * project page, so leaving it unregistered is what hides it elsewhere.
 */
export function buildCycleCreateCommand(context: CycleCreateCommandContext): PidashCommand | null {
  if (!context.cyclesEnabled || !context.canCreate) return null;
  return {
    id: CYCLE_CREATE_COMMAND_ID,
    title: CYCLE_CREATE_TITLE,
    hint: context.projectName,
    section: "Create",
    sequence: CYCLE_CREATE_SEQUENCE,
    keywords: ["create", "add", "new", "cycle"],
    run: context.onCreate,
  };
}
