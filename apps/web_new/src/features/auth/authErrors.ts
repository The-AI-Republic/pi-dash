// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth error mapping. The server answers failed native-form POSTs by
// redirecting back with a numeric error_code query param; the cards map
// each code to fresh banner copy plus the step and mode that can fix it.
// Written from parity/specs/auth.md AUTH-006; code meanings verified
// against the backend error table (apps/api). Unknown codes map to null:
// no banner, the card stays on its email step.

export type SignInStep = "email" | "password" | "code";

export type AuthMode = "sign-in" | "sign-up";

export interface AuthErrorAction {
  label: string;
  href: string;
}

export interface AuthError {
  message: string;
  step: SignInStep;
  mode: AuthMode;
  action?: AuthErrorAction | undefined;
}

interface ErrorSpec {
  message: string;
  step: SignInStep;
  /** Explicit mode for codes with sign-in/sign-up variants; otherwise the card's home mode. */
  mode?: AuthMode | undefined;
  /** Cross-link the banner carries; the typed address is preserved in its href. */
  link?: "sign-in" | "sign-up" | undefined;
}

const ADMIN_RANGE_MESSAGE = "An administrator request failed. Contact your administrator.";

const ERROR_SPECS: Record<string, ErrorSpec> = {
  "5000": { message: "This instance is not set up yet. Contact your administrator.", step: "email" },
  "5005": { message: "That address does not look valid.", step: "email" },
  "5010": { message: "Enter your email address.", step: "email" },
  "5015": { message: "Sign up disabled on this instance.", step: "email" },
  "5016": { message: "Code sign-in is disabled on this instance.", step: "email" },
  "5018": { message: "Password sign-in is disabled on this instance.", step: "email" },
  "5019": { message: "This account has been deactivated. Contact your administrator.", step: "email" },
  "5020": { message: "That password is not valid.", step: "email" },
  "5021": { message: "That password is too weak. Choose a stronger one.", step: "email" },
  "5025": { message: "This instance cannot send email, so codes are unavailable.", step: "email" },
  "5030": {
    message: "An account with this address already exists.",
    step: "email",
    link: "sign-in",
  },
  "5035": {
    message: "Authentication failed. Check your password and try again.",
    step: "password",
    mode: "sign-up",
  },
  "5040": { message: "Enter your address and password to create an account.", step: "email" },
  "5045": { message: "That address does not look valid.", step: "email" },
  "5050": { message: "That address cannot receive a sign-up code.", step: "code", mode: "sign-up" },
  "5055": { message: "Enter your address and the emailed code.", step: "email" },
  "5060": { message: "No account found for this address.", step: "email", link: "sign-up" },
  "5065": {
    message: "Authentication failed. Check your password and try again.",
    step: "password",
    mode: "sign-in",
  },
  "5070": { message: "Enter your address and password.", step: "email" },
  "5075": { message: "That address does not look valid.", step: "email" },
  "5080": { message: "That address cannot receive a sign-in code.", step: "code", mode: "sign-in" },
  "5085": { message: "Enter your address and the emailed code.", step: "email" },
  "5090": {
    message: "Invalid magic code. Check the latest email and try again.",
    step: "code",
    mode: "sign-in",
  },
  "5092": {
    message: "Invalid magic code. Check the latest email and try again.",
    step: "code",
    mode: "sign-up",
  },
  "5095": {
    message: "Expired magic code. Request a fresh code and try again.",
    step: "code",
    mode: "sign-in",
  },
  "5097": {
    message: "Expired magic code. Request a fresh code and try again.",
    step: "code",
    mode: "sign-up",
  },
  "5100": {
    message: "Expired magic code. Request a fresh code and try again.",
    step: "code",
    mode: "sign-in",
  },
  "5102": {
    message: "Expired magic code. Request a fresh code and try again.",
    step: "code",
    mode: "sign-up",
  },
  "5104": {
    message: "Sign-in with a provider is not set up. Contact your administrator.",
    step: "email",
  },
  "5105": { message: "Google sign-in is not set up. Contact your administrator.", step: "email" },
  "5110": { message: "GitHub sign-in is not set up. Contact your administrator.", step: "email" },
  "5111": { message: "GitLab sign-in is not set up. Contact your administrator.", step: "email" },
  "5112": { message: "Gitea sign-in is not set up. Contact your administrator.", step: "email" },
  "5115": { message: "Google sign-in failed. Try again or use another method.", step: "email" },
  "5120": { message: "GitHub sign-in failed. Try again or use another method.", step: "email" },
  "5121": { message: "GitLab sign-in failed. Try again or use another method.", step: "email" },
  "5122": { message: "Your GitHub account is not in the required organization.", step: "email" },
  "5123": { message: "Gitea sign-in failed. Try again or use another method.", step: "email" },
  "5125": { message: "That reset link is not valid. Request a fresh one.", step: "email" },
  "5130": { message: "That reset link has expired. Request a fresh one.", step: "email" },
  "5135": { message: "The current password did not match.", step: "email" },
  "5138": { message: "Enter your password.", step: "email" },
  "5140": { message: "That new password is not valid.", step: "email" },
  "5145": { message: "A password is already set on this account.", step: "email" },
  "5900": { message: "Rate limit exceeded. Wait a moment and try again.", step: "email" },
};

function linkFor(target: "sign-in" | "sign-up", email: string): AuthErrorAction {
  const preserved = encodeURIComponent(email);
  return target === "sign-in"
    ? { label: "Sign in", href: `/?email=${preserved}` }
    : { label: "Create an account", href: `/sign-up?email=${preserved}` };
}

export interface SubmitFailure {
  message: string;
  step: SignInStep;
  mode: AuthMode;
  action: AuthErrorAction | null;
}

/**
 * Map a failed credential POST to its banner. Recognized codes recover
 * through the matrix; anything else surfaces the server message on the
 * submitting step so the failure is never silent.
 */
export function describeSubmitFailure(
  result: { code: string; message: string },
  address: string,
  fallback: { step: SignInStep; mode: AuthMode }
): SubmitFailure {
  const mapped = describeAuthError(result.code, "sign-in", address);
  if (!mapped) {
    return { message: result.message, step: fallback.step, mode: fallback.mode, action: null };
  }
  return { message: mapped.message, step: mapped.step, mode: mapped.mode, action: mapped.action ?? null };
}

/** Map a server error_code to its banner, recovery step, and mode. Unknown codes map to null. */
export function describeAuthError(
  code: string | null | undefined,
  homeMode: AuthMode = "sign-in",
  email = ""
): AuthError | null {
  if (!code) return null;
  const spec = ERROR_SPECS[code];
  if (spec === undefined) {
    if (/^\d+$/.test(code)) {
      const numeric = Number(code);
      if (numeric >= 5150 && numeric <= 5190) {
        return { message: ADMIN_RANGE_MESSAGE, step: "email", mode: homeMode };
      }
    }
    return null;
  }
  return {
    message: spec.message,
    step: spec.step,
    mode: spec.mode ?? homeMode,
    ...(spec.link === undefined ? {} : { action: linkFor(spec.link, email) }),
  };
}
