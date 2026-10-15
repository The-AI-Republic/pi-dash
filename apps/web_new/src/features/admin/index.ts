// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin console (god-mode) feature barrel. Other features and
// routes reach admin code only through here.

export { GOD_MODE_ENTRY, resolveSetupError, resolveSignInBanners } from "./adminErrors.js";
export type { SetupError, SetupErrorPlacement, SignInBanners, SignInMappedBanner } from "./adminErrors.js";
export { decideAdminGuard, GOD_MODE_GENERAL } from "./adminGuards.js";
export type { AdminGuardDecision, AdminGuardTree } from "./adminGuards.js";
export { auditAdminAbsence, auditAdminToggles } from "./adminInvariants.js";
export type { AdminAbsenceAudit, AdminToggleAudit } from "./adminInvariants.js";
export {
  ADMIN_NARROW_VIEWPORT_PX,
  ADMIN_SEGMENT_LABELS,
  ADMIN_SIDEBAR_COLLAPSED_KEY,
  ADMIN_SIDEBAR_ENTRIES,
  buildAdminCrumbs,
  isSidebarEntryActive,
} from "./adminShell.js";
export type { AdminCrumb, AdminSidebarEntryDef } from "./adminShell.js";
export {
  AUTH_METHOD_ENABLE_KEYS,
  AUTH_METHODS,
  canDisableAuthMethod,
  ENABLE_SIGNUP_KEY,
  isConfigFlagOn,
  isMethodConfigured,
  toConfigMap,
} from "./authMethods.js";
export type { AuthMethodDef, AuthMethodKey, ConfigMap } from "./authMethods.js";
export {
  CONFIG_SAVE_FAILED_LOG,
  toggleLoopJobMutationOptions,
  useAdminCsrfToken,
  useAdminSession,
  useInstanceAdmins,
  useInstanceBootstrap,
  useInstanceConfigurations,
  useLoopJobs,
  useToggleLoopJob,
  useUpdateGeneralSettings,
  useUpdateInstanceConfigurations,
} from "./hooks.js";
export type { LoopJobToggle } from "./hooks.js";
export {
  firstAdminEmail,
  GENERAL_NAME_REQUIRED_MESSAGE,
  GENERAL_SAVE_BUSY_LABEL,
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_IDLE_LABEL,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
  GENERAL_TELEMETRY_DOC_URL,
  isGeneralNameValid,
  mergeGeneralSettingsIntoBootstrap,
  resolveTelemetryPrefill,
} from "./generalSettings.js";
export { LOOP_MIN_ROLE_LABELS, resolveLoopMinRoleLabel } from "./loopJobs.js";
export {
  adminCsrfQueryOptions,
  adminKeys,
  adminQueryDefaults,
  adminSessionQueryOptions,
  instanceAdminsQueryOptions,
  instanceBootstrapQueryOptions,
  instanceConfigurationsQueryOptions,
  loopJobsQueryOptions,
  mergeConfigurationRows,
} from "./queries.js";
export {
  acceptCompanyName,
  acceptPersonName,
  COMPANY_NAME_MAX_LENGTH,
  isCompanyNameText,
  isMismatchVisible,
  isPersonNameText,
  isSetupSubmittable,
  isStrengthMeterVisible,
  PERSON_NAME_MAX_LENGTH,
} from "./setupValidation.js";
export type { MismatchInput, SetupGateInput } from "./setupValidation.js";
export {
  registerAdminStoreReset,
  resetAdminClientState,
  resetAdminStores,
  useAdminSidebarStore,
  useFirstWorkspaceStore,
} from "./stores.js";
export type { AdminStoreReset } from "./stores.js";
export {
  ADMIN_THEME_KEY,
  applyAdminThemePreference,
  applyStoredAdminTheme,
  readStoredAdminTheme,
  useAdminThemeStore,
} from "./theme.js";
export type { AdminThemeMode } from "./theme.js";
export { AdminAccountMenu } from "./components/AdminAccountMenu.js";
export { AdminAuthLanding, AUTH_LANDING_TITLE } from "./components/AdminAuthLanding.js";
export { AdminAuthMethodCard } from "./components/AdminAuthMethodCard.js";
export type { AdminAuthMethodCardProps } from "./components/AdminAuthMethodCard.js";
export { AdminBrandLoader } from "./components/AdminBrandLoader.js";
export { AdminDashboardLayout } from "./components/AdminDashboardLayout.js";
export { AdminEntryGate } from "./components/AdminEntryGate.js";
export { AdminLoopJobsPage } from "./components/AdminLoopJobsPage.js";
export type { AdminEntrySearch } from "./components/AdminEntryGate.js";
export { AdminDashboardGuard, AdminHomeGuard } from "./components/AdminGuards.js";
export { AdminHeader } from "./components/AdminHeader.js";
export { AdminHelpSection } from "./components/AdminHelpSection.js";
export { ADMIN_NOT_FOUND_ART, ADMIN_NOT_FOUND_TITLE, AdminNotFoundScreen } from "./components/AdminNotFoundScreen.js";
export { AdminSidebar } from "./components/AdminSidebar.js";
export { AdminSignOutForm, SIGN_OUT_POST_PATH } from "./components/AdminSignOutForm.js";
export { FIRST_WORKSPACE_CREATE_PATH, FirstWorkspacePopup } from "./components/FirstWorkspacePopup.js";
export {
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  reloadEntryPage,
} from "./components/InstanceFailureScreen.js";
export { AdminGeneralSettingsPage, GENERAL_SETTINGS_TITLE } from "./components/AdminGeneralSettingsPage.js";
export { AdminSetupForm, resolveSetupPrefill, SETUP_POST_PATH, SETUP_TITLE } from "./components/AdminSetupForm.js";
export type { SetupPrefill } from "./components/AdminSetupForm.js";
export { AdminSignInForm, SIGN_IN_POST_PATH, SIGN_IN_TITLE } from "./components/AdminSignInForm.js";
export { CloudAdminEntry } from "./components/CloudAdminEntry.js";
