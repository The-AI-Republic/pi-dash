// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail sidebar property rows A (ISS-148–152). Feature code imports
// the sidebar through the issues barrel; these exports exist for the
// barrel and for focused tests.

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
export { SIDEBAR_ROW_CLASS, SIDEBAR_ROW_LABEL_CLASS, SidebarRow } from "./SidebarRow.js";
export type { SidebarRowProps } from "./SidebarRow.js";
