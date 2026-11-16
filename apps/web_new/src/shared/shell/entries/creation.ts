// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette creation entries (shell spec, SHELL-086). The shell owns the
// entry points it can prove: project creation opens the shell-owned
// create dialog, workspace creation routes to its dedicated page. Other
// areas register their own creation entries through the command registry
// from their epics (shell spec §10); nothing here stands in for them.

import type { PidashCommand } from "../../commands/index.js";

export interface CreationEntryContext {
  /** Workspace admin/member: may create projects (SHELL-086/092). */
  canCreateProject: boolean;
  /** False when the instance disables workspace creation (SHELL-092). */
  canCreateWorkspace: boolean;
  /** Opens the shell-owned project creation dialog. */
  onOpenCreateProject: () => void;
  /** Routes to the workspace creation page. */
  onCreateWorkspace: () => void;
}

/**
 * The shell-owned creation entries. Unauthorized entries are absent,
 * never disabled (SHELL-092): callers register exactly what this
 * returns.
 */
export function buildCreationCommands(context: CreationEntryContext): PidashCommand[] {
  const commands: PidashCommand[] = [];
  if (context.canCreateProject) {
    commands.push({
      id: "shell.create-project",
      title: "New project",
      section: "Create",
      sequence: "np",
      run: context.onOpenCreateProject,
    });
  }
  if (context.canCreateWorkspace) {
    commands.push({
      id: "shell.create-workspace",
      title: "New workspace",
      section: "Create",
      run: context.onCreateWorkspace,
    });
  }
  return commands;
}
