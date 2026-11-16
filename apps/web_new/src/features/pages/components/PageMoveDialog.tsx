// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-project page move (parity/specs/pages.md §3.8 and §11.1, PAGE-017).
// Cloud only: the community build never mounts this dialog because the row
// menu carries no entry there. The page owner or a project admin picks a
// target project from the shared project picker and confirms; the page then
// leaves this project's collection and appears in the target's. The move
// is a different feature from the nested reparent of PAGE-087 and never
// shares a dialog with it.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { ProjectPicker } from "../../../shared/pickers/ProjectPicker.js";
import { pageDisplayTitle } from "../lib/pageList.js";

export interface PageMoveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  /** The project the page lives in now: offered nowhere in the picker. */
  currentProjectId: string;
  pageName: string;
  pending: boolean;
  onConfirm: (newProjectId: string) => void;
}

export function PageMoveDialog({
  open,
  onOpenChange,
  workspaceSlug,
  currentProjectId,
  pageName,
  pending,
  onConfirm,
}: PageMoveDialogProps): React.ReactElement {
  const [target, setTarget] = React.useState<string | null>(null);
  const title = pageDisplayTitle(pageName);

  React.useEffect(() => {
    if (open) setTarget(null);
  }, [open]);

  return (
    <Dialog
      title="Move page"
      description={`Choose another project for “${title}”.`}
      open={open}
      onOpenChange={(next) => {
        if (!next && pending) return;
        onOpenChange(next);
      }}
      actions={
        <>
          <Button variant="secondary" type="button" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            type="button"
            loading={pending}
            disabled={pending || target === null}
            onClick={() => {
              if (target !== null) onConfirm(target);
            }}
          >
            {pending ? "Moving…" : "Move page"}
          </Button>
        </>
      }
    >
      <ProjectPicker
        workspaceSlug={workspaceSlug}
        value={target}
        excludeProjectIds={[currentProjectId]}
        onChange={setTarget}
        placeholder="Select a project"
      />
    </Dialog>
  );
}
