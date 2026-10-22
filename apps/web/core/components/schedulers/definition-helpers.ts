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
