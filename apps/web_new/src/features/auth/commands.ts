// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth-owned command descriptors (AUTH-017 seam). The sign-out call path
// lives in api.ts; this is the registration seam shell surfaces invoke:
// the F-06 shell already exposes the sidebar and palette entries through
// it, and NEWFRONT-70 registers the same descriptor into the full menus.

import type { PidashCommand } from "../../shared/commands/index.js";

/** Stable palette id for sign-out, owned by the auth area. */
export const SIGN_OUT_COMMAND_ID = "shell.sign-out";

/**
 * The sign-out palette command. `onSignOut` runs the useSignOut mutation
 * and navigates to its landing target; the descriptor only carries the
 * stable id, title, and section every surface shares.
 */
export function signOutCommand(options: { userLabel: string; onSignOut: () => void }): PidashCommand {
  return {
    id: SIGN_OUT_COMMAND_ID,
    title: "Sign out",
    hint: options.userLabel,
    section: "Account",
    run: options.onSignOut,
  };
}

/**
 * The sign-out action every surface invokes: resolve the useSignOut
 * mutation, then hand its landing target to the caller for navigation. A
 * rejection means the hook already toasted the failure, and a null
 * landing means the user cancelled the desktop confirmation, so the
 * action just returns and the user stays where they are.
 */
export async function runSignOutAction(
  mutateAsync: () => Promise<string | null>,
  onLanded: (landing: string) => void
): Promise<void> {
  let landing: string | null;
  try {
    landing = await mutateAsync();
  } catch {
    return;
  }
  if (landing === null) return;
  onLanded(landing);
}
