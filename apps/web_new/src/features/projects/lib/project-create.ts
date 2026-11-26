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

/** Repository list providers the server accepts (NEWPIDASH-409). */
export const CREATE_REPO_PROVIDERS = ["github", "gitlab", "other"] as const;
export type CreateRepoProvider = (typeof CREATE_REPO_PROVIDERS)[number];

/** Provider labels the create form renders. */
export const CREATE_REPO_PROVIDER_LABELS: Record<CreateRepoProvider, string> = {
  github: "GitHub",
  gitlab: "GitLab",
  other: "Other",
};

/** Caption a row shows when the entry follows the repo's own default branch. */
export const CREATE_REPO_DEFAULT_BRANCH_LABEL = "repo default";

/** Field-level messages the repository rows render inline. */
export const CREATE_REPO_URL_REQUIRED_MESSAGE = "Enter a repository URL.";
export const CREATE_REPO_URL_TOO_LONG_MESSAGE = `Keep the repository URL to ${CREATE_REPO_URL_MAX_LENGTH} characters or fewer.`;
export const CREATE_REPO_URL_INVALID_MESSAGE = "Enter a valid repository URL.";
export const CREATE_REPO_BRANCH_TOO_LONG_MESSAGE = `Keep the base branch to ${CREATE_BASE_BRANCH_MAX_LENGTH} characters.`;
export const CREATE_REPO_PROVIDER_MESSAGE = "Choose GitHub, GitLab or Other.";
export const CREATE_REPO_DUPLICATE_MESSAGE = "This repository is already in the list.";

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

/** One repository draft on the create form (NEWPIDASH-413). */
export interface CreateRepoDraft {
  url: string;
  provider: CreateRepoProvider;
  /** Blank means the repo's own default branch. */
  baseBranch: string;
}

export interface CreateFieldValues {
  name: string;
  identifier: string;
  repos?: CreateRepoDraft[];
}

/** Per-row errors the create form renders inline; `row` is a whole-row server message. */
export interface CreateRepoRowErrors {
  url?: string | undefined;
  provider?: string | undefined;
  baseBranch?: string | undefined;
  row?: string | undefined;
}

export interface CreateFieldErrors {
  name?: string | undefined;
  identifier?: string | undefined;
  repos?: CreateRepoRowErrors[] | undefined;
  /** A list-level refusal that names no row. */
  reposError?: string | undefined;
}

/** Client-side rules: identity required; every repository row validated. */
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
  const rows = validateCreateRepoRows(values.repos ?? []);
  if (rows.some((row) => Object.keys(row).length > 0)) {
    errors.repos = rows;
  }
  return errors;
}

/**
 * Infer the provider label for a repository URL (NEWPIDASH-413): the
 * client's mirror of the server rule — `github.com` reads as GitHub,
 * `gitlab.com` as GitLab, anything else (including an unparseable URL)
 * as Other. Kept beside the settings tab's copy; the server stays
 * authoritative on every write.
 */
export function inferCreateRepoProvider(url: string): CreateRepoProvider {
  const host = createRepoUrlHost(url);
  if (host === "github.com") return "github";
  if (host === "gitlab.com") return "gitlab";
  return "other";
}

/** Whitespace or control characters disqualify a URL (mirrors the server). */
function hasBadCreateUrlChar(candidate: string): boolean {
  for (const char of candidate) {
    if (/\s/.test(char)) return true;
    const code = char.codePointAt(0) ?? 0;
    if (code <= 0x1f || code === 0x7f || (code >= 0x80 && code <= 0x9f)) return true;
  }
  return false;
}

/** Lowercase host of a repository URL, or null when it has no usable one. */
function createRepoUrlHost(url: string): string | null {
  const candidate = url.trim();
  if (candidate === "" || hasBadCreateUrlChar(candidate)) return null;
  const schemeAt = candidate.indexOf("://");
  if (schemeAt !== -1) {
    const scheme = candidate.slice(0, schemeAt).toLowerCase();
    if (scheme !== "http" && scheme !== "https" && scheme !== "ssh") return null;
    const authority = candidate.slice(schemeAt + 3).split("/", 1)[0] ?? "";
    const hostPort = authority.split("@").pop() ?? "";
    const host = hostPort.toLowerCase().replace(/:(443|80)$/, "");
    return host === "" ? null : host;
  }
  const colonAt = candidate.indexOf(":");
  if (colonAt === -1) return null;
  const left = candidate.slice(0, colonAt);
  const path = candidate.slice(colonAt + 1);
  if (left === "" || left.includes("/") || path === "" || path.startsWith("/") || path.includes("\\")) return null;
  const host = (left.split("@").pop() ?? "").toLowerCase();
  return host === "" ? null : host;
}

/**
 * The client's duplicate key (NEWPIDASH-413): a trimmed URL with a
 * lowercased host, one trailing `.git` and trailing slashes stripped,
 * and — for GitHub only, like the server — a lowercased path. Null for
 * anything unparseable; the server stays authoritative there.
 */
