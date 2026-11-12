// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin entry error surfacing (ADM-006, ADM-008, from
// parity/specs/admin.md §1.6 and §1.8). Pure resolvers over the
// error_code / error_message query params the native auth POSTs
// redirect back with. Copy here is fresh; only the wire values the
// backend sends (code strings, numbers, param names) are reused.

/** Console entry: the mapped-banner link target (NEWFRONT-458 intent). */
export const GOD_MODE_ENTRY = "/god-mode/";

function verbatim(message: string | undefined): string | null {
  return message !== undefined && message.length > 0 ? message : null;
}

// --- Setup form (ADM-006) ---------------------------------------------------

export type SetupErrorPlacement = "inline-email" | "inline-password" | "banner" | "silent";

export type SetupError =
  | { placement: "silent"; text: null }
  | { placement: "inline-email" | "inline-password" | "banner"; text: string };

/** Codes that render under the email field: the named case plus the numeric the server sends. */
const SETUP_INLINE_EMAIL_CODES = new Set(["INVALID_EMAIL", "5160"]);

/** Codes that render under the password field: named, weak-secret numeric, and the unused password numeric. */
const SETUP_INLINE_PASSWORD_CODES = new Set(["INVALID_PASSWORD", "5021", "5165"]);

/**
 * Codes that render as the top banner, each with the fallback shown when
 * the redirect carries no message. Named codes keep the crafted-code
 * paths; numerics are the 5150-5190 admin series plus the
 * not-configured code the setup POST can return (NEWFRONT-457 intent:
 * every rejected post renders where crafted codes render).
 */
const SETUP_BANNER_DEFAULTS: ReadonlyMap<string, string> = new Map([
  ["INSTANCE_NOT_CONFIGURED", "This instance is not configured yet."],
  ["ADMIN_ALREADY_EXIST", "This instance already has an administrator."],
  ["REQUIRED_EMAIL_PASSWORD_FIRST_NAME", "First name, email, and password are all required."],
  ["USER_ALREADY_EXISTS", "That email address is already registered."],
  ["5000", "This instance is not configured yet."],
  ["5150", "This instance already has an administrator."],
  ["5155", "First name, email, and password are all required."],
  ["5170", "Email and password are both required."],
  ["5175", "Authentication failed."],
  ["5180", "That email address is already registered."],
  ["5185", "No account matches that email address."],
  ["5190", "That account has been deactivated."],
]);

export function resolveSetupError(code: string | undefined, message: string | undefined): SetupError {
  if (code === undefined) return { placement: "silent", text: null };
  if (SETUP_INLINE_EMAIL_CODES.has(code)) {
    return { placement: "inline-email", text: verbatim(message) ?? "That email address is not valid." };
  }
  if (SETUP_INLINE_PASSWORD_CODES.has(code)) {
    return { placement: "inline-password", text: verbatim(message) ?? "That password does not meet the requirements." };
  }
  const fallback = SETUP_BANNER_DEFAULTS.get(code);
  if (fallback === undefined) return { placement: "silent", text: null };
  return { placement: "banner", text: verbatim(message) ?? fallback };
}

// --- Sign-in form (ADM-008) --------------------------------------------------

/** Named codes that render the generic banner with the server message verbatim. */
const SIGN_IN_GENERIC_CODES = new Set([
  "INSTANCE_NOT_CONFIGURED",
  "REQUIRED_EMAIL_PASSWORD",
  "INVALID_EMAIL",
  "USER_DOES_NOT_EXIST",
  "AUTHENTICATION_FAILED",
]);

export interface SignInMappedBanner {
  title: string;
  message: string;
  /** Present on the two mappings that send the admin back to the entry. */
  linkHref: string | null;
}

/**
 * Client-side copy for the nine numeric admin auth codes. Titles and
 * messages are fresh; the wrong-secret mapping keeps the
 * authentication-failure wording the parity scenario reads.
 */
const SIGN_IN_MAPPED: Readonly<Record<string, SignInMappedBanner>> = {
  "5150": {
    title: "An admin already exists",
    message: "This instance already has an administrator. Sign in with that account.",
    linkHref: null,
  },
  "5155": {
    title: "Missing required fields",
    message: "Email, password, and first name are all required.",
    linkHref: null,
  },
  "5160": {
    title: "Email address not valid",
    message: "Check the address for typos and try again.",
    linkHref: null,
  },
  "5165": {
    title: "Password not valid",
    message: "Check the password requirements and try again.",
    linkHref: null,
  },
  "5170": {
    title: "Missing email or password",
    message: "Enter both your email address and your password.",
    linkHref: null,
  },
  "5175": {
    title: "Authentication failed",
    message: "The email or password did not match. Check both and try again.",
    linkHref: null,
  },
  "5180": {
    title: "Account already registered",
    message: "That email address is already registered.",
    linkHref: GOD_MODE_ENTRY,
  },
  "5185": {
    title: "Unknown admin account",
    message: "No admin account matches that email address.",
    linkHref: GOD_MODE_ENTRY,
  },
  "5190": {
    title: "Account deactivated",
    message: "This admin account has been deactivated. Contact your administrator to restore access.",
    linkHref: null,
  },
};

export interface SignInBanners {
  /** Verbatim server message, or null when the generic path does not match. */
  generic: string | null;
  /** Mapped info banner, or null when no numeric code matches. */
  mapped: SignInMappedBanner | null;
}

export function resolveSignInBanners(code: string | undefined, message: string | undefined): SignInBanners {
  if (code === undefined) return { generic: null, mapped: null };
  const text = verbatim(message);
  if (text !== null && SIGN_IN_GENERIC_CODES.has(code)) {
    // The generic banner takes precedence when both paths match.
    return { generic: text, mapped: null };
  }
  return { generic: null, mapped: SIGN_IN_MAPPED[code] ?? null };
}
