// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The request bridge between the auth sign-out sequence and the desktop
// confirmation dialog (RUN-047). It holds no UI and no kit imports so the
// root lifecycle can register it eagerly and keep the dialog in its own
// lazy chunk.

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";

type SignOutPrompt = (resolve: (choice: RuntimeSignOutChoice | null) => void) => void;

let activePrompt: SignOutPrompt | null = null;

/** Mount (or, with null, unmount) the prompt confirmRuntimeSignOut opens. */
export function registerSignOutPrompt(prompt: SignOutPrompt | null): void {
  activePrompt = prompt;
}

/**
 * Ask the user to confirm a sign-out they started, and whether to delete
 * this account's chat history with it. Resolves null when they cancel.
 * Deletion is opt-in: with no dialog mounted yet, sign-out proceeds and
 * keeps the history.
 */
export function confirmRuntimeSignOut(): Promise<RuntimeSignOutChoice | null> {
  const prompt = activePrompt;
  if (!prompt) return Promise.resolve({ deleteChatHistory: false });
  return new Promise((resolve) => prompt(resolve));
}
