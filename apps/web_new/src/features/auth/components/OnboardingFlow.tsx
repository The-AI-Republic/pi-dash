// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Onboarding flow (AUTH-029, AUTH-043). The stateful counterpart to the
// shell: it loads the account, workspaces, invitations, and join requests,
// resumes at the stored step, walks the visible sequence forward (stamping
// progress, finishing for members) and back, and renders the current step
// inside the shell. Later slices mount their steps through the `steps`
// prop (S8: workspace, invite) without reworking this composition.
// Written from parity/specs/auth.md.

import { useNavigate } from "@tanstack/react-router";
import type { WorkspaceInvitationList, WorkspaceJoinRequestList } from "@pidash/api-client";
import { Spinner } from "@pidash/kit";
import * as React from "react";

import { platform } from "../../../core/platform/index.js";
import { useInstanceConfig, useMe, useProfile, useWorkspaces } from "../../../core/session/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useFinishOnboarding } from "../api.js";
import { useUpdateProfile } from "../profile.js";
import {
  advanceOnboardingStep,
  decideInitialOnboardingStep,
  displayNameForAccount,
  mergeOnboardingFlags,
  onboardingProgress,
  previousOnboardingStep,
  shouldShowInviteStep,
  visibleOnboardingSteps,
  type OnboardingStep,
} from "../onboarding.js";
import { useMyWorkspaceInvitations, useMyWorkspaceJoinRequests } from "../invitations.js";
import { CliInstallStep } from "./CliInstallStep.js";
import { OnboardingShell } from "./OnboardingShell.js";
import { ProfileSetupStep } from "./ProfileSetupStep.js";
import { RoleSetupStep } from "./RoleSetupStep.js";
import { UseCaseSetupStep } from "./UseCaseSetupStep.js";

export const ONBOARDING_STAMP_FAILED_TITLE = "Could not save your progress";

export interface OnboardingStepProps {
  /** The step finished (saved or skipped); the flow stamps and moves on. */
  onAdvance: (options?: { skipInvites?: boolean }) => void;
  invitations: WorkspaceInvitationList;
  joinRequests: WorkspaceJoinRequestList;
  /**
   * True when the instance config disables workspace creation (AUTH-041).
   * Only the workspace step reads it; every step receives it.
   */
  creationDisabled?: boolean | undefined;
}

export type OnboardingStepComponent = React.ComponentType<OnboardingStepProps>;

const DEFAULT_STEPS: Record<"cli-install" | "profile" | "role" | "use-case", OnboardingStepComponent> = {
  "cli-install": CliInstallStep,
  profile: ProfileSetupStep,
  role: RoleSetupStep,
  "use-case": UseCaseSetupStep,
};

export interface OnboardingFlowProps {
  /** Steps later slices mount inside the shell (S8: workspace, invite). */
  steps?: Partial<Record<OnboardingStep, OnboardingStepComponent>>;
}

export function OnboardingFlow({ steps: extensions = {} }: OnboardingFlowProps): React.ReactElement {
  const navigate = useNavigate();
  const me = useMe();
  const profile = useProfile();
  const workspaces = useWorkspaces();
  const instanceConfig = useInstanceConfig();
  const invitations = useMyWorkspaceInvitations();
  const joinRequests = useMyWorkspaceJoinRequests();
  const updateProfile = useUpdateProfile();
  const finishOnboarding = useFinishOnboarding();

  // Desktop bundles the CLI, so browser builds show the CLI-install step
  // and desktop builds start at profile setup (target per main#494).
  const showCli = platform.kind === "web";
  const [current, setCurrent] = React.useState<OnboardingStep | null>(null);

  const loading =
    me.isLoading || profile.isLoading || workspaces.isLoading || instanceConfig.isLoading || invitations.isLoading;
  const flags = profile.data?.onboarding_step;

  React.useEffect(() => {
    if (!loading && current === null) {
      setCurrent(decideInitialOnboardingStep({ showCli, flags }));
    }
  }, [loading, current, showCli, flags]);

  if (loading || current === null) {
    return (
      <div className="grid min-h-screen place-items-center bg-(--bg)">
        <Spinner label="Loading onboarding" />
      </div>
    );
  }

  const selfManaged = instanceConfig.data?.is_self_managed === true;
  const workspaceList = workspaces.data ?? [];
  const invitationList = invitations.data ?? [];
  const requestList = joinRequests.data ?? [];
  const showInvite = shouldShowInviteStep({ hasInvitations: invitationList.length > 0, current });
  const visible = visibleOnboardingSteps({ showCli, selfManaged, showInvite });
  const progress = onboardingProgress({ steps: visible, current });
  const previous = previousOnboardingStep({ from: current, showCli, selfManaged });

  const advance = (from: OnboardingStep, options?: { skipInvites?: boolean }): void => {
    const plan = advanceOnboardingStep({
      from,
      selfManaged,
      hasWorkspaces: workspaceList.length > 0,
      skipInvites: options?.skipInvites,
    });
    const run = async (): Promise<void> => {
      if (plan.stamp !== null) {
        try {
          await updateProfile.mutateAsync({ onboarding_step: mergeOnboardingFlags(flags, plan.stamp) });
        } catch {
          // The step still moves: finishing re-stamps every flag, and a
          // missed stamp only costs a resume position.
          notifyError(ONBOARDING_STAMP_FAILED_TITLE);
        }
      }
      if (plan.to === "finish") {
        try {
          const landing = await finishOnboarding.mutateAsync();
          await navigate({ href: landing });
        } catch {
          // The finish hook toasted already; the user stays on the step to retry.
        }
        return;
      }
      setCurrent(plan.to);
    };
    void run();
  };

  const registry: Partial<Record<OnboardingStep, OnboardingStepComponent>> = { ...DEFAULT_STEPS, ...extensions };
  const StepView = registry[current];

  return (
    <OnboardingShell
      step={current}
      progress={progress}
      canGoBack={previous !== null}
      onBack={() => setCurrent(previous ?? current)}
      accountName={displayNameForAccount({
        display_name: me.data?.display_name ?? "",
        first_name: me.data?.first_name ?? "",
        last_name: me.data?.last_name ?? "",
        email: me.data?.email ?? "",
      })}
      accountAvatarUrl={me.data?.avatar_url ?? null}
      accountEmail={me.data?.email ?? ""}
    >
      {StepView ? (
        <StepView
          onAdvance={(options) => advance(current, options)}
          invitations={invitationList}
          joinRequests={requestList}
          creationDisabled={instanceConfig.data?.is_workspace_creation_disabled === true}
        />
      ) : null}
    </OnboardingShell>
  );
}
