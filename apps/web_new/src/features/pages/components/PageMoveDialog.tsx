// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-project page move (parity/specs/pages.md §3.8 and §11.1, PAGE-017,
// PDASHOSS01-347). A project admin picks a target from the shared project
// picker, narrowed to the other live projects the viewer can write to, and
// confirms; the page and its sub-pages then leave this project's collection
// and appear in the target's. The move is a different feature from the
// nested reparent of PAGE-087 and never shares a dialog with it.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { ProjectPicker } from "../../../shared/pickers/ProjectPicker.js";
import { usePickerProjects } from "../../../shared/pickers/reference.js";
import { pageDisplayTitle } from "../lib/pageList.js";
import { pageMoveProjectIds } from "../lib/pageProjectMove.js";

/** Shown once the project read settles with nowhere to move the page. */
export const PAGE_MOVE_NO_TARGETS = "There is no other project you can move this page into.";

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
      <PageMoveTargetField
        workspaceSlug={workspaceSlug}
        currentProjectId={currentProjectId}
        value={target}
        pending={pending}
        onChange={setTarget}
      />
    </Dialog>
  );
}

interface PageMoveTargetFieldProps {
  workspaceSlug: string;
  currentProjectId: string;
  value: string | null;
  pending: boolean;
  onChange: (projectId: string) => void;
}

/**
 * The picker body. It mounts only while the dialog is open, so the project
 * read never runs for the closed dialog every row carries.
 */
function PageMoveTargetField({
  workspaceSlug,
  currentProjectId,
  value,
  pending,
  onChange,
}: PageMoveTargetFieldProps): React.ReactElement {
  const projects = usePickerProjects(workspaceSlug, true);
  const targetIds = React.useMemo(
    () => pageMoveProjectIds(projects.data ?? [], currentProjectId),
    [projects.data, currentProjectId]
  );
  const noTargets = projects.isSuccess && targetIds.length === 0;
  return (
    <>
      <ProjectPicker
        workspaceSlug={workspaceSlug}
        value={value}
        includeProjectIds={targetIds}
        excludeProjectIds={[currentProjectId]}
        disabled={pending || noTargets}
        onChange={onChange}
        placeholder="Select a project"
      />
      {noTargets ? (
        <p data-testid="page-move-no-targets" className="text-body text-(--text-muted)">
          {PAGE_MOVE_NO_TARGETS}
        </p>
      ) : null}
    </>
  );
}
