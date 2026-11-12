/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * "This computer" for the runner UI (Add runner, the AI Agents nav).
 *
 * This is a seam. In the web app there is no "this computer": the hook returns
 * nothing and the UI behaves as before. The Tauri desktop build overlays this
 * file (`desktop-overlay/`) with the computer's own Pi Dash daemon: its dev
 * machine (preselected in Add runner), a folder picker for the working
 * directory, turning the built-in engine on for a project, and the engine
 * runners the "This computer" contact already represents in the nav.
 */

export interface ThisComputer {
  /** This computer's dev machine id, when the app is a client of its daemon. */
  machineId?: string;
  /**
   * Runner ids served by this computer's built-in engine. The nav shows them
   * as the local contact, not again under "Runners" (design §12.1).
   */
  localEngineRunnerIds: ReadonlySet<string>;
  /** Pick a folder on this computer; resolves `null` when cancelled. */
  pickFolder?: (defaultPath?: string) => Promise<string | null>;
  /** Turn the built-in engine on for a project on this computer. */
  addEngineRunner?: (input: { workspaceSlug: string; project: string; workingDir?: string }) => Promise<string>;
  /**
   * Set Pi Dash up on this computer (asks the user first). Resolves whether
   * this computer can now host runners.
   */
  setUp?: (workspaceSlug: string) => Promise<boolean>;
}

const NONE: ThisComputer = { localEngineRunnerIds: new Set() };

/** No "this computer" in the web app. Overridden in the desktop build. */
export function useThisComputer(): ThisComputer {
  return NONE;
}
