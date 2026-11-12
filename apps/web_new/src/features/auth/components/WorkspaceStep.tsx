// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace create-or-join onboarding step (AUTH-034, AUTH-036). The four
// sub-views under one step: the default resolves by precedence (a pending
// join request forces the holding view, else outstanding invites default
// to the invites view, else the create view), and each sub-view either
// navigates or advances the flow. Creating advances to the invite step,
// except the solo size and the already-member shortcut, which finish
// directly. Written from parity/specs/auth.md.

import { Button } from "@pidash/kit";
import type { WorkspaceInvitationList, WorkspaceJoinRequestList } from "@pidash/api-client";
import * as React from "react";

import {
  isSoloTeamSize,
  resolvePendingEmail,
  resolveWorkspaceDefaultView,
  selectPendingJoinRequest,
  type WorkspaceStepView,
} from "../workspace.js";
import { InOnboardingCreationNotice } from "./InOnboardingCreationNotice.js";
import { WorkspaceCreateForm } from "./WorkspaceCreateForm.js";
import { WorkspaceInvitesView } from "./WorkspaceInvitesView.js";
import { WorkspaceJoinByEmailView } from "./WorkspaceJoinByEmailView.js";
import { WorkspacePendingView } from "./WorkspacePendingView.js";

export interface WorkspaceStepProps {
  onAdvance: (options?: { skipInvites?: boolean }) => void;
  invitations: WorkspaceInvitationList;
  joinRequests: WorkspaceJoinRequestList;
  /**
   * True when the instance config disables workspace creation (AUTH-041):
   * the create view is replaced by the inline restricted notice steering
   * to joining. Absent reads as enabled.
   */
  creationDisabled?: boolean | undefined;
}

export function WorkspaceStep({
  onAdvance,
  invitations,
  joinRequests,
  creationDisabled = false,
}: WorkspaceStepProps): React.ReactElement {
  // Null until the user navigates, so the default tracks late-loading data
  // (the join-request list resolves after first paint) without ever
  // clobbering manual navigation on a refetch.
  const [view, setView] = React.useState<WorkspaceStepView | null>(null);
  // The admin email the user just requested, so the holding view can name
  // it before the join-request list refetches.
  const [requestedEmail, setRequestedEmail] = React.useState<string | null>(null);

  const pendingRequest = selectPendingJoinRequest(joinRequests);
  const current =
    view ??
    resolveWorkspaceDefaultView({
      hasPendingRequest: pendingRequest !== null,
      hasInvitations: invitations.length > 0,
    });

  switch (current) {
    case "invites":
      return (
        <WorkspaceInvitesView
          invitations={invitations}
          onAccepted={() => onAdvance({ skipInvites: true })}
          onCreateNew={() => setView("create")}
        />
      );
    case "join-by-email":
      return (
        <WorkspaceJoinByEmailView
          onRequested={(email) => {
            setRequestedEmail(email);
            setView("pending");
          }}
          onAlreadyMember={() => onAdvance({ skipInvites: true })}
          onBack={() => setView("create")}
        />
      );
    case "pending":
      return (
        <WorkspacePendingView
          adminEmail={resolvePendingEmail(requestedEmail, pendingRequest)}
          onCreateInstead={() => {
            setRequestedEmail(null);
            setView("create");
          }}
        />
      );
    case "create":
      if (creationDisabled) {
        return <InOnboardingCreationNotice onJoin={() => setView("join-by-email")} />;
      }
      return (
        <div className="flex flex-col gap-(--space-4)">
          <WorkspaceCreateForm
            onCreated={(_workspace, size) => onAdvance(isSoloTeamSize(size) ? { skipInvites: true } : undefined)}
          />
          <div className="flex flex-col gap-(--space-2)">
            {invitations.length > 0 ? (
              <Button type="button" variant="ghost" size="large" onClick={() => setView("invites")}>
                Join with an invite
              </Button>
            ) : null}
            <Button type="button" variant="ghost" size="large" onClick={() => setView("join-by-email")}>
              Join by admin email
            </Button>
          </div>
        </div>
      );
  }
}
