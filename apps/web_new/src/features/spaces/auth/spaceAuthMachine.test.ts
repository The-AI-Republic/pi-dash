// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { SpaceAuthCapabilities, SpaceAuthMachineState } from "./spaceAuthMachine.js";
import {
  initialSpaceAuthState,
  routeAfterSpaceEmailCheck,
  spaceAuthReducer,
  SPACE_AUTH_NO_METHOD_MESSAGE,
} from "./spaceAuthMachine.js";

const FULL_MAIL: SpaceAuthCapabilities = {
  smtpConfigured: true,
  magicLoginEnabled: true,
  emailPasswordEnabled: true,
};

const PASSWORD_ONLY: SpaceAuthCapabilities = {
  smtpConfigured: false,
  magicLoginEnabled: false,
  emailPasswordEnabled: true,
};

const NO_METHODS: SpaceAuthCapabilities = {
  smtpConfigured: false,
  magicLoginEnabled: false,
  emailPasswordEnabled: false,
};

function state(overrides: Partial<SpaceAuthMachineState> = {}): SpaceAuthMachineState {
  return { step: "email", mode: "sign-up", email: "", banner: null, ...overrides };
}

describe("routeAfterSpaceEmailCheck", () => {
  it("routes new addresses with mail on to the code step in sign-up mode", () => {
    expect(routeAfterSpaceEmailCheck({ existing: false, status: "MAGIC_CODE" }, FULL_MAIL)).toEqual({
      step: "code",
      mode: "sign-up",
      banner: null,
    });
  });

  it("routes code-backed known addresses to the code step in sign-in mode", () => {
    expect(routeAfterSpaceEmailCheck({ existing: true, status: "MAGIC_CODE" }, FULL_MAIL)).toEqual({
      step: "code",
      mode: "sign-in",
      banner: null,
    });
  });

  it("routes password accounts to the password step even with mail on", () => {
    expect(routeAfterSpaceEmailCheck({ existing: true, status: "CREDENTIAL" }, FULL_MAIL)).toEqual({
      step: "password",
      mode: "sign-in",
      banner: null,
    });
  });

  it("routes to the password step when mail is off", () => {
    expect(routeAfterSpaceEmailCheck({ existing: false, status: "CREDENTIAL" }, PASSWORD_ONLY)).toEqual({
      step: "password",
      mode: "sign-up",
      banner: null,
    });
  });

  it("routes to the password step when magic login is off despite mail", () => {
    const mailWithoutMagic: SpaceAuthCapabilities = { ...FULL_MAIL, magicLoginEnabled: false };
    expect(routeAfterSpaceEmailCheck({ existing: false, status: "MAGIC_CODE" }, mailWithoutMagic)).toEqual({
      step: "password",
      mode: "sign-up",
      banner: null,
    });
  });

  it.each([true, false])("explains itself on the email step with no method (existing: %s)", (existing) => {
    expect(routeAfterSpaceEmailCheck({ existing, status: "CREDENTIAL" }, NO_METHODS)).toEqual({
      step: "email",
      mode: existing ? "sign-in" : "sign-up",
      banner: { message: SPACE_AUTH_NO_METHOD_MESSAGE },
    });
  });
});

describe("initialSpaceAuthState", () => {
  it("starts on the email step in sign-up mode", () => {
    expect(initialSpaceAuthState()).toEqual({ step: "email", mode: "sign-up", email: "", banner: null });
  });

  it("prefills the address", () => {
    expect(initialSpaceAuthState({ email: "Ada@Example.com" }).email).toBe("Ada@Example.com");
  });

  it("treats missing codes as no banner", () => {
    expect(initialSpaceAuthState({ errorCode: null }).banner).toBeNull();
    expect(initialSpaceAuthState({ errorCode: undefined }).banner).toBeNull();
  });

  it("maps a coded link to its fixing step, mode and banner", () => {
    expect(initialSpaceAuthState({ errorCode: "5065" })).toEqual({
      step: "password",
      mode: "sign-in",
      email: "",
      banner: { message: "Sign-in failed. Check your password and try again." },
    });
  });

  it("keeps the home step and mode for codes without either", () => {
    expect(initialSpaceAuthState({ errorCode: "5040" })).toEqual({
      step: "email",
      mode: "sign-up",
      email: "",
      banner: { message: "Enter your email address and password to create an account." },
    });
  });

  it("ignores unknown codes", () => {
    expect(initialSpaceAuthState({ errorCode: "9999" })).toEqual({
      step: "email",
      mode: "sign-up",
      email: "",
      banner: null,
    });
  });
});

describe("spaceAuthReducer", () => {
  it("stores the checked address with its route", () => {
    expect(
      spaceAuthReducer(state({ banner: { message: "stale" } }), {
        type: "checkResolved",
        email: "ada@example.com",
        answer: { existing: true, status: "CREDENTIAL" },
        capabilities: PASSWORD_ONLY,
      })
    ).toEqual({ step: "password", mode: "sign-in", email: "ada@example.com", banner: null });
  });

  it("banners failed requests in place", () => {
    const before = state({ step: "code", mode: "sign-up", email: "ada@example.com" });
    expect(spaceAuthReducer(before, { type: "requestFailed", code: "5102" })).toEqual({
      ...before,
      banner: { message: "That code has expired. Send another code and try again." },
    });
  });

  it("leaves unknown request failures silent", () => {
    const before = state({ email: "ada@example.com" });
    expect(spaceAuthReducer(before, { type: "requestFailed", code: "5900" })).toBe(before);
  });

  it("applies later error codes like initial ones", () => {
    expect(spaceAuthReducer(state(), { type: "errorCode", code: "5092" })).toEqual({
      step: "code",
      mode: "sign-up",
      email: "",
      banner: { message: "That code isn't valid. Check your email and try again." },
    });
    const before = state();
    expect(spaceAuthReducer(before, { type: "errorCode", code: "9999" })).toBe(before);
  });

  it("dismisses the banner and keeps the step", () => {
    const before = state({ step: "password", mode: "sign-in", banner: { message: "nope" } });
    expect(spaceAuthReducer(before, { type: "dismissBanner" })).toEqual({ ...before, banner: null });
    const bare = state();
    expect(spaceAuthReducer(bare, { type: "dismissBanner" })).toBe(bare);
  });

  it("clears the address back to the email step and keeps the mode", () => {
    expect(
      spaceAuthReducer(state({ step: "password", mode: "sign-in", email: "ada@example.com" }), { type: "clearEmail" })
    ).toEqual({ step: "email", mode: "sign-in", email: "", banner: null });
  });

  it("moves to the code step", () => {
    expect(spaceAuthReducer(state({ step: "password", mode: "sign-in" }), { type: "switchToCode" })).toEqual({
      step: "code",
      mode: "sign-in",
      email: "",
      banner: null,
    });
    const already = state({ step: "code" });
    expect(spaceAuthReducer(already, { type: "switchToCode" })).toBe(already);
  });
});
