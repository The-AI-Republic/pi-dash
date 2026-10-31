// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Email settings value model (parity/specs/admin.md §3). Pure helpers
// over the SMTP configuration keys: flag parsing, the TLS/SSL/none
// selector mapping, form validity and dirtiness, and the save payload.
// Components own rendering; this module owns the rules.

/** Master SMTP enable flag. */
export const SMTP_ENABLED_KEY = "ENABLE_SMTP";
/** SMTP server hostname. */
export const SMTP_HOST_KEY = "EMAIL_HOST";
/** SMTP server port. */
export const SMTP_PORT_KEY = "EMAIL_PORT";
/** Sender address for instance mail. */
export const SMTP_FROM_KEY = "EMAIL_FROM";
/** SMTP auth username (optional). */
export const SMTP_USER_KEY = "EMAIL_HOST_USER";
/** SMTP auth password (optional). */
export const SMTP_PASSWORD_KEY = "EMAIL_HOST_PASSWORD";
/** Transport-security TLS flag. */
export const SMTP_TLS_KEY = "EMAIL_USE_TLS";
/** Transport-security SSL flag. */
export const SMTP_SSL_KEY = "EMAIL_USE_SSL";

/**
 * Keys the disable endpoint blanks (ADM-018): the mail values plus the
 * master flag, which it stands down to "0". The transport-security pair
 * is not blanked and is absent here on purpose.
 */
export const BLANKED_MAIL_KEYS: ReadonlySet<string> = new Set([
  SMTP_HOST_KEY,
  SMTP_PORT_KEY,
  SMTP_USER_KEY,
  SMTP_PASSWORD_KEY,
  SMTP_FROM_KEY,
  SMTP_ENABLED_KEY,
]);

/**
 * Flag parse (spec §0.6): numeric parse, nonzero is on, missing is off.
 * Blank and non-numeric values read as off.
 */
export function parseConfigFlag(value: string | null | undefined): boolean {
  if (value === null || value === undefined) return false;
  if (value.trim() === "") return false;
  const numeric = Number(value);
  return !Number.isNaN(numeric) && numeric !== 0;
}

/** Flag format: saves write flags back as "1"/"0" strings. */
export function formatConfigFlag(on: boolean): "1" | "0" {
  return on ? "1" : "0";
}

/** Transport-security choice rendered by the selector (ADM-020). */
export type EmailSecurityChoice = "tls" | "ssl" | "none";

/**
 * Displayed choice derived from the two stored flags, defaulting to
 * none, with TLS winning a both-on tie.
 */
export function securityChoiceFromFlags(
  tls: string | null | undefined,
  ssl: string | null | undefined
): EmailSecurityChoice {
  if (parseConfigFlag(tls)) return "tls";
  if (parseConfigFlag(ssl)) return "ssl";
  return "none";
}

/**
 * Stored flag pair for a choice: TLS and SSL each set exactly one flag,
 * none sets neither, so exactly one or neither flag is ever on.
 */
export function securityFlagsFromChoice(choice: EmailSecurityChoice): Record<string, "1" | "0"> {
  return {
    [SMTP_TLS_KEY]: formatConfigFlag(choice === "tls"),
    [SMTP_SSL_KEY]: formatConfigFlag(choice === "ssl"),
  };
}

/** SMTP form values: five text fields plus the security choice. */
export interface EmailFormValues {
  host: string;
  port: string;
  from: string;
  user: string;
  password: string;
  security: EmailSecurityChoice;
}

/** Raw configuration values by key; null rows and missing keys read as blank. */
export type ConfigValueMap = Record<string, string | null | undefined>;

function textValue(value: string | null | undefined): string {
  return value ?? "";
}

/** Form values for a configuration map: stored values verbatim. */
export function emailFormValuesFromConfig(values: ConfigValueMap): EmailFormValues {
  return {
    host: textValue(values[SMTP_HOST_KEY]),
    port: textValue(values[SMTP_PORT_KEY]),
    from: textValue(values[SMTP_FROM_KEY]),
    user: textValue(values[SMTP_USER_KEY]),
    password: textValue(values[SMTP_PASSWORD_KEY]),
    security: securityChoiceFromFlags(values[SMTP_TLS_KEY], values[SMTP_SSL_KEY]),
  };
}

/**
 * Form validity (ADM-019): host, port, and sender address are required
 * (blank means blank after trimming); username, password, and the
 * security choice never block.
 */
export function isEmailFormValid(values: EmailFormValues): boolean {
  return values.host.trim() !== "" && values.port.trim() !== "" && values.from.trim() !== "";
}

/**
 * Form dirtiness (ADM-019, ADM-020): any field or the security choice
 * differing from the loaded values. The picker dirties the form like any
 * other control (NEWFRONT-478 intent), so a security-only flip enables
 * save on its own.
 */
export function isEmailFormDirty(current: EmailFormValues, initial: EmailFormValues): boolean {
  return (
    current.host !== initial.host ||
    current.port !== initial.port ||
    current.from !== initial.from ||
    current.user !== initial.user ||
    current.password !== initial.password ||
    current.security !== initial.security
  );
}

/**
 * Save payload (ADM-019): the mail keys plus the security flags, always
 * forcing the master flag back on.
 */
export function buildSmtpPatch(values: EmailFormValues): Record<string, string> {
  return {
    [SMTP_HOST_KEY]: values.host,
    [SMTP_PORT_KEY]: values.port,
    [SMTP_FROM_KEY]: values.from,
    [SMTP_USER_KEY]: values.user,
    [SMTP_PASSWORD_KEY]: values.password,
    ...securityFlagsFromChoice(values.security),
    [SMTP_ENABLED_KEY]: "1",
  };
}
