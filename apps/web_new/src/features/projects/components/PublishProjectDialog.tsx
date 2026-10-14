// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Publish dialog shell (parity/specs/shell.md §7, SHELL-073). The entry
// opens this dialog; the publish form itself belongs to the space area,
// which mounts it on the anchor below.

import { Dialog } from "@pidash/kit";
import * as React from "react";

export interface PublishProjectDialogProps {
  projectName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function PublishProjectDialog({
  projectName,
  open,
  onOpenChange,
}: PublishProjectDialogProps): React.ReactElement {
  return (
    <Dialog
      title="Publish project"
      description={`${projectName} can go public on a shared board.`}
      open={open}
      onOpenChange={onOpenChange}
    >
      {/* Space-area seam: the publish form mounts here (SHELL-073 proves
          the heading; the form rows belong to the space epic). */}
      <section aria-label="Publish settings" data-testid="publish-settings-mount" />
    </Dialog>
  );
}
