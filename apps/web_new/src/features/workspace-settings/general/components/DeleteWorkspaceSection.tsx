// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete-workspace danger zone (parity/specs/workspace-settings.md §6,
// WSET-006). A boxed control names workspace deletion and its
// irreversibility; its Delete button opens a dialog naming the workspace
// and requiring both the exact workspace name and the fixed phrase.
// Cancel or dismiss resets the inputs and writes nothing; Confirm deletes,
// closes, refreshes the landing reads and navigates away.

import { Button, Dialog, Input } from "@pidash/kit";
import { AlertTriangle } from "lucide-react";
import * as React from "react";

import { useDeleteWorkspace } from "../api/mutations.js";
import { canConfirmWorkspaceDelete, DELETE_CONFIRM_PHRASE } from "../rules.js";

export const DELETE_SECTION_TITLE = "Delete this workspace";
export const DELETE_SECTION_DESCRIPTION =
  "Deleting a workspace removes all of its data and resources permanently. Nothing can be recovered afterwards.";
export const DELETE_DIALOG_TITLE = "Delete this workspace?";

export interface DeleteWorkspaceSectionProps {
  workspaceSlug: string;
  workspaceName: string;
  /** Where a successful delete lands. */
  destination: string;
  onNavigate: (path: string) => void;
}

export function DeleteWorkspaceSection({
  workspaceSlug,
  workspaceName,
  destination,
  onNavigate,
}: DeleteWorkspaceSectionProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [typedName, setTypedName] = React.useState("");
  const [typedPhrase, setTypedPhrase] = React.useState("");
  const removal = useDeleteWorkspace(workspaceSlug);
  const canConfirm = canConfirmWorkspaceDelete({ workspaceName, typedName, typedPhrase });

  const reset = (): void => {
    setTypedName("");
    setTypedPhrase("");
  };
  const close = (): void => {
    setOpen(false);
    reset();
  };

  const confirm = (): void => {
    if (!canConfirm || removal.isPending) return;
    removal.mutate(undefined, {
      onSuccess: () => {
        setOpen(false);
        reset();
        onNavigate(destination);
      },
    });
  };

  return (
    <>
      <section
        data-testid="workspace-delete-section"
        className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-6) lg:flex-row lg:items-center lg:justify-between"
      >
        <div className="flex flex-col gap-(--space-1)">
          <h2 className="text-emphasis text-(--text)">{DELETE_SECTION_TITLE}</h2>
          <p className="text-body max-w-xl text-(--text-muted)">{DELETE_SECTION_DESCRIPTION}</p>
        </div>
        <Button variant="danger" onClick={() => setOpen(true)}>
          Delete
        </Button>
      </section>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (next) setOpen(true);
          else close();
        }}
        title={DELETE_DIALOG_TITLE}
        actions={
          <div className="flex items-center gap-(--space-2)">
            <Button variant="secondary" onClick={close}>
              Cancel
            </Button>
            <Button variant="danger" loading={removal.isPending} disabled={!canConfirm} onClick={confirm}>
              {removal.isPending ? "Deleting" : "Confirm"}
            </Button>
          </div>
        }
      >
        <form
          className="flex flex-col gap-(--space-4)"
          onSubmit={(event) => {
            event.preventDefault();
            confirm();
          }}
        >
          <div className="flex items-start gap-(--space-4)">
            <span className="grid size-10 shrink-0 place-items-center rounded-full bg-(--subtle) text-(--danger)">
              <AlertTriangle size={20} strokeWidth={1.5} aria-hidden="true" />
            </span>
            <p className="text-body text-(--text)">
              You are about to delete the workspace <strong className="break-words">{workspaceName}</strong>. If you
              confirm, you will lose access to all of its work data with no way to restore it.
            </p>
          </div>
          <Input
            id="workspaceName"
            name="workspaceName"
            label="Type the workspace name to continue"
            placeholder={workspaceName}
            value={typedName}
            autoComplete="off"
            onChange={(event) => setTypedName(event.target.value)}
          />
          <Input
            id="confirmDelete"
            name="confirmDelete"
            label={`For final confirmation, type ${DELETE_CONFIRM_PHRASE}`}
            value={typedPhrase}
            autoComplete="off"
            onChange={(event) => setTypedPhrase(event.target.value)}
          />
        </form>
      </Dialog>
    </>
  );
}
