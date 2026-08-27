/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
// pi dash imports
import type { IWorkspaceJoinRequest, IWorkspaceMemberInvitation } from "@pi-dash/types";
import { ECreateOrJoinWorkspaceViews, EOnboardingSteps, EWorkspaceJoinRequestStatus } from "@pi-dash/types";
// hooks
import { useUser } from "@/hooks/store/user";
// local components
import {
  WorkspaceCreateStep,
  WorkspaceJoinByEmailStep,
  WorkspaceJoinInvitesStep,
  WorkspacePendingApprovalStep,
} from "./";

type Props = {
  invitations: IWorkspaceMemberInvitation[];
  joinRequests: IWorkspaceJoinRequest[];
  handleStepChange: (step: EOnboardingSteps, skipInvites?: boolean) => void;
};

export const WorkspaceSetupStep = observer(function WorkspaceSetupStep({
  invitations,
  joinRequests,
  handleStepChange,
}: Props) {
  // states
  const [currentView, setCurrentView] = useState<ECreateOrJoinWorkspaceViews | null>(null);
  // The admin email the user just requested, so the pending view can name it
  // before the join-requests list refetches.
  const [requestedAdminEmail, setRequestedAdminEmail] = useState<string | null>(null);
  // store hooks
  const { data: user } = useUser();

  // The user's outstanding request, if any — they land back here while waiting.
  const pendingRequest =
    joinRequests?.find((request) => request.status === EWorkspaceJoinRequestStatus.PENDING) ?? null;

  // Pick the initial view. A pending request takes precedence (the user is
  // mid-wait and should bounce back to it); otherwise invitations, else create.
  // Guarded by `joinRequests`/`invitations` deps only, so manual navigation
  // between views is never clobbered by a re-render.
  useEffect(() => {
    if (pendingRequest) {
      setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN_PENDING);
    } else if (invitations.length > 0) {
      setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN);
    } else {
      setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_CREATE);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [invitations, joinRequests]);

  const adminEmail = requestedAdminEmail ?? pendingRequest?.admin_email ?? "";

  switch (currentView) {
    case ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN:
      return (
        <WorkspaceJoinInvitesStep
          invitations={invitations}
          handleNextStep={async () => {
            handleStepChange(EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN, true);
          }}
          handleCurrentViewChange={() => setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_CREATE)}
        />
      );
    case ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN_BY_EMAIL:
      return (
        <WorkspaceJoinByEmailStep
          onRequestSent={(email) => {
            setRequestedAdminEmail(email);
            setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN_PENDING);
          }}
          onAlreadyMember={() => handleStepChange(EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN, true)}
          onBack={() => setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_CREATE)}
        />
      );
    case ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN_PENDING:
      return (
        <WorkspacePendingApprovalStep
          adminEmail={adminEmail}
          onCreateInstead={() => {
            setRequestedAdminEmail(null);
            setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_CREATE);
          }}
        />
      );
    default:
      return (
        <WorkspaceCreateStep
          user={user}
          onComplete={(skipInvites) => handleStepChange(EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN, skipInvites)}
          handleCurrentViewChange={() => setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN)}
          handleJoinByEmailView={() => setCurrentView(ECreateOrJoinWorkspaceViews.WORKSPACE_JOIN_BY_EMAIL)}
          hasInvitations={invitations.length > 0}
        />
      );
  }
});
