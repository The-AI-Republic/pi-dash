// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk-operations edition seam (parity/specs/issues.md §10 and §23).
// The community build renders the upgrade banner and no functional
// toolbar. A cloud build sets the flag and mounts the toolbar that drives
// the four bulk endpoints. The issues surface never branches on an
// edition id, only on this named capability.

import { edition } from "../../../core/edition/index.js";

/** Edition flag that enables the functional cloud bulk toolbar. */
export const BULK_OPERATIONS_FLAG = "issue-bulk-operations";

/** True when this build offers the functional bulk toolbar. */
export function isBulkOperationsEnabled(flags: Record<string, boolean> = edition.flags): boolean {
  return flags[BULK_OPERATIONS_FLAG] === true;
}
