/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Turning this computer into a Pi Dash runner host, from inside the app.
 *
 * Installing the desktop app should be enough to host a runner — including for
 * a user who brings their own agent (Claude Code, Codex, Cursor) rather than
 * the built-in one. That path runs through the `pidash` CLI, which the app
 * already ships and can install from its own bundle, and which then needs a
 * credential.
 *
 * Signing the CLI in is a device-code grant, and the app is already signed in,
 * so it can play both halves of the web `/auth/device/` page: start the grant,
 * then approve it with its own session. The approved code goes to the CLI,
 * which exchanges it and writes its own config — the machine token, the
 * workspace binding and the file format stay in the runner where they belong.
 * It is the same CLI, at the same default config, that the app's shared daemon
 * runs from (`desktop-account-sync.ts`).
 *
 * What is deliberately *not* here: `pidash runner add`. Registering a runner
 * picks a project, a working directory and an agent, which is a UI flow of its
 * own rather than a credential step.
 */

import { isDesktop } from "@/services/agent-runtime";
import { signDaemonIn } from "@/services/desktop-account-sync";

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

function invoke<T>(command: string, args?: Record<string, unknown>) {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

export interface CliStatus {
  installed: boolean;
  path: string | null;
  version: string | null;
}

/** Is the CLI on this machine, and which version? */
export async function detectCli(): Promise<CliStatus> {
  if (!isDesktop()) return { installed: false, path: null, version: null };
  return invoke<CliStatus>("detect_pidash_cli");
}

/**
 * Install the CLI from the app's own bundle (no download), then sign it in
 * with a grant this app approves (see `signDaemonIn`). Progress of the install
 * arrives on the `pidash-install-log` Tauri event, which the caller can
 * subscribe to.
 */
export async function setUpCliForThisMachine(workspaceSlug: string): Promise<void> {
  if (!isDesktop()) return;
  const status = await detectCli();
  if (!status.installed) await invoke<void>("install_pidash_cli");
  await signDaemonIn(workspaceSlug);
}
