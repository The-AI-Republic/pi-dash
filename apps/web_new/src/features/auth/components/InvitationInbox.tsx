// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Invitation inbox screen (AUTH-026/028). Written from
// parity/specs/auth.md §§AUTH-026/028: one selectable card per pending
// invite, a primary action joining every selected workspace at once, a way
// home without joining, and an illustrated empty state. Cards are real
// buttons so selection stays keyboard-reachable (AUTH-025). The list and
// the join come from the shared invitation hooks; this screen adds the
// stale-selection guard, the last-workspace stamp, and the landing.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Avatar, Button, Spinner } from "@pidash/kit";
import type { WorkspaceInvitationList } from "@pidash/api-client";
import * as React from "react";

import { notifyError, useDocumentHead } from "../../../shared/shell/index.js";
import { useAcceptWorkspaceInvitations } from "../api.js";
import { useUpdateProfile } from "../profile.js";
import { selectBatchLandingTarget, selectInvitationRoleLabel, toggleInvitationSelection } from "../invitations.js";
import { useMyWorkspaceInvitations } from "../invitations.js";
import { authKeys } from "../queries.js";

export const INVITATIONS_TITLE = "Workspace invitations";
export const NO_INVITES_ART = "/illustrations/no-invites.svg";
export const BATCH_ACCEPT_FAILED_TITLE = "Could not join the selected workspaces";
export const BATCH_ACCEPT_STALE_MESSAGE = "Those invitations are no longer pending. Refresh the list and try again.";

const CARD_RING =
  "focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-(--focus-ring-color) focus-visible:outline-offset-1";

export function InvitationInbox(): React.ReactElement {
  const [selected, setSelected] = React.useState<string[]>([]);
  const [joining, setJoining] = React.useState(false);
  const invitationsQuery = useMyWorkspaceInvitations();
  const accept = useAcceptWorkspaceInvitations();
  const stampLastWorkspace = useUpdateProfile();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  useDocumentHead({ title: INVITATIONS_TITLE, robots: "noindex, nofollow" });

  if (invitationsQuery.isPending) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-(--bg)">
        <Spinner label="Loading invitations" />
      </main>
    );
  }

  if (invitationsQuery.isError) {
    return (
      <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col items-start justify-center gap-(--space-3) bg-(--bg) p-(--space-8)">
        <h1 className="text-h2 text-(--text)">Could not load invitations</h1>
        <p className="text-body text-(--text-muted)">
          Something went wrong while loading your invitations. Try again, or head home.
        </p>
        <div className="flex items-center gap-(--space-3)">
          <Button
            variant="primary"
            onClick={() => {
              void invitationsQuery.refetch();
            }}
          >
            Retry
          </Button>
          <Button
            variant="secondary"
            onClick={() => {
              void navigate({ href: "/" });
            }}
          >
            Back to home
          </Button>
        </div>
      </main>
    );
  }

  const invitations = invitationsQuery.data;
  if (invitations.length === 0) {
    return (
      <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
        <img src={NO_INVITES_ART} alt="An empty inbox tray with an envelope above it" width={320} height={240} />
        <h1 className="text-h2 text-(--text)">No pending invites</h1>
        <p className="text-body text-(--text-muted)">When someone invites you to a workspace, it will appear here.</p>
        <Button
          variant="primary"
          onClick={() => {
            void navigate({ href: "/" });
          }}
        >
          Back to home
        </Button>
      </main>
    );
  }

  const submit = async (): Promise<void> => {
    // Fail closed when the list no longer names the selection (or nothing
    // is selected) instead of landing on an unknown workspace.
    const target = selectBatchLandingTarget(invitations, selected);
    if (target === null) {
      notifyError(BATCH_ACCEPT_FAILED_TITLE, BATCH_ACCEPT_STALE_MESSAGE);
      return;
    }
    setJoining(true);
    try {
      await accept.mutateAsync(selected);
      await stampLastWorkspace.mutateAsync({ last_workspace_id: target.id });
    } catch (error) {
      // The client throws ApiError for every failure, so the message is
      // always the server detail.
      notifyError(BATCH_ACCEPT_FAILED_TITLE, (error as Error).message);
      return;
    } finally {
      setJoining(false);
    }
    await queryClient.invalidateQueries({ queryKey: authKeys.myInvitations });
    void navigate({ href: `/${target.slug}` });
  };

  return (
    <InboxList
      invitations={invitations}
      selected={selected}
      joining={joining}
      onToggle={(id) => {
        setSelected(toggleInvitationSelection(selected, id));
      }}
      onAccept={() => {
        void submit();
      }}
      onHome={() => {
        void navigate({ href: "/" });
      }}
    />
  );
}

function InboxList({
  invitations,
  selected,
  joining,
  onToggle,
  onAccept,
  onHome,
}: {
  invitations: WorkspaceInvitationList;
  selected: string[];
  joining: boolean;
  onToggle: (id: string) => void;
  onAccept: () => void;
  onHome: () => void;
}): React.ReactElement {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col justify-center gap-(--space-6) bg-(--bg) p-(--space-8)">
      <div>
        <h1 className="text-h2 text-(--text)">Workspace invitations</h1>
        <p className="text-body mt-(--space-2) text-(--text-muted)">Select the workspaces you want to join.</p>
      </div>
      <ul className="flex flex-col gap-(--space-3)">
        {invitations.map((invitation) => {
          const isSelected = selected.includes(invitation.id);
          return (
            <li key={invitation.id}>
              <button
                type="button"
                aria-pressed={isSelected}
                onClick={() => {
                  onToggle(invitation.id);
                }}
                className={`flex w-full cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) border px-(--space-4) py-(--space-3) text-left ${CARD_RING} ${
                  isSelected
                    ? "border-(--accent) bg-(--accent-subtle)"
                    : "border-(--border) bg-(--surface) hover:bg-(--subtle)"
                }`}
              >
                <Avatar
                  name={invitation.workspace.name}
                  {...(invitation.workspace.logo_url === null ? {} : { src: invitation.workspace.logo_url })}
                  size="large"
                />
                <span className="min-w-0 flex-1">
                  <span className="text-body block truncate font-medium text-(--text)">
                    {invitation.workspace.name}
                  </span>
                  <span className="text-meta block text-(--text-muted)">
                    {selectInvitationRoleLabel(invitation.role)}
                  </span>
                </span>
                <span
                  aria-hidden="true"
                  className={`flex size-6 shrink-0 items-center justify-center rounded-(--radius-full) border ${
                    isSelected
                      ? "border-(--accent) bg-(--accent) text-(--on-accent)"
                      : "border-(--border) text-transparent"
                  }`}
                >
                  <svg viewBox="0 0 16 16" className="size-4" fill="none" stroke="currentColor" strokeWidth="2.5">
                    <polyline points="3,8.5 6.5,12 13,4.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="flex items-center gap-(--space-3)">
        <Button
          variant="primary"
          size="large"
          loading={joining}
          disabled={selected.length === 0 || joining}
          onClick={onAccept}
        >
          Accept selected
        </Button>
        <Button variant="secondary" size="large" onClick={onHome}>
          Go home
        </Button>
      </div>
    </main>
  );
}
