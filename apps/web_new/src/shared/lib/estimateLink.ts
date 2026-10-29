// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one rule behind every estimate control (parity/specs/estimates.md
// §6.2, EST-018): a project offers estimates exactly while it links a
// system. Only the project's own row carries that link — the workspace
// project list answers a fixed column set without it — so callers hand
// this the row they read from the project endpoint. A missing row reads
// off, which keeps estimate controls out of the first paint instead of
// flashing them before the row answers.

/** The single field this rule reads; both absent and null mean off. */
export interface EstimateLinkRow {
  estimate?: string | null | undefined;
}

/** True when the project links an estimate system, so controls show. */
export function hasEstimateLink(project: EstimateLinkRow | null | undefined): boolean {
  return project !== null && project !== undefined && project.estimate !== null && project.estimate !== undefined;
}
