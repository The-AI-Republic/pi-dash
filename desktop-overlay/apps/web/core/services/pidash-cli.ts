/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Setting up Pi Dash on this computer from the app
 * (`.ai_design/built_engine_as_runner/design.md` §4.7; PIDESKAPP-114).
 *
 * When the app finds no daemon it offers "Set up Pi Dash on this computer" the
 * first time the user turns the engine on for a project, adds a runner or
 * opens local chat ({@link requestDaemonSetup}). The service is required
 * (PO-12): the consent screen says it runs in the background, and declining
 * sets nothing up — the app stays a cloud client and offers set-up again from
 * the same places.
 *
 * Every step is the CLI's own, so the result is a terminal install:
 *
 * 1. install the CLI the app carries into the official location (the network
 *    installer on Windows);
 * 2. sign it in with a device grant the app approves with its session, scoped
 *    to the workspace — one browser sign-in in total, the app's;
 * 3. `pidash install` with the login-shell `PATH`, then `pidash start`;
 * 4. `pidash engine install` when the engine is not there.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import {
  approveDeviceGrant,
  compatibleHello,
  currentDiscovery,
  discoverDaemon,
  invoke,
  isDesktopHost,
} from "@/services/desktop-daemon";

export interface CliStatus {
  installed: boolean;
  path: string | null;
  version: string | null;
}

/** Is the CLI on this machine, and which version? */
export async function detectCli(): Promise<CliStatus> {
  if (!isDesktopHost()) return { installed: false, path: null, version: null };
  return invoke<CliStatus>("detect_pidash_cli");
}

/** The step the set-up is on, for its progress screen. */
export type SetUpStep = "install_cli" | "sign_in" | "service" | "engine" | "done";

/**
 * Set Pi Dash up on this computer. Run only after the user accepted the
 * consent screen. Progress lines arrive on the `pidash-install-log` Tauri
 * event, rendered as plain text by the caller.
 */
export async function setUpCliForThisMachine(
  workspaceSlug: string,
  onStep: (step: SetUpStep) => void = () => {}
): Promise<void> {
  if (!isDesktopHost()) return;
  onStep("install_cli");
  const status = await detectCli();
  if (!status.installed) await invoke<void>("install_pidash_cli");

  // Already signed in as this account (a CLI-first install whose service was
  // stopped, say): nothing to sign in.
  if (!compatibleHello(await discoverDaemon().catch(() => currentDiscovery()))) {
    onStep("sign_in");
    const deviceCode = await approveDeviceGrant({ workspaceSlug, interactive: false });
    // The CLI finishes the exchange and owns everything it writes.
    await invoke<void>("pidash_cli_login", {
      deviceCode,
      cloudUrl: API_BASE_URL,
      workspace: workspaceSlug,
    });
  }

  onStep("service");
  await invoke<void>("daemon_install_service");
  onStep("engine");
  await invoke<void>("daemon_ensure_engine");
  await discoverDaemon();
  onStep("done");
}

/** Why set-up was asked for, shown on the consent screen. */
export type SetUpReason = "engine" | "runner" | "chat";

type SetUpPrompt = (request: { reason: SetUpReason; workspaceSlug: string }, done: (ok: boolean) => void) => void;
let setUpPrompt: SetUpPrompt | null = null;

/** Mounted by the consent dialog; `null` unmounts it. */
export function registerSetUpPrompt(prompt: SetUpPrompt | null) {
  setUpPrompt = prompt;
}

/**
 * Make sure this computer has a daemon the app can use, asking the user to set
 * one up when there is none. Resolves `true` once it is there, `false` when
 * the user declined (nothing was set up) or the daemon needs something else
 * first (another account, an update — the caller shows that).
 */
export async function requestDaemonSetup(reason: SetUpReason, workspaceSlug: string): Promise<boolean> {
  if (!isDesktopHost()) return false;
  const found = await discoverDaemon().catch(() => currentDiscovery());
  if (compatibleHello(found)) return true;
  if (found && found.kind !== "no_daemon" && found.kind !== "signed_out") return false;
  const prompt = setUpPrompt;
  if (!prompt) return false;
  return new Promise((resolve) => prompt({ reason, workspaceSlug }, resolve));
}
