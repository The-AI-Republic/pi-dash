// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin console (god-mode) feature barrel. Other features and
// routes reach admin code only through here.

export { GOD_MODE_ENTRY, resolveSetupError, resolveSignInBanners } from "./adminErrors.js";
export type { SetupError, SetupErrorPlacement, SignInBanners, SignInMappedBanner } from "./adminErrors.js";
export { decideAdminGuard, GOD_MODE_GENERAL } from "./adminGuards.js";
export type { AdminGuardDecision, AdminGuardTree } from "./adminGuards.js";
export { useAdminCsrfToken, useAdminSession, useInstanceBootstrap } from "./hooks.js";
export {
  adminCsrfQueryOptions,
  adminKeys,
  adminSessionQueryOptions,
  instanceBootstrapQueryOptions,
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
export { registerAdminStoreReset, resetAdminClientState, resetAdminStores, useFirstWorkspaceStore } from "./stores.js";
export type { AdminStoreReset } from "./stores.js";
export {
  ADMIN_THEME_KEY,
  applyAdminThemePreference,
  applyStoredAdminTheme,
  readStoredAdminTheme,
  useAdminThemeStore,
} from "./theme.js";
export type { AdminThemeMode } from "./theme.js";
export { AdminBrandLoader } from "./components/AdminBrandLoader.js";
export { AdminEntryGate } from "./components/AdminEntryGate.js";
export type { AdminEntrySearch } from "./components/AdminEntryGate.js";
export { AdminDashboardGuard, AdminHomeGuard } from "./components/AdminGuards.js";
export { ADMIN_NOT_FOUND_ART, ADMIN_NOT_FOUND_TITLE, AdminNotFoundScreen } from "./components/AdminNotFoundScreen.js";
export { AdminSignOutForm, SIGN_OUT_POST_PATH } from "./components/AdminSignOutForm.js";
export { FIRST_WORKSPACE_CREATE_PATH, FirstWorkspacePopup } from "./components/FirstWorkspacePopup.js";
export {
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  reloadEntryPage,
} from "./components/InstanceFailureScreen.js";
export { AdminSetupForm, resolveSetupPrefill, SETUP_POST_PATH, SETUP_TITLE } from "./components/AdminSetupForm.js";
export type { SetupPrefill } from "./components/AdminSetupForm.js";
export { AdminSignInForm, SIGN_IN_POST_PATH, SIGN_IN_TITLE } from "./components/AdminSignInForm.js";
export { CloudAdminEntry } from "./components/CloudAdminEntry.js";
