// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job list pure helpers (ADM-051/052/053, from
// parity/specs/admin.md §7.1–7.3). The jobs table, the empty paragraph,
// and the toggle failure notice read from here; the create/edit modal
// slice (NEWFRONT-516) reuses the role labels for its minimum-role
// options. Strings marked oracle-pinned are asserted verbatim by
// e2e/parity/admin/loop.spec.ts and must stay exact.

/**
 * Minimum-role labels as the jobs table renders them (oracle-pinned:
 * loop.spec.ts ADM-051 maps 20/15/5 to Admin/Member/Guest).
 */
export const LOOP_MIN_ROLE_LABELS: Readonly<Record<number, string>> = {
  20: "Admin",
  15: "Member",
  5: "Guest",
};

/**
 * Numeric role through the Admin/Member/Guest labels, raw value as
 * fallback (spec §7.1).
 */
export function resolveLoopMinRoleLabel(minRole: number): string {
  return LOOP_MIN_ROLE_LABELS[minRole] ?? String(minRole);
}

/** Empty-list paragraph (oracle-pinned: loop.spec.ts ADM-053). */
export const LOOP_LIST_EMPTY_TEXT = "No jobs yet. Create one to get started.";

/** Toggle failure notice title (oracle-pinned: loop.spec.ts ADM-052). */
export const LOOP_TOGGLE_FAILURE_TITLE = "Update failed";

/**
 * Toggle failure notice message (oracle-pinned: loop.spec.ts ADM-052).
 * Fixed copy, not the server message: the scenario fails the write
 * with a probe error and still expects this text.
 */
export const LOOP_TOGGLE_FAILURE_MESSAGE = "Could not change the job state.";
