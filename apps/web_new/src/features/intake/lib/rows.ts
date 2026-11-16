// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake row shaping for the list (parity/specs/intake.md INT-006,
// INT-007): the project-scoped id label, and cursor-page assembly that
// never repeats a request already on screen.

import type { IntakeIssueRow } from "@pidash/api-client";

/** The project-scoped id label: the project prefix plus sequence number. */
export function intakeRowIdLabel(projectIdentifier: string, sequenceId: number): string {
  return `${projectIdentifier}-${sequenceId}`;
}

/**
 * Pages flattened in server order with repeats removed: a cursor the
 * server replays keeps the request's first position instead of
 * rendering twice (INT-007). Repeat detection keys on the work item,
 * which is the row's selection identity.
 */
export function uniqueIntakeRows(rows: readonly IntakeIssueRow[]): IntakeIssueRow[] {
  const seen = new Set<string>();
  const unique: IntakeIssueRow[] = [];
  for (const row of rows) {
    if (seen.has(row.issue.id)) continue;
    seen.add(row.issue.id);
    unique.push(row);
  }
  return unique;
}
