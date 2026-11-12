// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General-tab danger zone (parity/specs/profile.md §6, PROF-014). The
// section appears only on self-managed instances; confirming deletes the
// account and the auth flow signs the member out to the signed-out entry.
// Cancelling leaves the account untouched.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export const DEACTIVATE_DESCRIPTION =
  "When deactivating an account, all of the data and resources within that account will be permanently removed and cannot be recovered.";

export interface DeactivateAccountSectionProps {
  confirming: boolean;
  onConfirm: () => void;
}

export function DeactivateAccountSection({ confirming, onConfirm }: DeactivateAccountSectionProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);

  return (
    <section aria-labelledby="deactivate-heading" className="mt-(--space-8) border-t border-(--border) pt-(--space-6)">
      <div className="flex items-start justify-between gap-(--space-6)">
        <div className="flex max-w-lg flex-col gap-(--space-2)">
          <h2 id="deactivate-heading" className="text-emphasis font-medium text-(--text)">
            Deactivate account
          </h2>
          <p className="text-body text-(--text-muted)">{DEACTIVATE_DESCRIPTION}</p>
        </div>
        <Button variant="danger" onClick={() => setOpen(true)}>
          Deactivate account
        </Button>
      </div>

      <Dialog
        title="Deactivate your account"
        open={open}
        onOpenChange={(next) => {
          if (confirming) return;
          setOpen(next);
        }}
        nonDismissable={confirming}
        actions={
          <>
            <Button variant="secondary" onClick={() => setOpen(false)} disabled={confirming}>
              Cancel
            </Button>
            <Button
              variant="danger"
              loading={confirming}
              onClick={() => {
                onConfirm();
              }}
            >
              Deactivate account
            </Button>
          </>
        }
      >
        <p className="text-body text-(--text)">
          This removes the account permanently. You will be signed out, and returning needs a fresh workspace
          invitation.
        </p>
      </Dialog>
    </section>
  );
}
