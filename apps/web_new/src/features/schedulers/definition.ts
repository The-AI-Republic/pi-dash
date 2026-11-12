// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure definition-form rules from parity/specs/agents.md §1 (AGT-002/005).

/**
 * The preset swatches offered by the scheduler color field, in display
 * order. The values match the old app's palette so schedulers stored
 * earlier keep rendering the same color.
 */
export const SCHEDULER_COLOR_SWATCHES: readonly [string, ...string[]] = [
  "#3b82f6",
  "#6366f1",
  "#8b5cf6",
  "#a855f7",
  "#d946ef",
  "#ec4899",
  "#ef4444",
  "#f97316",
  "#eab308",
  "#84cc16",
  "#22c55e",
  "#10b981",
  "#14b8a6",
  "#06b6d4",
  "#0ea5e9",
  "#f59e0b",
];

/** The color a new scheduler starts with: the first preset swatch. */
export const DEFAULT_SCHEDULER_COLOR: string = SCHEDULER_COLOR_SWATCHES[0];

/** True when the stored value matches one of the preset swatches. */
export function isSchedulerSwatchColor(value: string): boolean {
  const folded = value.toLowerCase();
  return SCHEDULER_COLOR_SWATCHES.some((swatch) => swatch.toLowerCase() === folded);
}

export interface SchedulerDefinitionValue {
  name: string;
  slug: string;
  description: string;
  prompt: string;
  color: string;
  isEnabled: boolean;
}

export type SchedulerDefinitionErrors = Partial<Record<keyof SchedulerDefinitionValue, string>>;

export function emptySchedulerDefinition(): SchedulerDefinitionValue {
  return {
    name: "",
    slug: "",
    description: "",
    prompt: "",
    color: DEFAULT_SCHEDULER_COLOR,
    isEnabled: true,
  };
}

export function deriveSchedulerHandle(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function validateSchedulerDefinition(value: SchedulerDefinitionValue): SchedulerDefinitionErrors {
  const errors: SchedulerDefinitionErrors = {};
  if (value.name.trim() === "") errors.name = "Enter a scheduler name.";
  if (value.slug.trim() === "") {
    errors.slug = "Enter a handle.";
  } else if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value.slug)) {
    errors.slug = "Use lowercase letters, numbers, and single dashes.";
  }
  if (value.prompt.trim() === "") errors.prompt = "Enter the base prompt.";
  if (!/^#[0-9a-fA-F]{6}$/.test(value.color)) errors.color = "Choose a six-digit hex color.";
  return errors;
}

export function hasSchedulerDefinitionErrors(errors: SchedulerDefinitionErrors): boolean {
  return Object.keys(errors).length > 0;
}
