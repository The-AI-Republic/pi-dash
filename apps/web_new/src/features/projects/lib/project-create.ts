// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-create helpers (SHELL-040, SHELL-041, SHELL-042; written
// from parity/specs/shell.md §11). Pure rules the create dialog
// consumes: short-code shaping, browser-timezone default, icon
// prefill, client-side validation, duplicate-error mapping and the
// single-POST payload. The payload carries no cover keys at all:
// creation never uploads, confirms or patches a cover (SHELL-042).
import { isApiError, type ProjectCreateInput } from "@pidash/api-client";

/** Longest project name the server stores. */
export const CREATE_NAME_MAX_LENGTH = 255;

/** Longest short code the form keeps while deriving or typing. */
export const CREATE_IDENTIFIER_MAX_LENGTH = 10;

/** Visibility values the server reads (private hides, public shares). */
export const CREATE_NETWORK_PRIVATE = 0;
export const CREATE_NETWORK_PUBLIC = 2;

/** Server codes the create POST answers for reused name or code. */
export const NAME_TAKEN_CODE = "PROJECT_NAME_ALREADY_EXIST";
export const IDENTIFIER_TAKEN_CODE = "PROJECT_IDENTIFIER_ALREADY_EXIST";

export const NAME_TAKEN_MESSAGE = "The project name is already taken.";
export const IDENTIFIER_TAKEN_MESSAGE = "The project identifier is already taken.";

/** Fallback icon when the random pick lands out of range. */
export const CREATE_DEFAULT_ICON = "🌊";

/** Emoji the fresh form may prefill; the picker offers the same set. */
export const CREATE_ICON_CHOICES: ReadonlyArray<string> = [
  CREATE_DEFAULT_ICON,
  "🌿",
  "🔥",
  "⚡",
  "🌙",
  "☀️",
  "🎯",
  "🧭",
  "💎",
  "🚲",
  "🎨",
  "📚",
  "🌻",
  "🍀",
  "🐝",
  "🦊",
];

/**
 * Keep letters and digits in any script; drop separators, marks and
 * symbols so the short code stays a compact token.
 */
export function sanitizeProjectIdentifier(value: string): string {
  return value.replace(/[^\p{L}\p{N}]/gu, "");
}

/** Derive the short code from the typed name, capped to form length. */
export function deriveProjectIdentifier(name: string): string {
  return sanitizeProjectIdentifier(name).slice(0, CREATE_IDENTIFIER_MAX_LENGTH);
}

/**
 * The browser's IANA zone for the fresh form, UTC when detection is
 * missing or throws.
 */
export function resolveBrowserTimezone(): string {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    return zone === undefined || zone === "" ? "UTC" : zone;
  } catch {
    return "UTC";
  }
}

/** Random prefill icon; the random source injects for tests. */
export function pickCreateIcon(random: () => number = Math.random): string {
  const index = Math.floor(random() * CREATE_ICON_CHOICES.length);
  return CREATE_ICON_CHOICES[index] ?? CREATE_DEFAULT_ICON;
}

/** Logo value the cards render as an emoji glyph. */
export function buildCreateLogoProps(emoji: string): Record<string, unknown> {
  return { in_use: "emoji", emoji: { value: emoji } };
}

export interface CreateFieldValues {
  name: string;
  identifier: string;
}

export interface CreateFieldErrors {
  name?: string | undefined;
  identifier?: string | undefined;
}

/** Client-side rules: both fields required, both length-capped. */
export function validateCreateFields(values: CreateFieldValues): CreateFieldErrors {
  const errors: CreateFieldErrors = {};
  if (values.name.trim() === "") {
    errors.name = "Enter a project name.";
  } else if (values.name.length > CREATE_NAME_MAX_LENGTH) {
    errors.name = `Keep the name to ${CREATE_NAME_MAX_LENGTH} characters or fewer.`;
  }
  if (values.identifier.trim() === "") {
    errors.identifier = "Enter a short code.";
  } else if (values.identifier.length > CREATE_IDENTIFIER_MAX_LENGTH) {
    errors.identifier = `Keep the short code to ${CREATE_IDENTIFIER_MAX_LENGTH} characters.`;
  }
  return errors;
}

/**
 * Map a failed create POST onto the offending fields. Reused names or
 * codes surface the targeted taken messages; anything else leaves both
 * fields clear so the caller falls back to a generic notice.
 */
export function selectCreateFieldErrors(error: unknown): CreateFieldErrors {
  if (!isApiError(error) || error.fields === undefined) return {};
  const errors: CreateFieldErrors = {};
  if (error.fields["name"]?.includes(NAME_TAKEN_CODE)) errors.name = NAME_TAKEN_MESSAGE;
  if (error.fields["identifier"]?.includes(IDENTIFIER_TAKEN_CODE)) errors.identifier = IDENTIFIER_TAKEN_MESSAGE;
  return errors;
}

export interface CreatePayloadValues extends CreateFieldValues {
  description: string;
  network: number;
  projectLead: string | null;
  icon: string;
  timezone: string;
}

/**
 * The single-POST body (SHELL-040, SHELL-041): identity, visibility,
 * lead and icon plus the work-items-only view flags. No cover keys —
 * nothing here uploads, confirms or patches a cover (SHELL-042).
 */
export function buildCreatePayload(values: CreatePayloadValues): ProjectCreateInput {
  return {
    name: values.name.trim(),
    identifier: values.identifier.toUpperCase(),
    description: values.description,
    network: values.network,
    project_lead: values.projectLead,
    logo_props: buildCreateLogoProps(values.icon),
    timezone: values.timezone,
    cycle_view: false,
    module_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
  };
}
