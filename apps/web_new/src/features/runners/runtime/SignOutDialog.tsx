// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop sign-out confirmation (RUN-047). Signing out removes
// credentials and, by default, nothing else: an account's chats with the
// built-in agent stay on this computer, hidden until that account signs
// in again. The switch below is the one place a user can ask for them to
// be deleted instead, so it starts off every time the dialog opens. The
// supervisor mounts this on desktop only; with no dialog mounted,
// confirmation resolves to the keep-history default.

import { Button, Dialog, Switch } from "@pidash/kit";
import * as React from "react";

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";

type SignOutPrompt = (resolve: (choice: RuntimeSignOutChoice | null) => void) => void;

let signOutPrompt: SignOutPrompt | null = null;

/**
 * Mount (or, with null, unmount) the dialog confirmRuntimeSignOut opens
 * through. Owned by RuntimeSignOutDialog.
 */
export function registerSignOutPrompt(prompt: SignOutPrompt | null): void {
  signOutPrompt = prompt;
}

/**
 * Ask the user to confirm a sign-out they started, and whether to delete
 * this account's chat history with it. Resolves null when they cancel.
 * Deletion is opt-in: with no dialog to ask through, sign-out proceeds
 * and keeps the history.
 */
export function confirmRuntimeSignOut(): Promise<RuntimeSignOutChoice | null> {
  const prompt = signOutPrompt;
  if (!prompt) return Promise.resolve({ deleteChatHistory: false });
  return new Promise((resolve) => prompt(resolve));
}

export function RuntimeSignOutDialog(): React.ReactElement {
  const [isOpen, setIsOpen] = React.useState(false);
  const [deleteChatHistory, setDeleteChatHistory] = React.useState(false);
  const pending = React.useRef<((choice: RuntimeSignOutChoice | null) => void) | null>(null);

  const settle = React.useCallback((choice: RuntimeSignOutChoice | null): void => {
    const resolve = pending.current;
    pending.current = null;
    setIsOpen(false);
    if (resolve) resolve(choice);
  }, []);

  React.useEffect(() => {
    registerSignOutPrompt((resolve) => {
      // A second request while one is open replaces it; the first is
      // cancelled rather than left waiting forever.
      pending.current?.(null);
      pending.current = resolve;
      setDeleteChatHistory(false);
      setIsOpen(true);
    });
    return () => {
      registerSignOutPrompt(null);
      settle(null);
    };
  }, [settle]);

  return (
    <Dialog
      title="Sign out?"
      description="Signing out removes this account's credentials from this computer. Its chats with the built-in agent stay on this computer, hidden until the account signs in again."
      open={isOpen}
      onOpenChange={() => settle(null)}
      actions={
        <>
          <Button variant="ghost" onClick={() => settle(null)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => settle({ deleteChatHistory })}>
            Sign out
          </Button>
        </>
      }
    >
      <div className="flex items-center gap-(--space-3)">
        <Switch
          checked={deleteChatHistory}
          onCheckedChange={setDeleteChatHistory}
          label="Also delete chat history"
          data-testid="runtime-sign-out-delete-history"
        />
        <div className="flex flex-col">
          <span className="text-body text-(--text)">Also delete chat history</span>
          <span className="text-caption text-(--text-muted)">Task folders are kept.</span>
        </div>
      </div>
    </Dialog>
  );
}
