// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project archive/restore dialog (SHELL-037; written from
// parity/specs/shell.md §3). One confirm flow in both directions:
// archiving warns that the project leaves lists and search,
// restoring brings it back. Either success toasts and returns to
// the list; a refused write toasts and keeps the dialog open. The
// list cards and the future settings danger zone share this
// component.
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useArchiveProject, useRestoreProject } from "../api/mutations.js";

export type ArchiveDialogMode = "archive" | "restore";

export interface ProjectArchiveDialogProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  mode: ArchiveDialogMode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (href: string) => void;
}

/**
 * Warning copy per direction; the archive line names the archived state.
 * Archiving takes the whole project with it — work items, cycles, modules
 * and pages leave the lists and search together (parity/specs/archives.md
 * §6, ARCH-026) — so the warning spells each out instead of naming only
 * the work items.
 */
export function selectArchiveBody(mode: ArchiveDialogMode, projectName: string): string {
  if (mode === "restore") {
    return `Restoring a project brings ${projectName} back to the projects list and makes it visible to its members again.`;
  }
  return `${projectName} will be archived: it leaves the projects list, and its work items, cycles, modules and pages disappear from search until a project admin restores it.`;
}

export function ProjectArchiveDialog({
  workspaceSlug,
  projectId,
  projectName,
  mode,
  open,
  onOpenChange,
  onNavigate,
}: ProjectArchiveDialogProps): React.ReactElement {
  const archive = useArchiveProject();
  const restore = useRestoreProject();
  const pending = archive.isPending || restore.isPending;
  const listPath = `/${workspaceSlug}/projects`;

  function confirm(): void {
    const input = { workspaceSlug, projectId, projectName };
    if (mode === "restore") {
      restore.mutate(input, {
        onSuccess: () => {
          onOpenChange(false);
          onNavigate(listPath);
        },
      });
      return;
    }
    archive.mutate(input, {
      onSuccess: () => {
        onOpenChange(false);
        onNavigate(listPath);
      },
    });
  }

  const action = mode === "restore" ? "Restore" : "Archive";
  return (
    <Dialog
      title={`${action} ${projectName}`}
      description={selectArchiveBody(mode, projectName)}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={confirm}>
            {pending ? (mode === "restore" ? "Restoring…" : "Archiving…") : action}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        {mode === "restore"
          ? "Members keep their roles and everything reappears where it was."
          : "Nothing is deleted — the project waits in the archive view until it is restored or removed."}
      </p>
    </Dialog>
  );
}
