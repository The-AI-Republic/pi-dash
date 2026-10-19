// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the project-settings feature. Other features and routes
// import only from here.

export {
  admitsStanding,
  decidePageGate,
  findTabByKey,
  findTabBySuffix,
  findVisibleTab,
  isAdminStanding,
  isMemberStanding,
  isTabActive,
  PROJECT_SETTINGS_GROUP_LABELS,
  PROJECT_SETTINGS_GROUPS,
  PROJECT_SETTINGS_TABS,
  projectSettingsRoot,
  projectSettingsTabHref,
  resolveSettingsAddress,
  selectSettingsNav,
} from "./tabs.js";
export type {
  ProjectRole,
  ProjectSettingsGroup,
  ProjectSettingsNavGroup,
  ProjectSettingsPageGate,
  ProjectSettingsStanding,
  ProjectSettingsTabDef,
  ProjectSettingsTabKey,
} from "./tabs.js";
export {
  currentSectionLabel,
  ProjectSettingsContext,
  ProjectSettingsShell,
  ProjectSettingsTabGate,
  useProjectSettings,
} from "./components/ProjectSettingsLayout.js";
export type {
  ProjectSettingsContextValue,
  ProjectSettingsProjectFacts,
  ProjectSettingsShellProps,
  ProjectSettingsTabGateProps,
} from "./components/ProjectSettingsLayout.js";
export { ProjectSettingsPage } from "./components/ProjectSettingsPage.js";
export type { ProjectSettingsPageProps } from "./components/ProjectSettingsPage.js";
export { ProjectSettingsTabMount } from "./components/ProjectSettingsTabMount.js";
export type { ProjectSettingsTabMountProps } from "./components/ProjectSettingsTabMount.js";
export {
  ProjectSettingsNotAuthorized,
  ProjectSettingsRestriction,
  RESTRICTION_JOIN_BODY,
  RESTRICTION_JOIN_TITLE,
  RESTRICTION_NO_ACCESS_BODY,
  RESTRICTION_NO_ACCESS_TITLE,
  RESTRICTION_NOT_FOUND_BODY,
  RESTRICTION_NOT_FOUND_TITLE,
} from "./components/ProjectSettingsRestriction.js";
export type {
  ProjectSettingsRestrictionKind,
  ProjectSettingsRestrictionProps,
} from "./components/ProjectSettingsRestriction.js";
export { ProjectSettingsSidebar, ROLE_LABELS, roleLabel } from "./components/ProjectSettingsSidebar.js";
export type { ProjectSettingsSidebarProps } from "./components/ProjectSettingsSidebar.js";
export { ProjectSettingsGuidance } from "./components/ProjectSettingsGuidance.js";
export { ProjectSettingsScreen } from "./components/ProjectSettingsScreen.js";
export type { ProjectSettingsScreenProps } from "./components/ProjectSettingsScreen.js";
