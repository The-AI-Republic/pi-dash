// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public entry for the issues bulk-selection slice (parity/specs/issues.md
// §10, ISS-108–115). Layouts import the hook and controls; the edition
// seam chooses the upgrade banner or the functional toolbar.
export { armedBulkProperties, BulkToolbar } from "./BulkToolbar.js";
export { BulkUpgradeBanner, BULK_UPGRADE_HREF } from "./BulkUpgradeBanner.js";
export { ForeignRowSelectionHint, GroupSelectionCheckbox, RowSelectionCheckbox } from "./SelectionCheckbox.js";
export { BULK_OPERATIONS_FLAG, isBulkOperationsEnabled } from "./edition.js";
export { useBulkMutations } from "./useBulkMutations.js";
export { SELECTION_ROW_ATTR, useBulkSelection } from "./useBulkSelection.js";
export type { BulkSelection } from "./useBulkSelection.js";
export { groupSelectionState, moveActive, visibleBulkFields } from "./model.js";
export type { BulkPropertyKey, GroupSelectionState, SelectionItem } from "./model.js";
