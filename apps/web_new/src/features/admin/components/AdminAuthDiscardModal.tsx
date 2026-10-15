// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unsaved-changes discard modal (ADM-034, from parity/specs/admin.md
// §4.12): keep-editing shuts it with the draft preserved, and the
// leave link round-trips to the authentication landing page. Any
// dialog dismiss (Escape, backdrop) keeps the edits. Wording is pinned
// by the parity scenario.
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import {
  PROVIDER_BACK_LABEL,
  PROVIDER_DISCARD_BODY,
  PROVIDER_DISCARD_KEEP_LABEL,
  PROVIDER_DISCARD_TITLE,
} from "../providerAuth.js";

export interface AdminAuthDiscardModalProps {
  open: boolean;
  /** Landing href the leave link round-trips to. */
  backHref: string;
  /** Shut the modal with the draft preserved. */
  onKeep: () => void;
}

export function AdminAuthDiscardModal({ open, backHref, onKeep }: AdminAuthDiscardModalProps): React.ReactElement {
  return (
    <Dialog
      open={open}
      onOpenChange={onKeep}
      title={PROVIDER_DISCARD_TITLE}
      description={PROVIDER_DISCARD_BODY}
      actions={
        <>
          <Button variant="secondary" onClick={onKeep} data-testid="admin-auth-discard-keep">
            {PROVIDER_DISCARD_KEEP_LABEL}
          </Button>
          <a
            href={backHref}
            data-testid="admin-auth-discard-leave"
            className="text-body inline-flex h-(--control-height) cursor-pointer items-center justify-center rounded-(--radius-control) bg-(--accent) px-(--space-4) font-medium whitespace-nowrap text-(--on-accent) hover:bg-(--accent-hover)"
          >
            {PROVIDER_BACK_LABEL}
          </a>
        </>
      }
    >
      {null}
    </Dialog>
  );
}
