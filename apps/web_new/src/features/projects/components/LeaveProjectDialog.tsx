// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Leave-project dialog (parity/specs/shell.md §7, SHELL-073; flow owned
// by SHELL-036). Leaving is confirmed twice: the exact project name,
// then the ritual phrase. Wrong entries block with corrective errors;
// correct entries end the membership and land back on the projects
// list. One canonical dialog for every surface (tab chrome, sidebar,
// settings) that offers leaving.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { leaveProject } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { invalidateWorkspace } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

/** Confirmation phrase that arms the leave button. */
export const LEAVE_CONFIRM_PHRASE = "Leave Project";

const LEFT_TITLE = "Left the project";
const LEAVE_FAILED_TITLE = "Couldn't leave the project";

export interface LeaveProjectDialogProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (href: string) => void;
}

export function LeaveProjectDialog({
  workspaceSlug,
  projectId,
  projectName,
  open,
  onOpenChange,
  onNavigate,
}: LeaveProjectDialogProps): React.ReactElement {
  const queryClient = useQueryClient();
  const [typedName, setTypedName] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [nameError, setNameError] = React.useState<string | null>(null);
  const [phraseError, setPhraseError] = React.useState<string | null>(null);

  // Fresh confirmations per visit: reopening never inherits stale input.
  React.useEffect(() => {
    setTypedName("");
    setConfirm("");
    setNameError(null);
    setPhraseError(null);
  }, [open]);

  const leave = useMutation({
    mutationFn: async () => {
      try {
        await leaveProject(getAppClient(), workspaceSlug, projectId);
        await invalidateWorkspace(queryClient, workspaceSlug);
      } catch (error) {
        notifyError(LEAVE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSuccess: () => {
      notifySuccess(LEFT_TITLE, `${projectName} is off your project list.`);
      onOpenChange(false);
    },
  });

  function submit(): void {
    if (typedName !== projectName) {
      setNameError(`Enter the project name "${projectName}" exactly as shown.`);
      return;
    }
    setNameError(null);
    if (confirm !== LEAVE_CONFIRM_PHRASE) {
      setPhraseError(`Type "${LEAVE_CONFIRM_PHRASE}" exactly to confirm leaving.`);
      return;
    }
    setPhraseError(null);
    // The list route lands before the leave request fires (PSET-015), so a
    // held write leaves the caller on the list with membership intact.
    onOpenChange(false);
    onNavigate(`/${workspaceSlug}/projects`);
    leave.mutate();
  }

  return (
    <Dialog
      title="Leave project"
      description={`Leaving removes you from ${projectName}. Your assigned work stays behind for the team.`}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" disabled={leave.isPending} loading={leave.isPending} onClick={submit}>
            {leave.isPending ? "Leaving…" : "Leave project"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label={`Enter the project name ${projectName} to continue:`}
          value={typedName}
          onChange={(event) => setTypedName(event.target.value)}
          placeholder="Enter the project name"
          {...(nameError === null ? {} : { error: nameError })}
        />
        <Input
          label={`To confirm, type ${LEAVE_CONFIRM_PHRASE} below:`}
          value={confirm}
          onChange={(event) => setConfirm(event.target.value)}
          placeholder={`Enter '${LEAVE_CONFIRM_PHRASE.toLowerCase()}'`}
          {...(phraseError === null ? {} : { error: phraseError })}
        />
      </div>
    </Dialog>
  );
}
