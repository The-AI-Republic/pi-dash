// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { GOD_MODE_ENTRY, resolveSetupError, resolveSignInBanners } from "./adminErrors.js";

describe("resolveSetupError", () => {
  it("stays silent without a code", () => {
    expect(resolveSetupError(undefined, undefined)).toEqual({ placement: "silent", text: null });
    expect(resolveSetupError(undefined, "parity-probe")).toEqual({ placement: "silent", text: null });
  });

  it("renders the named email/password codes inline with the server message", () => {
    expect(resolveSetupError("INVALID_EMAIL", "parity-probe-email")).toEqual({
      placement: "inline-email",
      text: "parity-probe-email",
    });
    expect(resolveSetupError("INVALID_PASSWORD", "parity-probe-password")).toEqual({
      placement: "inline-password",
      text: "parity-probe-password",
    });
  });

  it("renders the named banner codes as a top banner with the server message", () => {
    for (const code of [
      "INSTANCE_NOT_CONFIGURED",
      "ADMIN_ALREADY_EXIST",
      "REQUIRED_EMAIL_PASSWORD_FIRST_NAME",
      "USER_ALREADY_EXISTS",
    ]) {
      expect(resolveSetupError(code, "parity-probe-banner")).toEqual({
        placement: "banner",
        text: "parity-probe-banner",
      });
    }
  });

  it("places the numeric codes the server sends like their named counterparts (NEWFRONT-457)", () => {
    expect(resolveSetupError("5160", "5160-message")).toEqual({ placement: "inline-email", text: "5160-message" });
    expect(resolveSetupError("5021", "5021-message")).toEqual({ placement: "inline-password", text: "5021-message" });
    expect(resolveSetupError("5165", "5165-message")).toEqual({ placement: "inline-password", text: "5165-message" });
    for (const code of ["5000", "5150", "5155", "5170", "5175", "5180", "5185", "5190"]) {
      expect(resolveSetupError(code, `${code}-message`)).toEqual({ placement: "banner", text: `${code}-message` });
    }
  });

  it("falls back to readable copy when a known code carries no message", () => {
    expect(resolveSetupError("INVALID_EMAIL", undefined).text).not.toBeNull();
    expect(resolveSetupError("INVALID_EMAIL", "").text).not.toBeNull();
    expect(resolveSetupError("5021", undefined).text).not.toBeNull();
    expect(resolveSetupError("5180", undefined).text).not.toBeNull();
    expect(resolveSetupError("5180", "").text).not.toBeNull();
  });

  it("stays silent on unknown codes", () => {
    expect(resolveSetupError("NO_SUCH_CODE", "parity-probe-unknown")).toEqual({ placement: "silent", text: null });
  });
});

describe("resolveSignInBanners", () => {
  it("stays silent without a code", () => {
    expect(resolveSignInBanners(undefined, undefined)).toEqual({ generic: null, mapped: null });
    expect(resolveSignInBanners(undefined, "parity-probe")).toEqual({ generic: null, mapped: null });
  });

  it("renders the five named codes as the generic banner with the server message verbatim", () => {
    for (const code of [
      "INSTANCE_NOT_CONFIGURED",
      "REQUIRED_EMAIL_PASSWORD",
      "INVALID_EMAIL",
      "USER_DOES_NOT_EXIST",
      "AUTHENTICATION_FAILED",
    ]) {
      expect(resolveSignInBanners(code, "parity-probe-generic")).toEqual({
        generic: "parity-probe-generic",
        mapped: null,
      });
    }
  });

  it("requires a message for the generic banner", () => {
    expect(resolveSignInBanners("INVALID_EMAIL", undefined)).toEqual({ generic: null, mapped: null });
    expect(resolveSignInBanners("INVALID_EMAIL", "")).toEqual({ generic: null, mapped: null });
  });

  it("maps every numeric admin code to fresh copy", () => {
    for (const code of ["5150", "5155", "5160", "5165", "5170", "5175", "5180", "5185", "5190"]) {
      const banners = resolveSignInBanners(code, "server-message-ignored");
      expect(banners.generic).toBeNull();
      expect(banners.mapped).not.toBeNull();
      expect(banners.mapped?.title.length).toBeGreaterThan(0);
      expect(banners.mapped?.message.length).toBeGreaterThan(0);
    }
  });

  it("names the authentication failure so the round-trip outcome reads", () => {
    const banners = resolveSignInBanners("5175", "ADMIN_AUTHENTICATION_FAILED");
    expect(`${banners.mapped?.title ?? ""} ${banners.mapped?.message ?? ""}`).toContain("Authentication failed");
    expect(banners.mapped?.linkHref).toBeNull();
  });

  it("links the already-exists and unknown-account mappings back to the sign-in entry (NEWFRONT-458)", () => {
    expect(resolveSignInBanners("5180", "ADMIN_USER_ALREADY_EXIST").mapped?.linkHref).toBe(GOD_MODE_ENTRY);
    expect(resolveSignInBanners("5185", "ADMIN_USER_DOES_NOT_EXIST").mapped?.linkHref).toBe(GOD_MODE_ENTRY);
    expect(GOD_MODE_ENTRY).toBe("/god-mode/");
  });

  it("leaves the link off every other mapping", () => {
    for (const code of ["5150", "5155", "5160", "5165", "5170", "5175", "5190"]) {
      expect(resolveSignInBanners(code, "x").mapped?.linkHref).toBeNull();
    }
  });

  it("stays silent on unknown codes", () => {
    expect(resolveSignInBanners("NO_SUCH_CODE", "parity-probe-unknown")).toEqual({ generic: null, mapped: null });
    expect(resolveSignInBanners("5000", "INSTANCE_NOT_CONFIGURED")).toEqual({ generic: null, mapped: null });
    expect(resolveSignInBanners("5021", "PASSWORD_TOO_WEAK")).toEqual({ generic: null, mapped: null });
  });
});
