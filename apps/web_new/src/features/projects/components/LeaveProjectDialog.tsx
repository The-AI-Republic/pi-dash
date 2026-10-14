// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Leave-project dialog (parity/specs/shell.md §7, SHELL-073). Leaving is
// confirmed by typing the project ritual phrase, then the membership ends
// and the member lands back on the projects list.

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
  const [confirm, setConfirm] = React.useState("");
  const armed = confirm === LEAVE_CONFIRM_PHRASE;

  // Fresh phrase per visit: reopening never inherits a stale confirmation.
  React.useEffect(() => {
    setConfirm("");
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
      onNavigate(`/${workspaceSlug}/projects`);
    },
  });

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
          <Button
            variant="primary"
            disabled={!armed || leave.isPending}
            loading={leave.isPending}
            onClick={() => leave.mutate()}
          >
            {leave.isPending ? "Leaving…" : "Leave project"}
          </Button>
        </>
      }
    >
      <Input
        label={`To confirm, type ${LEAVE_CONFIRM_PHRASE} below:`}
        value={confirm}
        onChange={(event) => setConfirm(event.target.value)}
        placeholder={`Enter '${LEAVE_CONFIRM_PHRASE.toLowerCase()}'`}
      />
    </Dialog>
  );
}
