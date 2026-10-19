// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Turning this machine into a runner host from inside the app (DESK-021,
// from parity/specs/desktop.md §5). The bundle ships the command-line tool;
// this flow makes sure it is installed, then signs it in with a device-code
// grant the signed-in app starts and approves itself. The tool exchanges the
// approved device code and owns the credential, workspace binding and file
// format — nothing here reads or stores a token. Registering a runner host
// proper (project, working directory, agent) is a separate flow.

import { approveDeviceCode, startDeviceGrant, type ApiClient } from "@pidash/api-client";

import type { CliInstallProgress, CliStatus } from "../../core/platform/index.js";

/** The part of the native tool capability this flow drives. */
export interface CliSetupApi {
  detect(): Promise<CliStatus>;
  install(): Promise<CliStatus>;
  onInstallLog(callback: (progress: CliInstallProgress) => void): () => void;
  login(input: { deviceCode: string; cloudUrl: string; workspace: string }): Promise<void>;
}

export interface CliSetupInput {
  /** API client of the signed-in session that approves the grant. */
  client: ApiClient;
  /** Address the tool should talk to; the app's own API origin. */
  cloudUrl: string;
  workspace: string;
  /** Absent on builds without the native tool capability. */
  cli: CliSetupApi | undefined;
}

export interface CliSetupDeps {
  /** False when a second setup is already running. */
  acquire(): boolean;
  release(): void;
}

/** One setup at a time per machine: the native installer cannot be raced. */
export function createSetupLock(): CliSetupDeps {
  let held = false;
  return {
    acquire: () => {
      if (held) return false;
      held = true;
      return true;
    },
    release: () => {
      held = false;
    },
  };
}

export const CLI_SETUP_BUSY = "The command-line tool is already being set up.";
export const CLI_SETUP_UNAVAILABLE = "This build cannot set up the command-line tool.";

/** Which part of the flow is running (for the panel's status line). */
export type CliSetupStage = "detecting" | "installing" | "signing-in";

export interface CliSetupResult {
  /** Tool state after the flow (installed, with its version). */
  status: CliStatus;
  /** Start state: whether the install step ran. */
  installedNow: boolean;
}

/**
 * Detect, install when missing, then sign the tool in. The install's output
 * lines reach `onProgress` verbatim and in order; the grant is always
 * approved by the caller's own session, never by a typed code.
 */
export async function setUpCli(
  input: CliSetupInput,
  deps: CliSetupDeps = createSetupLock(),
  onProgress: ((progress: CliInstallProgress) => void) | undefined = undefined,
  onStage: ((stage: CliSetupStage) => void) | undefined = undefined
): Promise<CliSetupResult> {
  const { cli } = input;
  if (cli === undefined) throw new Error(CLI_SETUP_UNAVAILABLE);
  if (!deps.acquire()) throw new Error(CLI_SETUP_BUSY);
  try {
    onStage?.("detecting");
    let status = await cli.detect();
    const installedNow = !status.installed;
    if (installedNow) {
      onStage?.("installing");
      const unsubscribe = onProgress === undefined ? () => undefined : cli.onInstallLog(onProgress);
      try {
        status = await cli.install();
      } finally {
        unsubscribe();
      }
    }
    onStage?.("signing-in");
    // Both halves of the browser device page, without the page: start the
    // grant, approve it with the signed-in session, hand the device code to
    // the tool.
    const grant = await startDeviceGrant(input.client);
    await approveDeviceCode(input.client, grant.user_code);
    await cli.login({ deviceCode: grant.device_code, cloudUrl: input.cloudUrl, workspace: input.workspace });
    return { status, installedNow };
  } finally {
    deps.release();
  }
}
