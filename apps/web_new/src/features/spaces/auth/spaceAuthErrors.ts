// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spaces auth failure catalog (parity/specs/space.md SPC-063, §5.4). Native
// form posts that fail redirect back to the spaces base carrying an
// error_code; the card maps each known code to banner copy plus the step
// and mode that can fix it. Codes the catalog does not know — including
// the deliberately unmodelled rate-limit code and Gitea configuration
// failure — map to null: no banner, the card stays where it is. The code
// itself is never shown and the server's raw message is never echoed.
import type { SpaceAuthMode, SpaceAuthStep } from "./spaceAuthMachine.js";

/** Banner copy plus the fixing step/mode; absent step/mode keeps the card's. */
export interface SpaceAuthCodeMapping {
  message: string;
  step?: SpaceAuthStep | undefined;
  mode?: SpaceAuthMode | undefined;
}

// Wire values first: what the server sends in error_code (spec §5.4;
// cross-checked against SPACE_AUTH_ERROR_CODES in @pidash/api-client).
const CATALOG: Record<string, SpaceAuthCodeMapping> = {
  "5000": { message: "This instance isn't set up yet. Contact your administrator." },
  "5005": { message: "That email address doesn't look right." },
  "5010": { message: "Enter your email address." },
  "5015": { message: "Creating new accounts is disabled on this instance. Contact your administrator." },
  "5019": { message: "This account has been deactivated. Contact your administrator." },
  "5020": { message: "That password isn't valid. Try again.", step: "password" },
  "5025": { message: "This instance can't send email, so codes are unavailable. Contact your administrator." },
  "5030": { message: "This address is already registered. Sign in with it instead.", step: "email" },
  "5035": { message: "Sign-up failed. Check your details and try again.", step: "password", mode: "sign-up" },
  "5040": { message: "Enter your email address and password to create an account." },
  "5045": { message: "That email address doesn't look right." },
  "5050": { message: "That email address doesn't look right." },
  "5055": { message: "Enter the code from your email.", step: "code", mode: "sign-up" },
  "5060": { message: "No account uses this address. Create one to get started.", step: "email" },
  "5065": { message: "Sign-in failed. Check your password and try again.", step: "password", mode: "sign-in" },
  "5070": { message: "Enter your email address and password to sign in." },
  "5075": { message: "That email address doesn't look right." },
  "5080": { message: "That email address doesn't look right." },
  "5085": { message: "Enter the code from your email.", step: "code", mode: "sign-in" },
  "5090": { message: "That code isn't valid. Check your email and try again.", step: "code", mode: "sign-in" },
  "5092": { message: "That code isn't valid. Check your email and try again.", step: "code", mode: "sign-up" },
  "5095": { message: "That code has expired. Send another code and try again.", step: "code", mode: "sign-in" },
  "5097": { message: "That code has expired. Send another code and try again.", step: "code", mode: "sign-up" },
  "5100": { message: "That code has expired. Send another code and try again.", step: "code", mode: "sign-in" },
  "5102": { message: "That code has expired. Send another code and try again.", step: "code", mode: "sign-up" },
  "5104": { message: "Provider sign-in isn't configured. Contact your administrator." },
  "5105": { message: "Google sign-in isn't configured. Contact your administrator." },
  "5110": { message: "GitHub sign-in isn't configured. Contact your administrator." },
  "5111": { message: "GitLab sign-in isn't configured. Contact your administrator." },
  "5115": { message: "Google couldn't sign you in. Try again." },
  "5120": { message: "GitHub couldn't sign you in. Try again." },
  "5121": { message: "GitLab couldn't sign you in. Try again." },
  "5122": { message: "Your GitHub account isn't in the required organization." },
};

/** Normalize the wire value; the JSON envelope may carry a number. */
function normalizeCode(code: string | number): string {
  return typeof code === "number" ? String(code) : code.trim();
}

/**
 * Map a server error code to its banner and fixing step/mode, or null when
 * the catalog does not model it (unknown numbers, the rate limit, the Gitea
 * configuration failure). A mapping without a step or mode keeps the card's
 * current step or mode.
 */
export function describeSpaceAuthCode(code: string | number): SpaceAuthCodeMapping | null {
  return CATALOG[normalizeCode(code)] ?? null;
}

/** Banner shown when the request itself failed (offline, timeout, 5xx page). */
export const SPACE_AUTH_REQUEST_FAILED_MESSAGE = "The request failed. Check your connection and try again.";

/**
 * Banner for a failed JSON call: numeric server codes read the catalog
 * (unknown ones stay silent); anything else means the request never got a
 * usable answer, so the card says so instead of stranding the visitor.
 */
export function bannerForSpaceAuthFailure(code: string): string | null {
  if (!/^\d+$/.test(code)) return SPACE_AUTH_REQUEST_FAILED_MESSAGE;
  return describeSpaceAuthCode(code)?.message ?? null;
}
