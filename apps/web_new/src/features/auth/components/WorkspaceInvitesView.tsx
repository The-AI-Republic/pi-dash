// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace invites sub-view (AUTH-034). Checkbox cards for the caller's
// outstanding invitations with a continue action that batch-accepts the
// selection and finishes onboarding, plus a ghost action back to the
// create view. Written from parity/specs/auth.md.

import { Avatar, Button } from "@pidash/kit";
import type { WorkspaceInvitationList } from "@pidash/api-client";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useAcceptWorkspaceInvitations } from "../api.js";
import { workspaceRoleLabel } from "../workspace.js";

export const INVITES_FAILED_TITLE = "Could not join the workspaces";
export const INVITES_EMPTY_MESSAGE = "No invitations found.";

export interface WorkspaceInvitesViewProps {
  invitations: WorkspaceInvitationList;
  /** Called after the selected invites are accepted; the caller finishes onboarding. */
  onAccepted: () => void;
  onCreateNew: () => void;
}

export function WorkspaceInvitesView({
  invitations,
  onAccepted,
  onCreateNew,
}: WorkspaceInvitesViewProps): React.ReactElement {
  const accept = useAcceptWorkspaceInvitations();
  const [selected, setSelected] = React.useState<string[]>([]);
  const joining = accept.isPending;

  const toggle = (id: string): void => {
    setSelected((current) => (current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id]));
  };

  const submit = async (): Promise<void> => {
    try {
      await accept.mutateAsync(selected);
      onAccepted();
    } catch (error) {
      notifyError(INVITES_FAILED_TITLE, (error as Error).message);
    }
  };

  if (invitations.length === 0) {
    return (
      <section aria-labelledby="invites-heading" className="flex flex-col gap-(--space-6)">
        <div className="flex flex-col gap-(--space-2)">
          <h1 id="invites-heading" className="text-h2 text-(--text)">
            Join a workspace
          </h1>
          <p className="text-body text-(--text-muted)">{INVITES_EMPTY_MESSAGE}</p>
        </div>
        <Button type="button" variant="ghost" size="large" onClick={onCreateNew}>
          Create a new workspace
        </Button>
      </section>
    );
  }

  return (
    <section aria-labelledby="invites-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="invites-heading" className="text-h2 text-(--text)">
          Join a workspace
        </h1>
        <p className="text-body text-(--text-muted)">Choose the workspaces you would like to join.</p>
      </div>

      <div className="flex flex-col gap-(--space-2)">
        {invitations.map((invitation) => {
          const checked = selected.includes(invitation.id);
          const workspace = invitation.workspace;
          return (
            <label
              key={invitation.id}
              className="text-body flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-3) text-(--text)"
            >
              <input type="checkbox" checked={checked} onChange={() => toggle(invitation.id)} />
              <Avatar name={workspace.name} size="small" {...(workspace.logo_url ? { src: workspace.logo_url } : {})} />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-medium">{workspace.name}</span>
                <span className="text-caption text-(--text-muted)">{workspaceRoleLabel(invitation.role)}</span>
              </span>
            </label>
          );
        })}
      </div>

      <div className="flex flex-col gap-(--space-2)">
        <Button
          type="button"
          variant="primary"
          size="large"
          loading={joining}
          disabled={joining || selected.length === 0}
          onClick={() => void submit()}
        >
          Continue
        </Button>
        <Button type="button" variant="ghost" size="large" onClick={onCreateNew} disabled={joining}>
          Create a new workspace
        </Button>
      </div>
    </section>
  );
}
