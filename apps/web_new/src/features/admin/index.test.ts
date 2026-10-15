// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The god-mode route reaches admin code only through this barrel, but its
// suite mocks the barrel out (branch selection belongs to the admin area).
// This suite owns the barrel itself: every re-export resolves, so a missing
// export fails here instead of at runtime.
import { describe, expect, it } from "vitest";

import {
  acceptCompanyName,
  acceptPersonName,
  AdminAccountMenu,
  AdminBrandLoader,
  AdminDashboardGuard,
  AdminDashboardLayout,
  AdminEntryGate,
  adminCsrfQueryOptions,
  AdminHeader,
  AdminHelpSection,
  AdminHomeGuard,
  AdminNotFoundScreen,
  AdminSetupForm,
  AdminSidebar,
  AdminSignInForm,
  AdminSignOutForm,
  adminKeys,
  adminSessionQueryOptions,
  applyAdminThemePreference,
  applyStoredAdminTheme,
  COMPANY_NAME_MAX_LENGTH,
  CloudAdminEntry,
  decideAdminGuard,
  FIRST_WORKSPACE_CREATE_PATH,
  FirstWorkspacePopup,
  GOD_MODE_ENTRY,
  GOD_MODE_GENERAL,
  ADMIN_NARROW_VIEWPORT_PX,
  ADMIN_NOT_FOUND_ART,
  ADMIN_NOT_FOUND_TITLE,
  ADMIN_SEGMENT_LABELS,
  ADMIN_SIDEBAR_COLLAPSED_KEY,
  ADMIN_SIDEBAR_ENTRIES,
  ADMIN_THEME_KEY,
  buildAdminCrumbs,
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  instanceBootstrapQueryOptions,
  isCompanyNameText,
  isMismatchVisible,
  isPersonNameText,
  isSetupSubmittable,
  isSidebarEntryActive,
  isStrengthMeterVisible,
  PERSON_NAME_MAX_LENGTH,
  readStoredAdminTheme,
  registerAdminStoreReset,
  reloadEntryPage,
  resetAdminClientState,
  resetAdminStores,
  resolveSetupError,
  resolveSetupPrefill,
  resolveSignInBanners,
  SETUP_POST_PATH,
  SETUP_TITLE,
  SIGN_IN_POST_PATH,
  SIGN_IN_TITLE,
  SIGN_OUT_POST_PATH,
  useAdminCsrfToken,
  useAdminSession,
  useAdminSidebarStore,
  useAdminThemeStore,
  useFirstWorkspaceStore,
  useInstanceBootstrap,
} from "./index.js";

describe("admin barrel", () => {
  it("re-exports every member the route and later slices consume", () => {
    for (const fn of [
      acceptCompanyName,
      acceptPersonName,
      adminCsrfQueryOptions,
      adminSessionQueryOptions,
      applyAdminThemePreference,
      applyStoredAdminTheme,
      buildAdminCrumbs,
      decideAdminGuard,
      instanceBootstrapQueryOptions,
      isCompanyNameText,
      isMismatchVisible,
      isPersonNameText,
      isSetupSubmittable,
      isSidebarEntryActive,
      isStrengthMeterVisible,
      readStoredAdminTheme,
      registerAdminStoreReset,
      reloadEntryPage,
      resetAdminClientState,
      resetAdminStores,
      resolveSetupError,
      resolveSetupPrefill,
      resolveSignInBanners,
    ]) {
      expect(typeof fn).toBe("function");
    }
    for (const hook of [
      useAdminCsrfToken,
      useAdminSession,
      useAdminSidebarStore,
      useAdminThemeStore,
      useFirstWorkspaceStore,
      useInstanceBootstrap,
    ]) {
      expect(typeof hook).toBe("function");
    }
    for (const component of [
      AdminAccountMenu,
      AdminBrandLoader,
      AdminDashboardGuard,
      AdminDashboardLayout,
      AdminEntryGate,
      AdminHeader,
      AdminHelpSection,
      AdminHomeGuard,
      AdminNotFoundScreen,
      AdminSetupForm,
      AdminSidebar,
      AdminSignInForm,
      AdminSignOutForm,
      CloudAdminEntry,
      FirstWorkspacePopup,
      InstanceFailureScreen,
    ]) {
      expect(component).toBeDefined();
    }
    expect(adminKeys).toBeDefined();
    expect(ADMIN_NARROW_VIEWPORT_PX).toBeGreaterThan(0);
    expect(ADMIN_SEGMENT_LABELS).toBeDefined();
    expect(ADMIN_SIDEBAR_ENTRIES.length).toBeGreaterThan(0);
    expect(COMPANY_NAME_MAX_LENGTH).toBeGreaterThan(0);
    expect(PERSON_NAME_MAX_LENGTH).toBeGreaterThan(0);
    for (const text of [
      ADMIN_NOT_FOUND_ART,
      ADMIN_NOT_FOUND_TITLE,
      ADMIN_SIDEBAR_COLLAPSED_KEY,
      ADMIN_THEME_KEY,
      FIRST_WORKSPACE_CREATE_PATH,
      GOD_MODE_ENTRY,
      GOD_MODE_GENERAL,
      INSTANCE_FAILURE_ART,
      INSTANCE_FAILURE_TITLE,
      SETUP_POST_PATH,
      SETUP_TITLE,
      SIGN_IN_POST_PATH,
      SIGN_IN_TITLE,
      SIGN_OUT_POST_PATH,
    ]) {
      expect(text.length).toBeGreaterThan(0);
    }
  });
});
