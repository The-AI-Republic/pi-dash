/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { ArrowDown, ArrowUp } from "lucide-react";
// apple pi dash imports
import { Button } from "@apple-pi-dash/propel/button";
import { cn } from "@apple-pi-dash/utils";
// constants
import type { TApplePiDashPlans } from "@/constants/plans";
import { ComingSoonBadge, APPLE_PI_DASH_PLANS, PLANS_LIST } from "@/constants/plans";
// local imports
import { PlanFeatureDetail } from "./feature-detail";

type TPlansComparisonBaseProps = {
  applePiDashDetails: React.ReactNode;
  isSelfManaged: boolean;
  isCompareAllFeaturesSectionOpen: boolean;
  setIsCompareAllFeaturesSectionOpen: React.Dispatch<React.SetStateAction<boolean>>;
};

export const shouldRenderPlanDetail = (planKey: TApplePiDashPlans) => {
  // Free plan is not required to be shown in the comparison
  if (planKey === "free") return false;
  // Apple Pi Dash one plan is not longer available
  if (planKey === "one") return false;
  return true;
};

export const PlansComparisonBase = observer(function PlansComparisonBase(props: TPlansComparisonBaseProps) {
  const { applePiDashDetails, isSelfManaged, isCompareAllFeaturesSectionOpen, setIsCompareAllFeaturesSectionOpen } = props;
  // plan details
  const { planDetails, planHighlights, planComparison } = APPLE_PI_DASH_PLANS;
  const numberOfPlansToRender = Object.keys(planDetails).filter((planKey) =>
    shouldRenderPlanDetail(planKey as TApplePiDashPlans)
  ).length;

  const getSubscriptionType = (planKey: TApplePiDashPlans) => planDetails[planKey].id;

  return (
    <div className="horizontal-scrollbar scrollbar-sm size-full overflow-x-auto">
      <div className="max-w-full" style={{ minWidth: `${numberOfPlansToRender * 280}px` }}>
        <div className="flex h-full flex-col gap-y-10">
          <div
            className={cn("sticky top-2 z-10 grid flex-shrink-0 gap-3 bg-layer-1 text-caption-md-medium")}
            style={{
              gridTemplateColumns: `repeat(${numberOfPlansToRender + 1}, minmax(0, 1fr))`,
            }}
          >
            <div className="col-span-1 space-y-0.5 p-3 text-body-sm-medium" />
            {applePiDashDetails}
          </div>
          {/* Plan Headers */}
          <section className="flex-shrink-0">
            {/* Plan Highlights */}
            <div
              className="text-caption-md grid gap-3 rounded-xs py-1 text-secondary even:bg-surface-2"
              style={{ gridTemplateColumns: `repeat(${numberOfPlansToRender + 1}, minmax(0, 1fr))` }}
            >
              <div className="col-span-1 p-3 text-body-sm-medium">Highlights</div>
              {Object.entries(planHighlights).map(
                ([planKey, highlights]) =>
                  shouldRenderPlanDetail(planKey as TApplePiDashPlans) && (
                    <div key={planKey} className="col-span-1 p-3">
                      <ul className="list-disc space-y-1 text-body-xs-regular">
                        {highlights.map((highlight, index) => (
                          <li key={index}>{highlight}</li>
                        ))}
                      </ul>
                    </div>
                  )
              )}
            </div>
          </section>

          {/* Feature Comparison */}
          {isCompareAllFeaturesSectionOpen && (
            <>
              {planComparison.map((section, sectionIdx) => (
                <section key={sectionIdx} className="flex-shrink-0">
                  <h2 className="mb-2 flex items-start gap-2 pl-2 text-h5-semibold text-secondary">
                    {section.title} {section.comingSoon && <ComingSoonBadge />}
                  </h2>
                  <div className="border-t border-subtle">
                    {section.features.map((feature, featureIdx) => (
                      <div
                        key={featureIdx}
                        className="text-caption-md grid gap-3 rounded-xs bg-layer-transparent text-secondary even:bg-layer-1"
                        style={{ gridTemplateColumns: `repeat(${numberOfPlansToRender + 1}, minmax(0, 1fr))` }}
                      >
                        <div className="col-span-1 flex items-center p-3 text-body-sm-medium">
                          <div className="flex w-full items-start justify-between gap-2">
                            {feature.title} {feature.comingSoon && <ComingSoonBadge />}
                          </div>
                        </div>
                        {PLANS_LIST.map(
                          (planKey) =>
                            shouldRenderPlanDetail(planKey) && (
                              <div
                                key={planKey}
                                className="col-span-1 flex items-center justify-center p-3 text-center text-body-xs-regular"
                              >
                                <PlanFeatureDetail
                                  subscriptionType={getSubscriptionType(planKey)}
                                  data={
                                    isSelfManaged
                                      ? (feature["self-hosted"]?.[planKey] ?? feature.cloud[planKey])
                                      : feature.cloud[planKey]
                                  }
                                />
                              </div>
                            )
                        )}
                      </div>
                    ))}
                  </div>
                </section>
              ))}
            </>
          )}
        </div>

        {/* Toggle Button */}
        <div className="my-4 flex items-center justify-center gap-1 pb-2">
          <Button
            variant="ghost"
            onClick={() => {
              setIsCompareAllFeaturesSectionOpen(!isCompareAllFeaturesSectionOpen);
            }}
            appendIcon={isCompareAllFeaturesSectionOpen ? <ArrowUp /> : <ArrowDown />}
          >
            {isCompareAllFeaturesSectionOpen ? "Collapse comparison" : "Compare all features"}
          </Button>
        </div>
      </div>
    </div>
  );
});
