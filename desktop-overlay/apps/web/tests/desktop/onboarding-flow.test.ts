/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import { EOnboardingSteps } from "@pi-dash/types";
import {
  getInitialOnboardingStep,
  getOnboardingStepOrder,
  getPreviousOnboardingStep,
} from "../../core/components/onboarding/flow";

describe("desktop onboarding flow", () => {
  it("starts at profile setup without a hidden CLI-install step", () => {
    expect(getInitialOnboardingStep()).toBe(EOnboardingSteps.PROFILE_SETUP);
    expect(getOnboardingStepOrder(false, true)).toEqual([
      EOnboardingSteps.PROFILE_SETUP,
      EOnboardingSteps.ROLE_SETUP,
      EOnboardingSteps.USE_CASE_SETUP,
      EOnboardingSteps.WORKSPACE_CREATE_OR_JOIN,
      EOnboardingSteps.INVITE_MEMBERS,
    ]);
    expect(getPreviousOnboardingStep(EOnboardingSteps.PROFILE_SETUP, false)).toBeNull();
    expect(getPreviousOnboardingStep(EOnboardingSteps.ROLE_SETUP, false)).toBe(EOnboardingSteps.PROFILE_SETUP);
  });
});
