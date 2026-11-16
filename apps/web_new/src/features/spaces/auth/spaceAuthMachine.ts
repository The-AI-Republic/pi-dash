// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Flow state machine for the public spaces auth card
// (parity/specs/space.md SPC-058-064). The card walks email → address
// check → code or password step; the reducer owns the step, the mode, the
// checked address and the banner, while the component performs the async
// calls and dispatches their outcomes. Field-level input state (drafts,
// focus, timers) stays in the component: the machine only sees resolved
// answers. Every event is total — unknown codes and stray events leave
// the state untouched rather than throwing.
import { bannerForSpaceAuthFailure, describeSpaceAuthCode } from "./spaceAuthErrors.js";

export type SpaceAuthStep = "email" | "code" | "password";

export type SpaceAuthMode = "sign-in" | "sign-up";

/** Instance capabilities the card routes on; plain data from the mounter. */
export interface SpaceAuthCapabilities {
  smtpConfigured: boolean;
  magicLoginEnabled: boolean;
  emailPasswordEnabled: boolean;
}

/** Answer of the spaces email check (see @pidash/api-client SpaceEmailCheck). */
export interface SpaceAuthCheckAnswer {
  existing: boolean;
  status: "MAGIC_CODE" | "CREDENTIAL";
}

export interface SpaceAuthBanner {
  message: string;
}

export interface SpaceAuthMachineState {
  step: SpaceAuthStep;
  mode: SpaceAuthMode;
  email: string;
  banner: SpaceAuthBanner | null;
}

export type SpaceAuthMachineEvent =
  | {
      type: "checkResolved";
      email: string;
      answer: SpaceAuthCheckAnswer;
      capabilities: SpaceAuthCapabilities;
    }
  | { type: "requestFailed"; code: string }
  | { type: "errorCode"; code: string | number }
  | { type: "dismissBanner" }
  | { type: "clearEmail" }
  | { type: "switchToCode" };

/** Shown when the instance offers neither method, in both modes. */
export const SPACE_AUTH_NO_METHOD_MESSAGE =
  "Signing in isn't available on this instance. Ask your administrator to enable a sign-in method.";

export interface SpaceAuthRoute {
  step: SpaceAuthStep;
  mode: SpaceAuthMode;
  banner: SpaceAuthBanner | null;
}

/**
 * Route an email-check answer: known addresses sign in, new ones sign up;
 * the code step wins when mail is on and the account is code-backed,
 * otherwise the password step when enabled, otherwise the email step keeps
 * the address with an explanatory banner.
 */
export function routeAfterSpaceEmailCheck(
  answer: SpaceAuthCheckAnswer,
  capabilities: SpaceAuthCapabilities
): SpaceAuthRoute {
  const mode: SpaceAuthMode = answer.existing ? "sign-in" : "sign-up";
  const codeReady = capabilities.smtpConfigured && capabilities.magicLoginEnabled && answer.status === "MAGIC_CODE";
  if (codeReady) return { step: "code", mode, banner: null };
  if (capabilities.emailPasswordEnabled) return { step: "password", mode, banner: null };
  return { step: "email", mode, banner: { message: SPACE_AUTH_NO_METHOD_MESSAGE } };
}

/**
 * Apply a server error code carried on the landing address: known codes
 * show their banner and move to the fixing step/mode they name, while
 * codes without a step or mode keep the card's. Unknown codes change
 * nothing. Consumed once, when the card mounts.
 */
function applyErrorCode(state: SpaceAuthMachineState, code: string | number): SpaceAuthMachineState {
  const mapping = describeSpaceAuthCode(code);
  if (mapping === null) return state;
  return {
    step: mapping.step ?? state.step,
    mode: mapping.mode ?? state.mode,
    email: state.email,
    banner: { message: mapping.message },
  };
}

export interface InitialSpaceAuth {
  email?: string | undefined;
  errorCode?: string | number | null | undefined;
}

/** Home state: the email step in sign-up mode, with the coded banner if any. */
export function initialSpaceAuthState(init: InitialSpaceAuth = {}): SpaceAuthMachineState {
  const state: SpaceAuthMachineState = {
    step: "email",
    mode: "sign-up",
    email: init.email ?? "",
    banner: null,
  };
  if (init.errorCode === undefined || init.errorCode === null) return state;
  return applyErrorCode(state, init.errorCode);
}

export function spaceAuthReducer(state: SpaceAuthMachineState, event: SpaceAuthMachineEvent): SpaceAuthMachineState {
  switch (event.type) {
    case "checkResolved": {
      // A resolved check supersedes the banner: the new step owns the
      // card now, so a failure from the previous step must not follow it.
      const route = routeAfterSpaceEmailCheck(event.answer, event.capabilities);
      return { step: route.step, mode: route.mode, email: event.email, banner: route.banner };
    }
    case "requestFailed": {
      const message = bannerForSpaceAuthFailure(event.code);
      if (message === null) return state;
      return { ...state, banner: { message } };
    }
    case "errorCode":
      return applyErrorCode(state, event.code);
    case "dismissBanner":
      if (state.banner === null) return state;
      return { ...state, banner: null };
    case "clearEmail":
      return { ...state, step: "email", email: "" };
    case "switchToCode":
      if (state.step === "code") return state;
      return { ...state, step: "code" };
  }
}
