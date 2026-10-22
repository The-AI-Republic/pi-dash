// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the project-settings feature. Other features and routes
// import only from here.

export {
  admitsStanding,
  decidePageGate,
  effectiveProjectRole,
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
export * from "./members/index.js";
export { EstimatesTab } from "./estimates/EstimatesTab.js";
export { RepositoryTab } from "./repository/RepositoryTab.js";
export {
  cadenceLine,
  providerLabel,
  REPOSITORY_BOUND_PREFIX,
  REPOSITORY_CADENCE,
  REPOSITORY_CARD_HEADING,
  REPOSITORY_CONTRACT,
  REPOSITORY_GUIDANCE_BODY,
  REPOSITORY_LAST_ERROR_PREFIX,
  REPOSITORY_SYNC_LABEL,
  REPOSITORY_UNBIND_LABEL,
  RepositorySyncCard,
} from "./repository/RepositorySyncCard.js";
export type { RepositorySyncCardProps } from "./repository/RepositorySyncCard.js";
export {
  repositoryKeys,
  repositoryQueryOptions,
  SYNC_FAILED_BODY,
  SYNC_FAILED_TITLE,
  SYNC_UPDATED_BODY,
  SYNC_UPDATED_TITLE,
  UNBOUND_BODY,
  UNBOUND_TITLE,
  useRepositoryStatus,
  useToggleRepositorySync,
  useUnbindRepository,
} from "./repository/api.js";
export { GeneralSkeleton, ProjectSettingsGeneralTab, SAVE_LABEL, SAVING_LABEL } from "./general/GeneralTabScreen.js";
export type { GeneralTabProps } from "./general/GeneralTabScreen.js";
export {
  LOST_RUNNER_RULE_SAVE_FAILED_MESSAGE,
  LOST_RUNNER_RULE_SAVE_FAILED_TITLE,
  LOST_RUNNER_RULE_SAVED_MESSAGE,
  LOST_RUNNER_RULE_SAVED_TITLE,
  useSaveLostRunnerRule,
} from "./ai-workers/api/mutations.js";
export { AiWorkersSkeleton, ProjectSettingsAiWorkersTab } from "./ai-workers/AiWorkersTabScreen.js";
export type { AiWorkersTabProps } from "./ai-workers/AiWorkersTabScreen.js";
export {
  DEFAULT_LOST_RUNNER_RULE,
  LOST_RUNNER_RULE_OPTIONS,
  LOST_RUNNER_RULE_ORDER,
  buildLostRunnerRulePayload,
  lostRunnerRuleDescription,
  lostRunnerRuleFrom,
  lostRunnerRuleLabel,
} from "./ai-workers/rules.js";
export type { LostRunnerRule, LostRunnerRuleOption } from "./ai-workers/rules.js";
