/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pure helpers for scheduler definition forms. Kept free of React so they
 * can be unit-tested without a DOM.
 */

/**
 * Derive a URL-safe scheduler slug from a display name: lowercase, runs of
 * anything outside [a-z0-9] collapse to a single dash, leading/trailing
 * dashes stripped. Returns "" when nothing usable remains so callers can
 * fall back to requiring manual input.
 */
export function deriveSchedulerSlug(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The scheduler template fields an edit form can change. */
export type SchedulerTemplateValues = {
  name: string;
  description: string;
  prompt: string;
  color: string;
};

/**
 * Fields of `current` that differ from the `saved` template, as a PATCH
 * payload. Empty when nothing changed, so callers can skip the request.
 * Colors compare case-insensitively (the API lowercases them).
 */
export function diffSchedulerTemplate(
  current: SchedulerTemplateValues,
  saved: SchedulerTemplateValues
): Partial<SchedulerTemplateValues> {
  const changes: Partial<SchedulerTemplateValues> = {};
  if (current.name !== saved.name) changes.name = current.name;
  if (current.description !== (saved.description ?? "")) changes.description = current.description;
  if (current.prompt !== saved.prompt) changes.prompt = current.prompt;
  if (current.color.toLowerCase() !== (saved.color ?? "").toLowerCase()) changes.color = current.color;
  return changes;
}
