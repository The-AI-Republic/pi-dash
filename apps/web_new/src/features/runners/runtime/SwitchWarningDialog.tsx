// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The pre-switch warning (desktop_unified_signin §6.4): an account switch
// with runs in flight asks before it stops them, and switches only on
// confirm. Lazy-loaded with the rest of the runtime UI on first use.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface SwitchWarningCopy {
  from: string;
  to: string;
  active: number;
}

export function SwitchWarningDialog({
  copy,
  onConfirm,
  onCancel,
}: {
  copy: SwitchWarningCopy;
  onConfirm: () => void;
  onCancel: () => void;
}): React.ReactElement {
  return (
    <Dialog
      title="Switch this computer's sign-in?"
      description={`Pi Dash on this computer is running ${copy.active} task(s) for ${copy.from}; signing in as ${copy.to} stops them (they end as failed and can be retried by hand).`}
      open
      onOpenChange={() => undefined}
      actions={
        <>
          <Button variant="ghost" onClick={onCancel}>
            Keep the app only
          </Button>
          <Button variant="primary" onClick={onConfirm}>
            Sign in and switch
          </Button>
        </>
      }
    >
      <></>
    </Dialog>
  );
}
