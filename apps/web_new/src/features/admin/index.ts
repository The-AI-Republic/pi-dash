// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin console (god-mode) feature barrel. Other features and
// routes reach admin code only through here.

export { GOD_MODE_ENTRY, resolveSetupError, resolveSignInBanners } from "./adminErrors.js";
export type { SetupError, SetupErrorPlacement, SignInBanners, SignInMappedBanner } from "./adminErrors.js";
export { useAdminCsrfToken, useInstanceBootstrap } from "./hooks.js";
export { adminCsrfQueryOptions, adminKeys, instanceBootstrapQueryOptions } from "./queries.js";
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
export { AdminBrandLoader } from "./components/AdminBrandLoader.js";
export { AdminEntryGate } from "./components/AdminEntryGate.js";
export type { AdminEntrySearch } from "./components/AdminEntryGate.js";
export {
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  reloadEntryPage,
} from "./components/InstanceFailureScreen.js";
export { AdminSetupForm, resolveSetupPrefill, SETUP_POST_PATH, SETUP_TITLE } from "./components/AdminSetupForm.js";
export type { SetupPrefill } from "./components/AdminSetupForm.js";
export { AdminSignInForm, SIGN_IN_POST_PATH, SIGN_IN_TITLE } from "./components/AdminSignInForm.js";
