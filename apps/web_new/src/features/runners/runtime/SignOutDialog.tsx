// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The desktop's sign-out confirmation.
//
// Signing out of the app signs out Pi Dash on this computer with it
// (desktop_unified_signin §4): one sign-in, both directions. The only
// opt-in left is "Also delete chat history" (unticked every time the
// dialog opens); the computer logout is not optional any more.

import { Button, Dialog, Switch } from "@pidash/kit";
import * as React from "react";

import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import { registerSignOutPrompt } from "./signOutPrompt.js";

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
      description="Signing out removes this account's credentials from this computer. Its chats with the Pi Dash Agent stay on this computer, hidden until the account signs in again."
      open={isOpen}
      onOpenChange={() => settle(null)}
      actions={
        <>
          <Button variant="ghost" onClick={() => settle(null)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => settle({ deleteChatHistory, signOutComputer: false })}>
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
      </div>
    </Dialog>
  );
}
