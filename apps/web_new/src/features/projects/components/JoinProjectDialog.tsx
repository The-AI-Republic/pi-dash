// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Join-project dialog (SHELL-035). Confirming adds the caller to the
// project and lands them in its work items; the shared join mutation
// refreshes the workspace subtree so the card flips to member state. A
// refused join toasts and keeps the dialog open.
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useJoinProject } from "../../auth/index.js";
import { projectIssuesPath } from "./cardMenu.js";

export interface JoinProjectDialogProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (href: string) => void;
}

export function JoinProjectDialog({
  workspaceSlug,
  projectId,
  projectName,
  open,
  onOpenChange,
  onNavigate,
}: JoinProjectDialogProps): React.ReactElement {
  const join = useJoinProject();
  const issuesPath = projectIssuesPath(workspaceSlug, projectId);

  function confirm(): void {
    join.mutate(
      { slug: workspaceSlug, projectId },
      {
        onSuccess: () => {
          onOpenChange(false);
          onNavigate(issuesPath);
        },
      }
    );
  }

  return (
    <Dialog
      title="Join Project?"
      description={`Joining adds you to ${projectName}. You'll land in its work items as a member.`}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={join.isPending} onClick={confirm}>
            {join.isPending ? "Joining…" : "Join Project"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        Public projects let any workspace member join. You can leave again later from the project menu.
      </p>
    </Dialog>
  );
}
