// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition flags for the issues area's cloud-only capabilities
// (parity/specs/issues.md §20 and §23). Every surface below asks this
// module, never an edition id. The community build leaves every flag
// absent, so each surface renders nothing at all — the oracle pins that
// absence in edition-stubs.spec.ts.

import { edition, ISSUE_DEDUPE_FLAG } from "../../../core/edition/index.js";

/** Cloud epic create/edit dialog (ISS-232). */
export const EPICS_FLAG = "issue-epics";
/** Cloud workflow transitions and their disabled states (ISS-233). */
export const WORKFLOW_FLAG = "issue-workflow";
/** Team-project scoping filter on the sub-issue dropdown (ISS-234). */
export const TEAM_PROJECT_FILTER_FLAG = "issue-team-project-filter";
/** Work-item-type filter plus its applied chips (ISS-234). */
export const ISSUE_TYPE_FILTER_FLAG = "issue-type-filter";
/** Work-item types, per-type properties and their activity rows (ISS-235). */
export const ISSUE_TYPES_FLAG = "issue-types";
/** Overdue and transfer-hop accents in the detail sidebar (ISS-236). */
export const DETAIL_BADGES_FLAG = "issue-detail-badges";
/** Dependency arrows and drag-to-link handles on the timeline (ISS-237). */
export const GANTT_DEPENDENCIES_FLAG = "issue-gantt-dependencies";

/** Re-exported so the detail's de-dupe seam names its own capability (ISS-231). */
export { ISSUE_DEDUPE_FLAG };

/** True when this build turns the named capability on. */
export function isEditionFlagOn(flag: string, flags: Record<string, boolean> = edition.flags): boolean {
  return flags[flag] === true;
}

/** De-dupe in the detail view (ISS-231); the modal shares the same flag. */
export function isIssueDedupeEnabled(flags: Record<string, boolean> = edition.flags): boolean {
  return isEditionFlagOn(ISSUE_DEDUPE_FLAG, flags);
}
