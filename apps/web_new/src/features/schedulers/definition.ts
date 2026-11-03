// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure definition-form rules from parity/specs/agents.md §1 (AGT-002/005).

export const DEFAULT_SCHEDULER_COLOR = "#5e6ad2";

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
