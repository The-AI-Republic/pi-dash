/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// pi dash imports
import { PiDashLockup, ChevronLeftIcon } from "@pi-dash/propel/icons";
import { Tooltip } from "@pi-dash/propel/tooltip";
import type { TOnboardingStep } from "@pi-dash/types";
import { EOnboardingSteps } from "@pi-dash/types";
import { cn } from "@pi-dash/utils";
// hooks
import { useInstance } from "@/hooks/store/use-instance";
import { useUser } from "@/hooks/store/user";
// local imports
import { getOnboardingStepOrder, getPreviousOnboardingStep } from "./flow";
import { SwitchAccountDropdown } from "./switch-account-dropdown";

type OnboardingHeaderProps = {
  currentStep: EOnboardingSteps;
  updateCurrentStep: (step: EOnboardingSteps) => void;
  hasInvitations: boolean;
};

export const OnboardingHeader = observer(function OnboardingHeader(props: OnboardingHeaderProps) {
  const { currentStep, updateCurrentStep, hasInvitations } = props;
  // store hooks
  const { data: user } = useUser();
  const { config: instanceConfig } = useInstance();
  const isSelfManaged = instanceConfig?.is_self_managed;

  const previousStep = getPreviousOnboardingStep(currentStep, isSelfManaged);
  const handleStepBack = () => {
    if (previousStep) updateCurrentStep(previousStep);
  };

  // can go back
  const canGoBack = previousStep !== null;

  // step order for progress tracking — include INVITE_MEMBERS if user is currently on it
  const showInviteStep = !hasInvitations || currentStep === EOnboardingSteps.INVITE_MEMBERS;
  const stepOrder: TOnboardingStep[] = getOnboardingStepOrder(isSelfManaged, showInviteStep);

  // derived values
  const currentStepNumber = stepOrder.indexOf(currentStep) + 1;
  const totalSteps = stepOrder.length;
  const userName = user?.display_name
    ? user?.display_name
    : user?.first_name
      ? `${user?.first_name} ${user?.last_name ?? ""}`
      : user?.email;

  return (
    <div className="sticky top-0 z-10 flex flex-col gap-4">
      <div className="h-1.5 w-full cursor-pointer overflow-hidden rounded-t-lg bg-surface-1">
        <Tooltip tooltipContent={`${currentStepNumber}/${totalSteps}`} position="bottom-end">
          <div
            className="h-full bg-accent-primary transition-all duration-700 ease-out"
            style={{ width: `${(currentStepNumber / totalSteps) * 100}%` }}
          />
        </Tooltip>
      </div>
      <div className={cn("flex w-full items-center justify-between gap-6 px-6", canGoBack && "pr-6 pl-4")}>
        <div className="flex items-center gap-2.5">
          {canGoBack && (
            <button onClick={handleStepBack} className="cursor-pointer" type="button" disabled={!canGoBack}>
              <ChevronLeftIcon className="size-6 text-placeholder" />
            </button>
          )}
          <PiDashLockup height={20} width={37} className="text-primary" />
        </div>
        <SwitchAccountDropdown fullName={userName} />
      </div>
    </div>
  );
});
