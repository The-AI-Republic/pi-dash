// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer entry action (parity/specs/cycles.md §12, CYC-031). One
// component behind both entry points — the finished detail banner and
// the completed list row. The caller passes the entry's label and
// gates the mount (member or admin, transferable work, and for the
// banner an empty snapshot); the action owns the dialog state.

import { Button, type ButtonSize, type ButtonVariant } from "@pidash/kit";
import * as React from "react";

import { CycleTransferDialog } from "./CycleTransferDialog.js";

export interface CycleTransferActionProps {
  workspaceSlug: string;
  projectId: string;
  sourceCycleId: string;
  /** Entry label: the banner's plain words or the row's counted one. */
  label: string;
  variant?: ButtonVariant | undefined;
  size?: ButtonSize | undefined;
  className?: string | undefined;
}

export function CycleTransferAction({
  workspaceSlug,
  projectId,
  sourceCycleId,
  label,
  variant = "primary",
  size = "small",
  className,
}: CycleTransferActionProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <Button variant={variant} size={size} onClick={() => setOpen(true)} className={className}>
        {label}
      </Button>
      <CycleTransferDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        sourceCycleId={sourceCycleId}
        open={open}
        onClose={() => setOpen(false)}
      />
    </>
  );
}
