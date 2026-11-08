// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The request bridge between the runtime service and the "Set up Pi Dash
// on this computer" consent dialog (PIDESKAPP-222; design §4.7). When the
// app finds no daemon it asks the first time the user turns the engine on
// for a project or opens local chat. Declining sets nothing up: the app
// stays a cloud client and asks again from the same places. No UI and no
// kit imports here, so the service can depend on it without the dialog.

/** Why set-up was asked for, shown on the consent screen. */
export type SetUpReason = "engine" | "chat";

export interface SetUpRequest {
  reason: SetUpReason;
  workspaceSlug: string;
}

/** Opens the consent dialog; `done(true)` once set-up finished, `done(false)` when declined. */
export type SetUpPrompt = (request: SetUpRequest, done: (ok: boolean) => void) => void;

let activePrompt: SetUpPrompt | null = null;

/** Mounted by the consent dialog; `null` unmounts it. */
export function registerSetUpPrompt(prompt: SetUpPrompt | null): void {
  activePrompt = prompt;
}

/**
 * Ask the user to set Pi Dash up on this computer. Resolves `true` once
 * set-up finished, `false` when they declined or no dialog is mounted
 * (nothing is ever set up without the consent screen).
 */
export function requestDaemonSetup(request: SetUpRequest): Promise<boolean> {
  const prompt = activePrompt;
  if (!prompt) return Promise.resolve(false);
  return new Promise((resolve) => prompt(request, resolve));
}
