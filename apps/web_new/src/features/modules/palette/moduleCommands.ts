// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules palette commands (parity/specs/modules.md §10.2, MOD-037). Pure
// builders: the entries component registers exactly what these return, so
// an entry the viewer may not use is absent rather than disabled
// (SHELL-092). Creation needs a project, the project's modules switch and
// member-or-better standing; the jump entry only navigates, so every role
// that can read the project gets it. The shell owns the keys, the palette
// frame and the search box; nothing here listens for a keypress.

import type { PidashCommand } from "../../../shared/commands/index.js";

/** Palette command that opens the modules area's create dialog. */
export const MODULE_CREATE_COMMAND_ID = "modules.create-module";
/** Palette command that opens the project's module jump sub-page. */
export const MODULE_JUMP_COMMAND_ID = "modules.open-module";
/** Sub-page id behind the jump command. */
export const MODULE_JUMP_PAGE_ID = "modules.module-picker";

/** Creation title the oracle matches (`nm` sequence beside it). */
export const MODULE_CREATE_TITLE = "New module";
/** Jump title the oracle matches. */
export const MODULE_JUMP_TITLE = "Open a module";

/** Two-key sequence opening the create dialog (MOD-037/040). */
export const MODULE_CREATE_SEQUENCE = "nm";

export interface ModuleCreateCommandContext {
  /** Project name shown as the entry's hint. */
  projectName: string;
  /** The project's modules switch as the loaded project row reports it. */
  modulesEnabled: boolean;
  /** Project admin or member standing; a guest reads false. */
  canCreate: boolean;
  /** Opens the create dialog for the project. */
  onCreate: () => void;
}

export interface ModuleJumpCommandContext {
  /** Project name shown as the entry's hint. */
  projectName: string;
  /** The project's modules switch; a gated project has nothing to open. */
  modulesEnabled: boolean;
}

/**
 * The creation entry, or null when the switch is off or the viewer is a
 * guest. There is no project-less variant: the command only exists on a
 * project page, so leaving it unregistered is what hides it elsewhere.
 */
export function buildModuleCreateCommand(context: ModuleCreateCommandContext): PidashCommand | null {
  if (!context.modulesEnabled || !context.canCreate) return null;
  return {
    id: MODULE_CREATE_COMMAND_ID,
    title: MODULE_CREATE_TITLE,
    hint: context.projectName,
    section: "Create",
    sequence: MODULE_CREATE_SEQUENCE,
    keywords: ["create", "add", "new", "module"],
    run: context.onCreate,
  };
}

/** The jump entry, or null while the project's modules switch is off. */
export function buildModuleJumpCommand(context: ModuleJumpCommandContext): PidashCommand | null {
  if (!context.modulesEnabled) return null;
  return {
    id: MODULE_JUMP_COMMAND_ID,
    title: MODULE_JUMP_TITLE,
    hint: context.projectName,
    section: "Navigate",
    page: MODULE_JUMP_PAGE_ID,
    run: () => undefined,
  };
}
