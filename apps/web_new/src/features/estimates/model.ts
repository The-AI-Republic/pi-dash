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

/** Edition capability that makes the time family available (EST-009). */
export const ESTIMATE_TIME_FAMILY_FLAG = "estimate-time-family";

/** Measurement families the creation dialog can offer. */
export const ESTIMATE_FAMILY_POINTS = "points";
export const ESTIMATE_FAMILY_CATEGORIES = "categories";
export const ESTIMATE_FAMILY_TIME = "time";

export type EstimateFamily =
  | typeof ESTIMATE_FAMILY_POINTS
  | typeof ESTIMATE_FAMILY_CATEGORIES
  | typeof ESTIMATE_FAMILY_TIME;

/** One family in the catalogue; a required flag gates it to that edition. */
export interface EstimateFamilyOption {
  value: EstimateFamily;
  label: string;
  /** Edition capability that must be on before the family is offered. */
  requiredFlag?: string;
}

/**
 * The complete family catalogue. The dialog filters this by edition flags:
 * points and categories are always offered; time needs the cloud capability
 * so an OSS build never renders its radio at all (EST-009).
 */
export const ESTIMATE_FAMILY_CATALOGUE: readonly EstimateFamilyOption[] = [
  { value: ESTIMATE_FAMILY_POINTS, label: "Points" },
  { value: ESTIMATE_FAMILY_CATEGORIES, label: "Categories" },
  { value: ESTIMATE_FAMILY_TIME, label: "Time", requiredFlag: ESTIMATE_TIME_FAMILY_FLAG },
];

/** Families offered by the community edition (time is absent). */
export const ESTIMATE_FAMILIES: readonly EstimateFamilyOption[] = ESTIMATE_FAMILY_CATALOGUE.filter(
  (family) => family.requiredFlag === undefined
);

/** True when the edition flags allow this family. */
export function estimateFamilyEnabled(family: EstimateFamily, flags: Readonly<Record<string, boolean>> = {}): boolean {
  const option = ESTIMATE_FAMILY_CATALOGUE.find((entry) => entry.value === family);
  return option !== undefined && (option.requiredFlag === undefined || flags[option.requiredFlag] === true);
}

/** The family options this edition offers, in catalogue order. */
export function estimateFamilies(flags: Readonly<Record<string, boolean>> = {}): readonly EstimateFamilyOption[] {
  return ESTIMATE_FAMILY_CATALOGUE.filter((family) => estimateFamilyEnabled(family.value, flags));
}

/** One starter card on creation step one (EST-010). */
export interface EstimateStarter {
  id: string;
  title: string;
  /** Stored values the card loads. */
  values: string[];
}

const STARTERS: Record<EstimateFamily, readonly EstimateStarter[]> = {
  points: [
    { id: "fibonacci", title: "Fibonacci", values: ["1", "2", "3", "5", "8", "13"] },
    { id: "linear", title: "Linear", values: ["1", "2", "3", "4", "5", "6"] },
    { id: "squares", title: "Squares", values: ["1", "4", "9", "16", "25", "36"] },
  ],
  categories: [
    { id: "tshirt", title: "T-Shirt Sizes", values: ["XS", "S", "M", "L", "XL", "XXL"] },
    { id: "easy-to-hard", title: "Easy to hard", values: ["Easy", "Medium", "Hard", "Very Hard"] },
  ],
  time: [{ id: "hours", title: "Hours", values: ["1", "2", "3", "4", "5", "6"] }],
};

/** The scratch card opens step two with the family's minimal values. */
export const CUSTOM_STARTER_ID = "custom";

const CUSTOM_VALUES: Record<EstimateFamily, readonly string[]> = {
  points: ["1", "2"],
  categories: ["Easy", "Hard"],
  time: ["1", "2"],
};

/** All starter cards for a family, scratch first (EST-010). */
export function estimateStarters(family: EstimateFamily): EstimateStarter[] {
  return [{ id: CUSTOM_STARTER_ID, title: "Custom", values: [...CUSTOM_VALUES[family]] }, ...STARTERS[family]];
}

/** Values one starter card loads, or undefined for an unknown id. */
export function starterValues(family: EstimateFamily, starterId: string): string[] | undefined {
  return estimateStarters(family).find((starter) => starter.id === starterId)?.values;
}

