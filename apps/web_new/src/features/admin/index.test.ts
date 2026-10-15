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
  AdminBrandLoader,
  AdminEntryGate,
  adminCsrfQueryOptions,
  AdminSetupForm,
  AdminSignInForm,
  adminKeys,
  COMPANY_NAME_MAX_LENGTH,
  GOD_MODE_ENTRY,
  INSTANCE_FAILURE_ART,
  INSTANCE_FAILURE_TITLE,
  InstanceFailureScreen,
  instanceBootstrapQueryOptions,
  isCompanyNameText,
  isMismatchVisible,
  isPersonNameText,
  isSetupSubmittable,
  isStrengthMeterVisible,
  PERSON_NAME_MAX_LENGTH,
  reloadEntryPage,
  resolveSetupError,
  resolveSetupPrefill,
  resolveSignInBanners,
  SETUP_POST_PATH,
  SETUP_TITLE,
  SIGN_IN_POST_PATH,
  SIGN_IN_TITLE,
  useAdminCsrfToken,
  useInstanceBootstrap,
} from "./index.js";

describe("admin barrel", () => {
  it("re-exports every member the route and later slices consume", () => {
    for (const fn of [
      acceptCompanyName,
      acceptPersonName,
      adminCsrfQueryOptions,
      instanceBootstrapQueryOptions,
      isCompanyNameText,
      isMismatchVisible,
      isPersonNameText,
      isSetupSubmittable,
      isStrengthMeterVisible,
      reloadEntryPage,
      resolveSetupError,
      resolveSetupPrefill,
      resolveSignInBanners,
    ]) {
      expect(typeof fn).toBe("function");
    }
    for (const hook of [useAdminCsrfToken, useInstanceBootstrap]) {
      expect(typeof hook).toBe("function");
    }
    for (const component of [
      AdminBrandLoader,
      AdminEntryGate,
      AdminSetupForm,
      AdminSignInForm,
      InstanceFailureScreen,
    ]) {
      expect(component).toBeDefined();
    }
    expect(adminKeys).toBeDefined();
    expect(COMPANY_NAME_MAX_LENGTH).toBeGreaterThan(0);
    expect(PERSON_NAME_MAX_LENGTH).toBeGreaterThan(0);
    for (const text of [
      GOD_MODE_ENTRY,
      INSTANCE_FAILURE_ART,
      INSTANCE_FAILURE_TITLE,
      SETUP_POST_PATH,
      SETUP_TITLE,
      SIGN_IN_POST_PATH,
      SIGN_IN_TITLE,
    ]) {
      expect(text.length).toBeGreaterThan(0);
    }
  });
});
