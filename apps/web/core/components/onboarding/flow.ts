/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TOnboardingStep } from "@pi-dash/types";
import { EOnboardingSteps } from "@pi-dash/types";
import { SHOW_CLI_INSTALL_STEP } from "@/pi-dash-web/components/desktop/onboarding-edition";

export function getInitialOnboardingStep(): TOnboardingStep {
  return SHOW_CLI_INSTALL_STEP ? EOnboardingSteps.CLI_INSTALL : EOnboardingSteps.PROFILE_SETUP;
}

export function getPreviousOnboardingStep(
  currentStep: EOnboardingSteps,
  isSelfManaged?: boolean
): TOnboardingStep | null {
  switch (currentStep) {
    case EOnboardingSteps.PROFILE_SETUP:
      return SHOW_CLI_INSTALL_STEP ? EOnboardingSteps.CLI_INSTALL : null;
    case EOnboardingSteps.ROLE_SETUP:
      return EOnboardingSteps.PROFILE_SETUP;
    case EOnboardingSteps.USE_CASE_SETUP:
      return EOnboardingSteps.ROLE_SETUP;
    case EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN:
      return isSelfManaged ? EOnboardingSteps.PROFILE_SETUP : EOnboardingSteps.USE_CASE_SETUP;
    default:
      return null;
  }
}

export function getOnboardingStepOrder(isSelfManaged: boolean | undefined, showInviteStep: boolean) {
  return [
    ...(SHOW_CLI_INSTALL_STEP ? [EOnboardingSteps.CLI_INSTALL] : []),
    EOnboardingSteps.PROFILE_SETUP,
    ...(isSelfManaged ? [] : [EOnboardingSteps.ROLE_SETUP, EOnboardingSteps.USE_CASE_SETUP]),
    EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN,
    ...(showInviteStep ? [EOnboardingSteps.INVITE_MEMBERS] : []),
  ] satisfies TOnboardingStep[];
}
