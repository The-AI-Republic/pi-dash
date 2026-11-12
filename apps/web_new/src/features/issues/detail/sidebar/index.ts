// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar property rows (ISS-148–159). Feature code imports the
// sidebar through the issues barrel; these exports exist for the barrel
// and for focused tests.

export { CreatedByRow } from "./CreatedByRow.js";
export type { CreatedByRowProps } from "./CreatedByRow.js";
export { PRODUCT_CREATOR_NAME, UNKNOWN_CREATOR_NAME, isIntakeBot, selectCreator } from "./creator.js";
export type { CreatorDisplay } from "./creator.js";
export { IssuePropertiesSidebar, PROPERTIES_LOAD_FAILED } from "./IssuePropertiesSidebar.js";
export type { IssuePropertiesSidebarProps, SidebarProject } from "./IssuePropertiesSidebar.js";
export {
  PROPERTY_SAVE_FAILED_TITLE,
  SidebarProperties,
  SidebarPropertiesSkeleton,
  selectExecutionSelection,
} from "./SidebarProperties.js";
export type { SidebarPropertiesProps } from "./SidebarProperties.js";
export {
  CycleRow,
  DueDateRow,
  EstimateRow,
  LabelsRow,
  ModulesRow,
  ParentRow,
  PROPERTY_SAVE_FAILED_B_TITLE,
  SidebarPropertiesB,
  StartDateRow,
  parentIdentifier,
} from "./SidebarPropertiesB.js";
export type {
  CycleRowProps,
  DueDateRowProps,
  EstimateRowProps,
  LabelsRowProps,
  ModulesRowProps,
  ParentRowProps,
  SidebarPropertiesBProps,
  StartDateRowProps,
} from "./SidebarPropertiesB.js";
export { PARENT_SEARCH_DEBOUNCE_MS, ParentPickerDialog, parentSearchQueryOptions } from "./ParentPickerDialog.js";
export type { ParentPickerDialogProps } from "./ParentPickerDialog.js";
export { dateBounds, isDueDateOverdue, showsCycleRow, showsEstimateRow, showsModuleRow } from "./sidebarState.js";
export type { SidebarFeatureProject } from "./sidebarState.js";
export { SIDEBAR_ROW_CLASS, SIDEBAR_ROW_LABEL_CLASS, SidebarRow } from "./SidebarRow.js";
export type { SidebarRowProps } from "./SidebarRow.js";