/** Human-readable value for a row or preview; time stores raw minutes. */
export function estimateValueLabel(family: EstimateFamily, value: string): string {
  return formatEstimatePoint(value, family);
}

/** Comma-separated preview text for a starter card. */
export function estimatePreview(family: EstimateFamily, values: readonly string[]): string {
  return values.map((value) => estimateValueLabel(family, value)).join(", ");
}

/** Name the created system stores: the family's display label. */
export function familySystemName(family: EstimateFamily): string {
  if (family === ESTIMATE_FAMILY_POINTS) return "Points";
  if (family === ESTIMATE_FAMILY_CATEGORIES) return "Categories";
  return "Time";
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

/** A per-value problem and the row it belongs to (EST-012). */
export interface ValueProblem {
  index: number;
  message: string;
}

export const VALUE_REQUIRED = "An estimate value cannot be empty.";
export const VALUE_DUPLICATE = "This value already exists in the set.";
export const VALUE_NOT_NUMBER = "Enter a numeric value.";
export const VALUE_NOT_POSITIVE = "Use a value greater than 0.";
export const VALUE_NOT_CATEGORY = "Categories need text characters, not a number.";
export const VALUE_TOO_LONG = "Use at most 20 characters.";

/** The server rejects longer text; the dialog caps input here first. */
export const ESTIMATE_VALUE_MAX_LENGTH = 20;

/** Keep an over-long edit to the prefix the user meant. */
export function truncateEstimateValue(value: string): string {
  return value.slice(0, ESTIMATE_VALUE_MAX_LENGTH);
}

function numericFamily(family: EstimateFamily): boolean {
  return family === ESTIMATE_FAMILY_POINTS || family === ESTIMATE_FAMILY_TIME;
}

/**
 * Problems across a value set. Values are trimmed once before judging:
 * empty, over-long, duplicate (numeric equality for points/time; exact
 * text for categories), and the family rule (numeric families take
 * positive numbers, categories take non-numeric text). Order follows the
 * row index.
 */
export function valueSetProblems(family: EstimateFamily, values: readonly string[]): ValueProblem[] {
  const problems: ValueProblem[] = [];
  const seen = new Map<string, number>();
  const reportedDuplicates = new Set<string>();
  const numeric = numericFamily(family);
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
    const parsed = Number(value);
    // Non-numeric text has no numeric identity, so each such row reports
    // its own family problem rather than colliding on "NaN".
    const duplicateKey = numeric ? (Number.isFinite(parsed) ? String(parsed) : undefined) : value;
    if (duplicateKey !== undefined && seen.has(duplicateKey)) {
      if (!reportedDuplicates.has(duplicateKey)) {
        reportedDuplicates.add(duplicateKey);
        problems.push({ index, message: VALUE_DUPLICATE });
      }
      return;
    }
    if (duplicateKey !== undefined) seen.set(duplicateKey, index);
    if (numeric) {
      if (!Number.isFinite(parsed)) problems.push({ index, message: VALUE_NOT_NUMBER });
      else if (parsed <= 0) problems.push({ index, message: VALUE_NOT_POSITIVE });
    } else if (!Number.isNaN(Number(value))) {
      problems.push({ index, message: VALUE_NOT_CATEGORY });
    }
  });
  return problems;
}

/** Position keys renumbered 1..n in display order (decision D21). */
export function renumberValues(values: readonly string[]): { key: number; value: string }[] {
  return values.map((value, index) => ({ key: index + 1, value }));
}

/**
 * Move the value at `sourceIndex` before the row currently at `targetIndex`
 * (EST-011). Out-of-range or same-row requests return an unchanged copy.
 */
export function moveValueBefore(values: readonly string[], sourceIndex: number, targetIndex: number): string[] {
  if (
    sourceIndex === targetIndex ||
    sourceIndex < 0 ||
    targetIndex < 0 ||
    sourceIndex >= values.length ||
    targetIndex >= values.length
  ) {
    return [...values];
  }
  const next = [...values];
  const moved = next.splice(sourceIndex, 1)[0] as string;
  const insertAt = sourceIndex < targetIndex ? targetIndex - 1 : targetIndex;
  next.splice(insertAt, 0, moved);
  return next;
}
