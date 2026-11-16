// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Deactivate-account dialog (AUTH-019). Owned by the auth area; the
// settings page owned by NEWFRONT-90 places the trigger. Explains the
// consequences, confirms the deactivation call, and on success toasts,
// signs out, and returns to the signed-out entry. A failed deactivation
// toasts the server detail (in the hook) and leaves the account signed
// in. Written from parity/specs/auth.md.

import { useNavigate } from "@tanstack/react-router";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useDeactivateAccount } from "../api.js";

export interface DeactivateAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The account address, named in the way back. */
  email: string;
}

export function DeactivateAccountDialog({
  open,
  onOpenChange,
  email,
}: DeactivateAccountDialogProps): React.ReactElement {
  const navigate = useNavigate();
  const deactivate = useDeactivateAccount();
  const pending = deactivate.isPending;

  const confirm = (): void => {
    deactivate.mutate(undefined, {
      onSuccess: (landing) => {
        onOpenChange(false);
        void navigate({ href: landing });
      },
    });
  };

  return (
    <Dialog
      title="Deactivate account"
      open={open}
      onOpenChange={onOpenChange}
      nonDismissable={pending}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={pending}>
            Cancel
          </Button>
          <Button variant="danger" onClick={confirm} loading={pending}>
            Deactivate account
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <p className="text-body text-(--text)">
          Deactivating removes your assignments and billing standing. The only way back is a fresh workspace invitation
          sent to <span className="font-medium">{email}</span>.
        </p>
      </div>
    </Dialog>
  );
}
