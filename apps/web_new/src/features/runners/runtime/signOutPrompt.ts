// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The request bridge between the auth sign-out sequence and the desktop
// confirmation dialog (RUN-047). It holds no UI and no kit imports so the
// root lifecycle can register it eagerly and keep the dialog in its own
// lazy chunk. A sign-out that starts while that chunk is still loading
// waits for the dialog to register instead of skipping past it.

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";

type SignOutPrompt = (resolve: (choice: RuntimeSignOutChoice | null) => void) => void;
type LoadState = "idle" | "loading" | "ready" | "unavailable";

let activePrompt: SignOutPrompt | null = null;
let loadState: LoadState = "idle";
let waiting: Array<(choice: RuntimeSignOutChoice | null) => void> = [];

function keepHistory(): RuntimeSignOutChoice {
  return { deleteChatHistory: false };
}

/**
 * Mark the confirmation dialog as being fetched. Called once by the
 * desktop lifecycle before the chunk loads, so sign-outs that arrive
 * during the fetch queue up rather than taking the no-dialog default.
 */
export function beginSignOutPromptLoad(): void {
  if (loadState === "idle") loadState = "loading";
}

/**
 * The dialog chunk could not be fetched. Nothing can ask the user, so
 * pending and later confirmations keep the chat history and let sign-out
 * proceed instead of trapping the user.
 */
export function failSignOutPromptLoad(): void {
  loadState = "unavailable";
  const queued = waiting;
  waiting = [];
  for (const resolve of queued) resolve(keepHistory());
}

/** Mount (or, with null, unmount) the prompt confirmRuntimeSignOut opens. */
export function registerSignOutPrompt(prompt: SignOutPrompt | null): void {
  activePrompt = prompt;
  if (!prompt) {
    // The confirmation UI went away with the app shell; anyone who was
    // still waiting signs out keeping their history rather than hanging.
    loadState = "idle";
    const queued = waiting;
    waiting = [];
    for (const resolve of queued) resolve(keepHistory());
    return;
  }
  loadState = "ready";
  const queued = waiting;
  waiting = [];
  for (const resolve of queued) prompt(resolve);
}

/**
 * Ask the user to confirm a sign-out they started, and whether to delete
 * this account's chat history with it. Resolves null when they cancel.
 * While the dialog chunk is loading the request waits for the dialog;
 * only when no dialog is expected (web, unmounted, or a failed fetch)
 * does sign-out proceed and keep the history.
 */
export function confirmRuntimeSignOut(): Promise<RuntimeSignOutChoice | null> {
  const prompt = activePrompt;
  if (prompt) {
    return new Promise((resolve) => prompt(resolve));
  }
  if (loadState === "loading") {
    return new Promise((resolve) => waiting.push(resolve));
  }
  return Promise.resolve(keepHistory());
}
