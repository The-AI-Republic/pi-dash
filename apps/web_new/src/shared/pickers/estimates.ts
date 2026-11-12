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
import { orderEstimatePoints, selectActiveEstimate, type Estimate } from "@pidash/api-client";

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

/**
 * The value label a stored point reads as, looked up across the project's
 * systems (EST-022). The active system is searched first so its family
 * decides how the value renders (time as hours and minutes); a point that
 * belongs to a superseded system still names itself, because the issue
 * keeps pointing at it after the project switches systems. Undefined means
 * no system lists the id, so callers keep their own placeholder rule.
 */
export function selectEstimateValueLabel(
  systems: readonly Estimate[] | undefined,
  pointId: string | null | undefined
): string | undefined {
  if (systems === undefined || pointId === null || pointId === undefined || pointId === "") return undefined;
  const active = selectActiveEstimate([...systems]);
  const ordered = active === undefined ? [...systems] : [active, ...systems.filter((row) => row.id !== active.id)];
  for (const system of ordered) {
    const point = system.points.find((row) => row.id === pointId);
    if (point !== undefined) return formatEstimatePoint(point.value, system.type);
  }
  return undefined;
}

/**
 * The label a read-only spot shows for a stored point (EST-022): its value
 * when a system names it, the raw stored id otherwise (the spreadsheet's
 * long-standing fallback), and nothing when no point is set so the caller
 * renders its empty placeholder.
 */
export function readOnlyEstimateLabel(
  systems: readonly Estimate[] | undefined,
  pointId: string | null | undefined
): string | null {
  if (pointId === null || pointId === undefined || pointId === "") return null;
  return selectEstimateValueLabel(systems, pointId) ?? pointId;
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
