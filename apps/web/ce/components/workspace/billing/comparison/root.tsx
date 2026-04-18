/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash imports
import type { EProductSubscriptionEnum, TBillingFrequency } from "@apple-pi-dash/types";
// components
import { PlansComparisonBase, shouldRenderPlanDetail } from "@/components/workspace/billing/comparison/base";
import type { TApplePiDashPlans } from "@/constants/plans";
import { APPLE_PI_DASH_PLANS } from "@/constants/plans";
// apple pi dash web imports
import { PlanDetail } from "./plan-detail";

type TPlansComparisonProps = {
  isCompareAllFeaturesSectionOpen: boolean;
  getBillingFrequency: (subscriptionType: EProductSubscriptionEnum) => TBillingFrequency | undefined;
  setBillingFrequency: (subscriptionType: EProductSubscriptionEnum, frequency: TBillingFrequency) => void;
  setIsCompareAllFeaturesSectionOpen: React.Dispatch<React.SetStateAction<boolean>>;
};

export const PlansComparison = observer(function PlansComparison(props: TPlansComparisonProps) {
  const {
    isCompareAllFeaturesSectionOpen,
    getBillingFrequency,
    setBillingFrequency,
    setIsCompareAllFeaturesSectionOpen,
  } = props;
  // plan details
  const { planDetails } = APPLE_PI_DASH_PLANS;

  return (
    <PlansComparisonBase
      applePiDashDetails={Object.entries(planDetails).map(([planKey, plan]) => {
        const currentPlanKey = planKey as TApplePiDashPlans;
        if (!shouldRenderPlanDetail(currentPlanKey)) return null;
        return (
          <PlanDetail
            key={planKey}
            subscriptionType={plan.id}
            planDetail={plan}
            billingFrequency={getBillingFrequency(plan.id)}
            setBillingFrequency={(frequency) => setBillingFrequency(plan.id, frequency)}
          />
        );
      })}
      isSelfManaged
      isCompareAllFeaturesSectionOpen={isCompareAllFeaturesSectionOpen}
      setIsCompareAllFeaturesSectionOpen={setIsCompareAllFeaturesSectionOpen}
    />
  );
});