export function createRepoIdentityKey(url: string): string | null {
  const candidate = url.trim();
  if (candidate === "" || hasBadCreateUrlChar(candidate)) return null;
  const host = createRepoUrlHost(candidate);
  if (host === null) return null;
  const schemeAt = candidate.indexOf("://");
  const rawPath =
    schemeAt !== -1
      ? candidate.slice(candidate.indexOf("/", schemeAt + 3) + 1 || candidate.length)
      : candidate.slice(candidate.indexOf(":") + 1);
  const path = rawPath.split("?")[0]?.split("#")[0] ?? "";
  const clean = path.replace(/^\/+|\/+$/g, "").replace(/\.git$/, "");
  if (clean === "" || clean.includes(":") || clean.split("/").some((part) => part === "")) return null;
  const keyed = host === "github.com" ? clean.toLowerCase() : clean;
  return `${host}/${keyed}`;
}

/**
 * Client-side row rules (NEWPIDASH-413): URL required, capped and shaped;
 * provider known; branch capped with the server's charset; no two rows
 * share an identity key.
 */
export function validateCreateRepoRows(rows: CreateRepoDraft[]): CreateRepoRowErrors[] {
  const keys = rows.map((row) => createRepoIdentityKey(row.url));
  return rows.map((row, index) => {
    const errors: CreateRepoRowErrors = {};
    if (row.url.trim() === "") {
      errors.url = CREATE_REPO_URL_REQUIRED_MESSAGE;
    } else if (row.url.length > CREATE_REPO_URL_MAX_LENGTH) {
      errors.url = CREATE_REPO_URL_TOO_LONG_MESSAGE;
    } else if (createRepoIdentityKey(row.url) === null) {
      errors.url = CREATE_REPO_URL_INVALID_MESSAGE;
    }
    if (!(CREATE_REPO_PROVIDERS as readonly string[]).includes(row.provider)) {
      errors.provider = CREATE_REPO_PROVIDER_MESSAGE;
    }
    if (row.baseBranch.length > CREATE_BASE_BRANCH_MAX_LENGTH) {
      errors.baseBranch = CREATE_REPO_BRANCH_TOO_LONG_MESSAGE;
    } else if (!BASE_BRANCH_PATTERN.test(row.baseBranch)) {
      errors.baseBranch = "Base branch may only contain letters, numbers, and . _ / -.";
    }
    // Like the server, only the later entry is the duplicate.
    if (
      errors.url === undefined &&
      keys[index] !== null &&
      keys.some((key, other) => other < index && key !== null && key === keys[index])
    ) {
      errors.url = CREATE_REPO_DUPLICATE_MESSAGE;
    }
    return errors;
  });
}

/**
 * Map a failed create POST onto the offending fields. Reused names or
 * codes surface the targeted taken messages; a refused repos list maps
 * each `entry <n>` message onto its row and anything unindexed onto the
 * list. Anything else leaves every field clear so the caller falls back
 * to a generic notice.
 */
export function selectCreateFieldErrors(error: unknown): CreateFieldErrors {
  if (!isApiError(error) || error.fields === undefined) return {};
  const errors: CreateFieldErrors = {};
  if (error.fields["name"]?.includes(NAME_TAKEN_CODE)) errors.name = NAME_TAKEN_MESSAGE;
  if (error.fields["identifier"]?.includes(IDENTIFIER_TAKEN_CODE)) errors.identifier = IDENTIFIER_TAKEN_MESSAGE;
  const settings = error.fields["settings"] ?? [];
  if (settings.length > 0) {
    const rows: CreateRepoRowErrors[] = [];
    const general: string[] = [];
    for (const message of settings) {
      const match = /entry (\d+)\b/.exec(message);
      if (match === null || match.index === undefined) {
        general.push(message);
        continue;
      }
      const index = Number(match[1]);
      const detail = message.slice(match.index + match[0].length).trim();
      const text = detail === "" ? message : detail;
      rows[index] = { ...rows[index], row: rows[index]?.row === undefined ? text : `${rows[index]?.row} ${text}` };
    }
    if (rows.some((row) => row !== undefined && Object.keys(row).length > 0)) errors.repos = rows;
    if (general.length > 0) errors.reposError = general.join(" ");
  }
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
 * One repository entry as the create POST carries it: a fresh id, the
 * trimmed URL, and a null branch for the repo's own default.
 */
export function buildCreateReposPayload(
  rows: CreateRepoDraft[]
): NonNullable<NonNullable<ProjectCreateInput["settings"]>["repos"]> {
  return rows.map((row) => ({
    id: globalThis.crypto.randomUUID(),
    url: row.url.trim(),
    provider: row.provider,
    base_branch: row.baseBranch === "" ? null : row.baseBranch,
  }));
}

/**
 * The single-POST body (SHELL-040, SHELL-041): identity, visibility,
 * lead and icon plus the work-items-only view flags. Repositories ride
 * the validated settings bag (NEWPIDASH-413); the legacy columns stay
 * empty. No cover keys — nothing here uploads, confirms or patches a
 * cover (SHELL-042).
 */
export function buildCreatePayload(values: CreatePayloadValues): ProjectCreateInput {
  const repos = values.repos ?? [];
  return {
    name: values.name.trim(),
    identifier: values.identifier.toUpperCase(),
    description: values.description,
    ...(repos.length === 0 ? {} : { settings: { repos: buildCreateReposPayload(repos) } }),
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
