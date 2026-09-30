// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Sign-in error mapping. The server answers failed form POSTs by
// redirecting back with error_code / error_message query params; the card
// maps each code to its own words and to the step that can fix it. Codes
// outside this table render a generic banner on the email step.

export type SignInStep = "email" | "password" | "code";

export interface SignInError {
  message: string;
  step: SignInStep;
}

const PASSWORD_STEP_CODES: Record<string, string> = {
  USER_DOES_NOT_EXIST: "No account uses this address yet.",
  AUTHENTICATION_FAILED_SIGN_IN: "That password did not match. Try again.",
  REQUIRED_EMAIL_PASSWORD_SIGN_IN: "Enter both your address and your password.",
  INVALID_EMAIL_SIGN_IN: "That address does not look valid.",
  USER_ACCOUNT_DEACTIVATED: "This account has been deactivated. Contact your administrator.",
  PASSWORD_LOGIN_DISABLED: "Password sign-in is disabled on this instance.",
  INSTANCE_NOT_CONFIGURED: "This instance is not set up yet. Contact your administrator.",
};

const CODE_STEP_CODES: Record<string, string> = {
  MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED: "Enter both your address and the emailed code.",
  INVALID_EMAIL_MAGIC_SIGN_IN: "That address does not look valid.",
  INVALID_MAGIC_CODE_SIGN_IN: "That code did not match. Check the latest email and try again.",
  EXPIRED_MAGIC_CODE_SIGN_IN: "That code has expired. Request a fresh one below.",
  EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN: "Too many wrong codes. Request a fresh one and try again.",
  MAGIC_LINK_LOGIN_DISABLED: "Code sign-in is disabled on this instance.",
  SMTP_NOT_CONFIGURED: "This instance cannot send email, so codes are unavailable.",
};

/** Map a server error_code to its banner and recovery step. */
export function describeSignInError(code: string | null | undefined): SignInError | null {
  if (!code) return null;
  const passwordMessage = PASSWORD_STEP_CODES[code];
  if (passwordMessage !== undefined) {
    return { message: passwordMessage, step: "password" };
  }
  const codeMessage = CODE_STEP_CODES[code];
  if (codeMessage !== undefined) {
    return { message: codeMessage, step: "code" };
  }
  return { message: "Sign-in failed. Please try again.", step: "email" };
}
