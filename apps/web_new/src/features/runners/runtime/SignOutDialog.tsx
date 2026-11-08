// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The desktop's sign-out confirmation.
//
// Signing out of the app signs out only the app (design §4.6): Pi Dash on
// this computer — the daemon, the built-in engine and every runner — stays
// signed in, because it does not depend on the app. Two opt-ins, both
// unticked every time the dialog opens:
//
// - "Delete chat history" removes this account's local chats;
// - "Also sign out Pi Dash on this computer" runs `pidash auth logout`. Shown
//   only when the computer's daemon is signed in as this account.

import { Button, Dialog, Switch } from "@pidash/kit";
import * as React from "react";

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import { canSignOutComputer } from "./service.js";
import { registerSignOutPrompt } from "./signOutPrompt.js";

export function RuntimeSignOutDialog(): React.ReactElement {
  const [isOpen, setIsOpen] = React.useState(false);
  const [deleteChatHistory, setDeleteChatHistory] = React.useState(false);
  const [signOutComputer, setSignOutComputer] = React.useState(false);
  const [offerComputer, setOfferComputer] = React.useState(false);
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
      setSignOutComputer(false);
      setOfferComputer(canSignOutComputer());
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
          <Button
            variant="primary"
            onClick={() => settle({ deleteChatHistory, signOutComputer: offerComputer && signOutComputer })}
          >
            Sign out
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
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
        {offerComputer ? (
          <div className="flex items-center gap-(--space-3)">
            <Switch
              checked={signOutComputer}
              onCheckedChange={setSignOutComputer}
              label="Also sign out Pi Dash on this computer"
              data-testid="runtime-sign-out-sign-out-computer"
            />
            <div className="flex flex-col">
              <span className="text-body text-(--text)">Also sign out Pi Dash on this computer</span>
              <span className="text-caption text-(--text-muted)">
                The runners on this computer stop taking work until you sign in again.
              </span>
            </div>
          </div>
        ) : null}
      </div>
    </Dialog>
  );
}
