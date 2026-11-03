// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog edition seam (parity/specs/issues.md §16 and §23, ISS-206).
// The community build renders no worklog surface at all — no button,
// rows, summary or upgrade call to action — and the activity filter
// keeps no worklog category. The cloud edition turns the flag on and
// mounts its own entry; the issues surface only asks this module.

import { edition } from "../../../../../core/edition/index.js";

/** Edition flag that enables the cloud worklog surfaces. */
export const WORKLOG_FLAG = "issue-worklog";

/** True when this build should mount worklog surfaces. */
export function isWorklogEnabled(flags: Record<string, boolean> = edition.flags): boolean {
  return flags[WORKLOG_FLAG] === true;
}
