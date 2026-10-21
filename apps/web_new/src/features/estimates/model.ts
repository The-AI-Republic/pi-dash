// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates settings model (parity/specs/project-settings-part-f.md
// §Estimates entry, PSET-049; parity/inventory/estimates.md EST-001–016).
// Pure selectors and value rules: which system is active, which are
// superseded, how list rows read, and the starter sets the creation
// dialog offers. No React, router or network here.

import type { Estimate, EstimateList } from "@pidash/api-client";

import { formatEstimatePoint } from "../../shared/shell/contextual/workItems.js";

/** Measurement families the OSS dialog offers (EST-009: time is absent). */
export const ESTIMATE_FAMILY_POINTS = "points";
export const ESTIMATE_FAMILY_CATEGORIES = "categories";

export type EstimateFamily = typeof ESTIMATE_FAMILY_POINTS | typeof ESTIMATE_FAMILY_CATEGORIES;

export const ESTIMATE_FAMILIES: readonly { value: EstimateFamily; label: string }[] = [
  { value: ESTIMATE_FAMILY_POINTS, label: "Points" },
  { value: ESTIMATE_FAMILY_CATEGORIES, label: "Categories" },
];

/** One starter card on creation step one (EST-010). */
export interface EstimateStarter {
  id: string;
  title: string;
  /** Plain preview of the values the card loads. */
  values: string[];
}

const STARTERS: Record<EstimateFamily, readonly EstimateStarter[]> = {
  points: [
    {
      id: "fibonacci",
      title: "Fibonacci",
      values: ["1", "2", "3", "5", "8", "13"],
    },
    {
      id: "linear",
      title: "Linear",
      values: ["1", "2", "3", "4", "5", "6"],
    },
    {
      id: "squares",
      title: "Squares",
      values: ["1", "4", "9", "16", "25", "36"],
    },
  ],
  categories: [
    {
      id: "tshirt",
      title: "T-Shirt Sizes",
      values: ["XS", "S", "M", "L", "XL", "XXL"],
    },
  ],
};

/** The scratch card opens step two with the family's minimal values. */
export const CUSTOM_STARTER_ID = "custom";

const CUSTOM_VALUES: Record<EstimateFamily, readonly string[]> = {
  points: ["1", "2"],
  categories: ["Easy", "Hard"],
};

/** All starter cards for a family, scratch first (EST-010). */
export function estimateStarters(family: EstimateFamily): EstimateStarter[] {
  return [
    {
      id: CUSTOM_STARTER_ID,
      title: "Custom",
      values: [...CUSTOM_VALUES[family]],
    },
    ...STARTERS[family],
  ];
}

/** Values one starter card loads, or undefined for an unknown id. */
export function starterValues(family: EstimateFamily, starterId: string): string[] | undefined {
  return estimateStarters(family).find((starter) => starter.id === starterId)?.values;
}

/** Name the created system stores: the family's display label. */
export function familySystemName(family: EstimateFamily): string {
  return family === ESTIMATE_FAMILY_POINTS ? "Points" : "Categories";
}

/** The project's active system: the one marked in use (EST-005). */
export function selectActiveSystem(systems: EstimateList): Estimate | undefined {
  return systems.find((system) => system.last_used);
}

/**
 * Superseded systems for the read-only section (EST-006): everything not
 * active, newest first, matching the old list's creation ordering.
 */
export function selectSupersededSystems(systems: EstimateList): Estimate[] {
  return systems
    .filter((system) => !system.last_used)
    .slice()
    .sort((left, right) => right.created_at.localeCompare(left.created_at));
}

/** Values of one system, humanized for the time family (EST-005). */
export function systemValueLabels(system: Estimate): string[] {
  return system.points
    .slice()
    .sort((left, right) => left.key - right.key)
    .map((point) => formatEstimatePoint(point.value, system.type));
}

/** Ahuman per-value rule set for the dialog (EST-012). */
export interface ValueProblem {
  index: number;
  message: string;
}

export const VALUE_REQUIRED = "Enter a value.";
export const VALUE_DUPLICATE = "Each value must be different.";
export const VALUE_NOT_NUMBER = "Use a number greater than zero.";
export const VALUE_NOT_CATEGORY = "Categories cannot be numbers.";
export const VALUE_TOO_LONG = "Use at most 40 characters.";

export const ESTIMATE_VALUE_MAX_LENGTH = 40;

/**
 * Problems across a value set (empty, over-long, duplicates, and the
 * family rule: numeric families take positive numbers, categories take
 * non-numeric text). Order follows the row index.
 */
export function valueSetProblems(family: EstimateFamily, values: readonly string[]): ValueProblem[] {
  const problems: ValueProblem[] = [];
  const seen = new Map<string, number>();
  values.forEach((raw, index) => {
    const value = raw.trim();
    if (value === "") {
      problems.push({ index, message: VALUE_REQUIRED });
      return;
    }
    if (value.length > ESTIMATE_VALUE_MAX_LENGTH) {
      problems.push({ index, message: VALUE_TOO_LONG });
      return;
    }
    const key = value.toLowerCase();
    if (seen.has(key)) {
      problems.push({ index, message: VALUE_DUPLICATE });
      return;
    }
    seen.set(key, index);
    if (family === ESTIMATE_FAMILY_POINTS) {
      const numeric = Number(value);
      if (!Number.isFinite(numeric) || numeric <= 0) problems.push({ index, message: VALUE_NOT_NUMBER });
    } else if (Number.isFinite(Number(value)) && value.trim() !== "") {
      problems.push({ index, message: VALUE_NOT_CATEGORY });
    }
  });
  return problems;
}

/** Position keys renumbered 1..n in display order (decision D21). */
export function renumberValues(values: readonly string[]): { key: number; value: string }[] {
  return values.map((value, index) => ({ key: index + 1, value }));
}
