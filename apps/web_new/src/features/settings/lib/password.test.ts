// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  canSubmitPassword,
  classifyPasswordFieldError,
  describePasswordFailure,
  isStrongPassword,
  mismatchVisible,
  PASSWORD_MESSAGES,
  passwordErrors,
  passwordRules,
  sameAsOldVisible,
  strengthVisible,
  type PasswordFormState,
} from "./password.js";

function state(overrides: Partial<PasswordFormState> = {}): PasswordFormState {
  return {
    requiresCurrent: true,
    focused: null,
    confirmTouched: false,
    values: { current: "Old-Pass-1", next: "Fresh-Pass-9", confirm: "Fresh-Pass-9" },
    ...overrides,
  };
}

describe("password rules", () => {
  it("lists each rule's state", () => {
    const rules = passwordRules("Ab1!");
    expect(rules.map((rule) => [rule.id, rule.met])).toEqual([
      ["length", false],
      ["upper", true],
      ["lower", true],
      ["digit", true],
      ["symbol", true],
    ]);
    expect(passwordRules("abcdefgh").map((rule) => rule.met)).toEqual([true, false, true, false, false]);
    expect(passwordRules("A1!a1!a1!").every((rule) => rule.met)).toBe(true);
  });

  it("requires every rule for strength and shows the meter only for weak non-empty guesses", () => {
    expect(isStrongPassword("Parity-Strong-1A")).toBe(true);
    expect(isStrongPassword("weak")).toBe(false);
    expect(strengthVisible("")).toBe(false);
    expect(strengthVisible("weak")).toBe(true);
    expect(strengthVisible("Parity-Strong-1A")).toBe(false);
    expect(passwordRules("Parity-Strong-1A").find((rule) => rule.id === "length")?.label).toBe("Min 8 characters");
  });

  it("times the mismatch note on blur or a long-enough guess", () => {
    const shorter = state({
      values: { current: "Old-Pass-1", next: "Parity-Strong-1A", confirm: "x" },
      focused: "confirm",
    });
    expect(mismatchVisible(shorter)).toBe(false);
    expect(mismatchVisible({ ...shorter, confirmTouched: true })).toBe(true);
    expect(
      mismatchVisible({
        ...shorter,
        values: { ...shorter.values, confirm: "Parity-Strong-1A-zzz" },
      })
    ).toBe(true);
    expect(mismatchVisible({ ...shorter, values: { ...shorter.values, confirm: "Parity-Strong-1A" } })).toBe(false);
    expect(mismatchVisible({ ...shorter, values: { ...shorter.values, confirm: "" } })).toBe(false);
  });

  it("shows the same-as-old note only once the new field loses focus", () => {
    const same = state({ values: { current: "Old-Pass-1", next: "Old-Pass-1", confirm: "Old-Pass-1" } });
    expect(sameAsOldVisible(same)).toBe(true);
    expect(sameAsOldVisible({ ...same, focused: "next" })).toBe(false);
    expect(sameAsOldVisible({ ...same, values: { current: "", next: "Old-Pass-1", confirm: "" } })).toBe(false);
    expect(sameAsOldVisible({ ...same, values: { current: "Old-Pass-1", next: "", confirm: "" } })).toBe(false);
  });

  it("reports required fields", () => {
    expect(passwordErrors(state({ values: { current: " ", next: "", confirm: "" } }))).toEqual({
      current: PASSWORD_MESSAGES.currentRequired,
      new: PASSWORD_MESSAGES.newRequired,
    });
    expect(passwordErrors(state({ requiresCurrent: false, values: { current: "", next: "", confirm: "" } }))).toEqual({
      new: PASSWORD_MESSAGES.newRequired,
    });
    expect(passwordErrors(state())).toEqual({});
  });

  it("gates submit on presence, strength, match and difference", () => {
    expect(canSubmitPassword(state())).toBe(true);
    expect(
      canSubmitPassword(state({ requiresCurrent: false, values: { current: "", next: "weak", confirm: "weak" } }))
    ).toBe(false);
    expect(canSubmitPassword(state({ values: { current: "Old-Pass-1", next: "weak", confirm: "weak" } }))).toBe(false);
    expect(canSubmitPassword(state({ values: { current: "Old-Pass-1", next: "Fresh-Pass-9", confirm: "x" } }))).toBe(
      false
    );
    expect(
      canSubmitPassword(state({ values: { current: "Fresh-Pass-9", next: "Fresh-Pass-9", confirm: "Fresh-Pass-9" } }))
    ).toBe(false);
    expect(
      canSubmitPassword(
        state({
          requiresCurrent: false,
          values: { current: "Anything", next: "Fresh-Pass-9", confirm: "Fresh-Pass-9" },
        })
      )
    ).toBe(true);
  });
});

describe("password failure mapping", () => {
  it("maps the server's password codes", () => {
    expect(describePasswordFailure({ code: "5135" })).toEqual({
      title: "Incorrect old password",
      message: "Incorrect old password. Please try again.",
      field: null,
    });
    expect(describePasswordFailure({ code: 5135 })).toMatchObject({ title: "Incorrect old password" });
    expect(describePasswordFailure({ code: "5021", message: "PASSWORD_TOO_WEAK" })).toEqual({
      title: "Password too weak",
      message: PASSWORD_MESSAGES.weakFromServer,
      field: "new",
    });
    expect(describePasswordFailure({ code: "5138" })).toMatchObject({ field: "current" });
  });

  it("falls back to the server message, then a generic line", () => {
    expect(describePasswordFailure({ message: "Server said no" })).toMatchObject({
      title: "Could not change password",
      message: "Server said no",
      field: null,
    });
    expect(describePasswordFailure({ message: "" })).toMatchObject({
      message: "Something went wrong. Please try again.",
    });
    expect(describePasswordFailure(undefined)).toMatchObject({ field: null });
    expect(describePasswordFailure({ code: "9999" })).toMatchObject({ title: "Could not change password" });
  });

  it("classifies which field a refusal belongs to", () => {
    expect(classifyPasswordFieldError({ code: "5021" })).toBe("new");
    expect(classifyPasswordFieldError({ code: "5138" })).toBe("current");
    expect(classifyPasswordFieldError({ code: "5135" })).toBeNull();
    expect(classifyPasswordFieldError(undefined)).toBeNull();
    expect(classifyPasswordFieldError({ code: null })).toBeNull();
  });
});
