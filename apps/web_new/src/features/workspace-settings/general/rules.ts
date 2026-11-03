// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure rules for the workspace-settings general page
// (parity/specs/workspace-settings.md §3–§6, WSET-003–006). Everything
// here is render-free so the form, the dialogs and their tests share one
// answer: the shared workspace-name rule, the fixed size bands, the
// merged timezone choices, the logo file limits, the delete confirmation
// gate, the stored-logo asset shapes and the post-delete landing target.

/** Workspace names hold at most eighty characters. */
export const WORKSPACE_NAME_MAX_LENGTH = 80;

/** Characters the shared workspace-name rule refuses (injection risk). */
const FORBIDDEN_NAME_CHARS = /[<>'"{}[\]*^!#%]/;

/** Name charset: letters, numbers, whitespace, hyphens and underscores. */
const NAME_CHARS = /^[\p{L}\p{N}\s_-]+$/u;

/**
 * The shared workspace-name rule as the general form states it. Null when
 * the name may be saved; otherwise the inline message. The required and
 * special-character messages are the oracle's pinned wording; the length
 * and charset messages only need to explain the rule.
 */
export function validateWorkspaceDetailsName(name: string): string | null {
  if (name.trim() === "") return "Workspace name is required";
  if (name.length > WORKSPACE_NAME_MAX_LENGTH) {
    return `Workspace name must be ${WORKSPACE_NAME_MAX_LENGTH} characters or less`;
  }
  if (FORBIDDEN_NAME_CHARS.test(name)) {
    return "Workspace name cannot contain special characters like < > ' \" { } [ ] * ^ ! # %";
  }
  if (!NAME_CHARS.test(name))
    return "Workspace name can only contain letters, numbers, spaces, hyphens and underscores";
  return null;
}

/** Organization-size presets, in display order; the value doubles as the label. */
export const ORGANIZATION_SIZE_BANDS: readonly string[] = ["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"];

/** How the size control reads before any band is stored. */
export const ORGANIZATION_SIZE_PLACEHOLDER = "Select organization size";

/** One searchable timezone row. */
export interface TimezoneChoice {
  /** The stored zone identifier (for example Pacific/Auckland). */
  id: string;
  /** The merged city label (for example "Auckland, Wellington"). */
  label: string;
  /** Extra search text: identifier, labels and both offsets. */
  searchText: string;
}

/** Minimal shape of one GET /api/timezones/ row. */
export interface TimezoneRow {
  value: string;
  label: string;
  utc_offset: string;
  gmt_offset: string;
}

/**
 * Build the picker's choices: entries sharing a zone identifier merge into
 * one row whose label joins the city names in server order, and a UTC
 * choice leads the list. Search covers the identifier, the merged label
 * and both offsets (§3).
 */
export function mergeTimezoneOptions(rows: readonly TimezoneRow[]): TimezoneChoice[] {
  const merged = new Map<string, TimezoneChoice>();
  for (const row of rows) {
    const existing = merged.get(row.value);
    if (existing === undefined) {
      merged.set(row.value, {
        id: row.value,
        label: row.label,
        searchText: `${row.value} ${row.label}, ${row.gmt_offset}, ${row.utc_offset}`,
      });
    } else {
      const label = `${existing.label}, ${row.label}`;
      merged.set(row.value, {
        id: row.value,
        label,
        searchText: `${row.value} ${label}, ${row.gmt_offset}, ${row.utc_offset}`,
      });
    }
  }
  return [{ id: "UTC", label: "UTC", searchText: "utc, coordinated universal time" }, ...merged.values()];
}

/** The label the picker shows for the stored zone, or the placeholder. */
export function timezoneChoiceLabel(
  choices: readonly TimezoneChoice[],
  value: string | null,
  placeholder = "Select a timezone"
): string {
  if (value === null || value === "") return placeholder;
  return choices.find((choice) => choice.id === value)?.label ?? value;
}

/** Images the logo dialog accepts. */
export const LOGO_ACCEPTED_TYPES: readonly string[] = ["image/jpeg", "image/png", "image/webp"];

/** The attribute for the file input; the browser filters the picker. */
export const LOGO_ACCEPT_ATTRIBUTE = LOGO_ACCEPTED_TYPES.join(",");

/** Largest accepted logo, in bytes. */
export const LOGO_MAX_BYTES = 5 * 1024 * 1024;

export const LOGO_TOO_LARGE_MESSAGE = "The image must be 5 MB or smaller.";
export const LOGO_WRONG_TYPE_MESSAGE = "Choose a JPEG, PNG or WebP image.";

/**
 * Check one picked or dropped file against the dialog's limits. Null when
 * it may be uploaded; otherwise the inline message.
 */
export function validateLogoFile(file: { type: string; size: number }): string | null {
  if (!LOGO_ACCEPTED_TYPES.includes(file.type)) return LOGO_WRONG_TYPE_MESSAGE;
  if (file.size > LOGO_MAX_BYTES) return LOGO_TOO_LARGE_MESSAGE;
  return null;
}

/** The initial the avatar block shows without a logo. */
export function workspaceInitial(name: string): string {
  const first = Array.from(name.trim())[0];
  return first === undefined ? "W" : first.toUpperCase();
}

/** The full workspace address the copy control writes and the URL field echoes. */
export function workspaceAddress(origin: string, workspaceSlug: string): string {
  return `${origin.replace(/\/+$/, "")}/${workspaceSlug}`;
}

/** Whether a stored logo value is a legacy full URL rather than an asset path. */
export function isLegacyLogoValue(value: string | null | undefined): boolean {
  return value !== null && value !== undefined && /^https?:\/\//i.test(value);
}

/** The trailing path segment of a stored asset URL, ignoring a trailing slash. */
export function assetKeyFromUrl(value: string): string {
  const trimmed = value.replace(/\/+$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1);
}

/** The fixed phrase the delete dialog requires beside the workspace name. */
export const DELETE_CONFIRM_PHRASE = "delete my workspace";

/** Confirm stays disabled until both inputs match exactly (§6). */
export function canConfirmWorkspaceDelete(input: {
  workspaceName: string;
  typedName: string;
  typedPhrase: string;
}): boolean {
  return input.typedName === input.workspaceName && input.typedPhrase === DELETE_CONFIRM_PHRASE;
}

/** Where a successful delete lands: a remaining workspace home, else creation. */
export function postDeletePath(input: {
  remainingSlugs: readonly string[];
  lastWorkspaceSlug: string | null;
  fallbackWorkspaceSlug: string | null;
  creationPath: string;
}): string {
  const candidates = [input.lastWorkspaceSlug, input.fallbackWorkspaceSlug];
  const surviving = candidates.find((slug) => slug !== null && input.remainingSlugs.includes(slug));
  // The router preserves the trailing slash an address carries;
  // the workspace home the oracle polls for is the slashed form.
  return surviving === undefined ? input.creationPath : `/${surviving}/`;
}
