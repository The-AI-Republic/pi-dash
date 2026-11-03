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

/** Longest repository URL and base branch the form accepts. */
export const CREATE_REPO_URL_MAX_LENGTH = 512;
export const CREATE_BASE_BRANCH_MAX_LENGTH = 128;

/** Characters a base branch may contain. */
export const BASE_BRANCH_PATTERN = /^[A-Za-z0-9._/-]*$/;

/** Guidance the description field carries on the create form (target main#503). */
export const CREATE_DESCRIPTION_PLACEHOLDER = "Project Context (Will be shown to AI agents)";

/** Reserved identifier shape: one to ten uppercase ASCII letters or digits. */
export const IDENTIFIER_PATTERN = /^[\p{L}\p{N}]{1,10}$/u;

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
  repoUrl?: string;
  baseBranch?: string;
}

export interface CreateFieldErrors {
  name?: string | undefined;
  identifier?: string | undefined;
  repoUrl?: string | undefined;
  baseBranch?: string | undefined;
}

/** Client-side rules: identity required; repository and branch length/shape capped. */
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
  } else if (!IDENTIFIER_PATTERN.test(values.identifier.toUpperCase())) {
    errors.identifier = "Use up to 10 letters and numbers only.";
  }
  if ((values.repoUrl ?? "").trim().length > CREATE_REPO_URL_MAX_LENGTH) {
    errors.repoUrl = `Keep the repository URL to ${CREATE_REPO_URL_MAX_LENGTH} characters or fewer.`;
  }
  if ((values.baseBranch ?? "").length > CREATE_BASE_BRANCH_MAX_LENGTH) {
    errors.baseBranch = `Keep the base branch to ${CREATE_BASE_BRANCH_MAX_LENGTH} characters.`;
  } else if (!BASE_BRANCH_PATTERN.test(values.baseBranch ?? "")) {
    errors.baseBranch = "Base branch may only contain letters, numbers, and . _ / -.";
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
  repoUrl?: string;
  baseBranch?: string;
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
    repo_url: (values.repoUrl ?? "").trim(),
    base_branch: (values.baseBranch ?? "").trim(),
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
