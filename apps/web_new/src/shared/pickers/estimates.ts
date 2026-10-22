// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimate option shaping (parity/specs/issues.md §17, ISS-216). The
// active system's points with a leading No-estimate clear row; point
// values render through the palette's formatter so TIME systems read
// as hours and minutes in both surfaces. Points arrive in the model's
// text order, so the list sorts by key before rendering (EST-017).
// Without an active system the list holds a single non-interactive
// empty row. The list stays thin: estimate system configuration lives
// in the estimates area.
import { orderEstimatePoints, type Estimate } from "@pidash/api-client";

import { formatEstimatePoint } from "../shell/contextual/workItems.js";

/** Clear-row id, shared with the palette's estimate page. */
export const NO_ESTIMATE_ID = "none";

/** One estimate row in display order. */
export interface EstimateOption {
  id: string;
  label: string;
  /** True for the leading No-estimate row. */
  clear: boolean;
  /** True only for the empty row without an active system. */
  disabled: boolean;
}

export function selectEstimateOptions(system: Estimate | undefined): EstimateOption[] {
  if (system === undefined) return [{ id: NO_ESTIMATE_ID, label: "No estimate", clear: true, disabled: true }];
  return [
    { id: NO_ESTIMATE_ID, label: "No estimate", clear: true, disabled: false },
    ...orderEstimatePoints(system.points).map((point) => ({
      id: point.id,
      label: formatEstimatePoint(point.value, system.type),
      clear: false,
      disabled: false,
    })),
  ];
}
