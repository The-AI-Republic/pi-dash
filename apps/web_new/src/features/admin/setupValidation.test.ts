// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
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

const STRONG = "Parity-Setup-1!";

describe("person names", () => {
  it("caps input length at 50 characters", () => {
    expect(PERSON_NAME_MAX_LENGTH).toBe(50);
    expect(isPersonNameText("a".repeat(50))).toBe(true);
    expect(isPersonNameText("a".repeat(51))).toBe(false);
  });

  it("accepts letters, spaces, hyphens, and apostrophes", () => {
    for (const name of ["Parity", "Jean-Paul", "O'Brien", "Mary Jane", "José", "Müller"]) {
      expect(isPersonNameText(name)).toBe(true);
    }
  });

  it("rejects digits and symbols", () => {
    for (const name of ["Ann1", "Acme_Corp", "Bo{ss}", "a/b", "x@y"]) {
      expect(isPersonNameText(name)).toBe(false);
    }
    expect(isPersonNameText("")).toBe(false);
  });

  it("accepts empty for clearing but swallows rejected whole values", () => {
    expect(acceptPersonName("")).toBe(true);
    expect(acceptPersonName("Jean-Paul")).toBe(true);
    expect(acceptPersonName("Ann1")).toBe(false);
    expect(acceptPersonName("a".repeat(60))).toBe(false);
  });

  it("rejects whitespace-only names like the old required check", () => {
    expect(isPersonNameText(" ")).toBe(false);
    expect(isPersonNameText("   ")).toBe(false);
    expect(acceptPersonName(" ")).toBe(false);
  });
});

describe("company names", () => {
  it("caps input length at 80 characters", () => {
    expect(COMPANY_NAME_MAX_LENGTH).toBe(80);
    expect(isCompanyNameText("b".repeat(80))).toBe(true);
    expect(isCompanyNameText("b".repeat(81))).toBe(false);
  });

  it("accepts letters, numbers, spaces, hyphens, and underscores", () => {
    for (const company of ["Prefill Co", "Acme_123 Corp-x", "42", "R&D".replace("&", "")]) {
      expect(isCompanyNameText(company)).toBe(true);
    }
  });

  it("rejects braces and other symbols", () => {
    for (const company of ["Acme{Corp}", "a/b", "x@y", "O'Brien"]) {
      expect(isCompanyNameText(company)).toBe(false);
    }
    expect(isCompanyNameText("")).toBe(false);
  });

  it("accepts empty for clearing but swallows rejected whole values", () => {
    expect(acceptCompanyName("")).toBe(true);
    expect(acceptCompanyName("Acme_123 Corp-x")).toBe(true);
    expect(acceptCompanyName("Acme{Corp}")).toBe(false);
    expect(acceptCompanyName("b".repeat(90))).toBe(false);
  });

  it("keeps the old optional-field behavior for whitespace-only input", () => {
    expect(isCompanyNameText(" ")).toBe(true);
    expect(acceptCompanyName(" ")).toBe(true);
  });
});

describe("isSetupSubmittable", () => {
  const open = {
    firstName: "Parity",
    email: "a@example.com",
    password: STRONG,
    confirmPassword: STRONG,
    csrfReady: true,
  };

  it("opens only with a name, an email, a strong secret, and a match", () => {
    expect(isSetupSubmittable(open)).toBe(true);
  });

  it("stays shut while anything is missing", () => {
    expect(isSetupSubmittable({ ...open, firstName: "" })).toBe(false);
    expect(isSetupSubmittable({ ...open, email: "" })).toBe(false);
    expect(isSetupSubmittable({ ...open, password: "" })).toBe(false);
    expect(isSetupSubmittable({ ...open, confirmPassword: "" })).toBe(false);
    expect(isSetupSubmittable({ ...open, csrfReady: false })).toBe(false);
  });

  it("stays shut on a weak secret or a mismatch", () => {
    expect(isSetupSubmittable({ ...open, password: "short", confirmPassword: "short" })).toBe(false);
    expect(isSetupSubmittable({ ...open, confirmPassword: "Parity-Setup-2!" })).toBe(false);
  });
});

describe("isStrengthMeterVisible", () => {
  it("shows on an empty untouched field", () => {
    expect(isStrengthMeterVisible("", false)).toBe(true);
  });

  it("shows while focused whatever the value", () => {
    expect(isStrengthMeterVisible("", true)).toBe(true);
    expect(isStrengthMeterVisible("short", true)).toBe(true);
    expect(isStrengthMeterVisible(STRONG, true)).toBe(true);
  });

  it("hides only when blurred with a strong secret", () => {
    expect(isStrengthMeterVisible("short", false)).toBe(true);
    expect(isStrengthMeterVisible(STRONG, false)).toBe(false);
  });
});

describe("isMismatchVisible", () => {
  it("hides on empty or matching confirmations", () => {
    expect(isMismatchVisible({ password: STRONG, confirmPassword: "", confirmFocused: false })).toBe(false);
    expect(isMismatchVisible({ password: STRONG, confirmPassword: STRONG, confirmFocused: false })).toBe(false);
    expect(isMismatchVisible({ password: STRONG, confirmPassword: STRONG, confirmFocused: true })).toBe(false);
  });

  it("hides while a trailing confirmation still has focus", () => {
    expect(isMismatchVisible({ password: STRONG, confirmPassword: "Parity-Se", confirmFocused: true })).toBe(false);
  });

  it("shows once the field is left or catches up in length", () => {
    expect(isMismatchVisible({ password: STRONG, confirmPassword: "Parity-Se", confirmFocused: false })).toBe(true);
    expect(isMismatchVisible({ password: "abcdef", confirmPassword: "abcdefg", confirmFocused: true })).toBe(true);
  });
});
