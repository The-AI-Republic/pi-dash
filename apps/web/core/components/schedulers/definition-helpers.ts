/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pure helpers for scheduler definition forms. Kept free of React so they
 * can be unit-tested without a DOM.
 */

/** `Scheduler.slug` is a 64-char column. */
const SCHEDULER_SLUG_MAX_LENGTH = 64;

/**
 * Derive a scheduler slug from a display name: lowercase, runs of anything
 * outside [a-z0-9] collapse to a single dash, leading/trailing dashes
 * stripped, capped at 64 chars. Returns "" when the name has no letter or
 * digit.
 *
 * The server derives the stored slug with the same rule
 * (`apps/api/pi_dash/scheduler/slug.py`); both are checked against the shared
 * vectors in `apps/api/pi_dash/scheduler/slug_vectors.json`. The forms use it
 * only to catch a name the server would reject.
 */
export function deriveSchedulerSlug(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, SCHEDULER_SLUG_MAX_LENGTH)
    .replace(/-+$/, "");
}
