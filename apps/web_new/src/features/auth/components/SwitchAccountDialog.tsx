// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Switch-account dialog (AUTH-018). Owned by the auth area; shell and
// profile surfaces owned by later epics place the trigger. Names the
// active account, confirms through sign-out into a fresh login, and
// navigates to the signed-out entry on success. A failed sign-out toasts
// (in the hook) and the dialog stays open. Written from
// parity/specs/auth.md.

import { useNavigate } from "@tanstack/react-router";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useSwitchAccount } from "../api.js";

export interface SwitchAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The active account's address, named so the switch can be confirmed. */
  email: string;
}

export function SwitchAccountDialog({ open, onOpenChange, email }: SwitchAccountDialogProps): React.ReactElement {
  const navigate = useNavigate();
  const switchAccount = useSwitchAccount();
  const pending = switchAccount.isPending;

  const confirm = (): void => {
    switchAccount.mutate(undefined, {
      onSuccess: (landing) => {
        onOpenChange(false);
        void navigate({ href: landing });
      },
    });
  };

  return (
    <Dialog
      title="Switch accounts"
      open={open}
      onOpenChange={onOpenChange}
      nonDismissable={pending}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={pending}>
            Cancel
          </Button>
          <Button variant="primary" onClick={confirm} loading={pending}>
            Switch account
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <p className="text-body text-(--text)">
          You are signed in as <span className="font-medium">{email}</span>.
        </p>
        <p className="text-body text-(--text-muted)">
          Switching signs you out so you can sign in with a different address. Use this if you signed in with the wrong
          identity.
        </p>
      </div>
    </Dialog>
  );
}
