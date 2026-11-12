/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { EProductSubscriptionEnum } from "@pi-dash/types";

/**
 * Calculates the yearly discount percentage when switching from monthly to yearly billing
 * @param monthlyPrice - The monthly subscription price
 * @param yearlyPricePerMonth - The monthly equivalent price when billed yearly
 * @returns The discount percentage as a whole number (floored)
 */
export const calculateYearlyDiscount = (monthlyPrice: number, yearlyPricePerMonth: number): number => {
  const monthlyCost = monthlyPrice * 12;
  const yearlyCost = yearlyPricePerMonth * 12;
  const amountSaved = monthlyCost - yearlyCost;
  const discountPercentage = (amountSaved / monthlyCost) * 100;
  return Math.floor(discountPercentage);
};

/**
 * Gets the display name for a subscription plan variant
 * @param planVariant - The subscription plan variant enum
 * @returns The human-readable name of the plan
 */
export const getSubscriptionName = (planVariant: EProductSubscriptionEnum): string => {
  switch (planVariant) {
    case EProductSubscriptionEnum.FREE:
      return "Free";
    case EProductSubscriptionEnum.ONE:
      return "One";
    case EProductSubscriptionEnum.PRO:
      return "Pro";
    case EProductSubscriptionEnum.BUSINESS:
      return "Business";
    case EProductSubscriptionEnum.ENTERPRISE:
      return "Enterprise";
    default:
      return "--";
  }
};
