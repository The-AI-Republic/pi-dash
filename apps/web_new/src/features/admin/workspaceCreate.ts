// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-workspace helpers (ADM-043–045, from
// parity/specs/admin.md §5.7–§5.9): slug derivation, field validation
// over the fixed size bands and the restricted-URL list, the
// parity-pinned notice and submit copy, and the probe/taken error
// signals the save mutation raises. Pure for tests; hooks.ts wires
// them to the slug-check probe and the create POST.
import type { InstanceWorkspaceCreateInput } from "@pidash/api-client";

/** Submit label at rest (parity-pinned). */
export const WORKSPACE_CREATE_SUBMIT_LABEL = "Create workspace";

/** Submit label while the probe plus create are in flight (parity-pinned). */
export const WORKSPACE_CREATE_BUSY_LABEL = "Creating workspace";

/** Success notice once the create lands (parity-pinned). */
export const WORKSPACE_CREATE_SUCCESS_TITLE = "Success!";
export const WORKSPACE_CREATE_SUCCESS_MESSAGE = "Workspace created successfully.";

/** Failure notice title for probe and create failures (parity-pinned). */
export const WORKSPACE_CREATE_FAILURE_TITLE = "Error!";

/** Failure notice when the slug probe itself fails; nothing was created. */
export const WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE =
  "The workspace address could not be checked. Nothing was created — try again.";

/** Failure notice when the create POST fails after a clear probe. */
export const WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE = "The workspace could not be created. Try again.";

/** Inline message when the slug is taken or restricted; no create runs. */
export const WORKSPACE_CREATE_TAKEN_MESSAGE = "That address is taken. Pick another one.";

/** Inline hint when the slug carries characters outside the URL set. */
export const WORKSPACE_CREATE_INVALID_SLUG_HINT = "Slugs use letters, numbers, hyphens and underscores only.";

/** Longest workspace name the form accepts. */
export const WORKSPACE_NAME_MAX_LENGTH = 80;

/** Longest workspace slug the form accepts. */
export const WORKSPACE_SLUG_MAX_LENGTH = 48;

/**
 * Fixed organization-size bands (values double as labels). The create
 * POST carries the picked band as `organization_size`.
 */
export const ORGANIZATION_SIZE_BANDS: readonly string[] = ["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"];

/**
 * Slugs the console refuses without a create, even when the server
 * probe reports them free: top-level app paths and marketing words
 * that would shadow real routes. Membership is exact; callers pass
 * the forced-lowercase slug.
 */
const RESTRICTED_WORKSPACE_SLUGS: ReadonlySet<string> = new Set([
  "404",
  "accounts",
  "admin",
  "api",
  "billing",
  "business",
  "calendar",
  "channels",
  "chat",
  "config",
  "configuration",
  "create-workspace",
  "dashboard",
  "desktop",
  "disco",
  "drive",
  "enterprise",
  "epic",
  "epics",
  "flags",
  "god-mode",
  "import",
  "importers",
  "ingest",
  "initiative",
  "initiatives",
  "installations",
  "instance",
  "instances",
  "integration",
  "integrations",
  "invitations",
  "license",
  "licenses",
  "live",
  "m",
  "mobile",
  "monitor",
  "monitoring",
  "one",
  "onboarding",
  "onload",
  "pages",
  "password",
  "pi-dash-enterprise",
  "pi-dash-pro",
  "pi-dash-ultimate",
  "pro",
  "profile",
  "real-time",
  "settings",
  "sign-in",
  "sign-up",
  "signin",
  "signup",
  "silo",
  "spaces",
  "story",
  "upgrade",
  "workflow",
  "workflows",
  "workspace-invitations",
]);

/** Whether the slug sits on the restricted-URL list (§5.8). */
export function isRestrictedWorkspaceSlug(slug: string): boolean {
  return RESTRICTED_WORKSPACE_SLUGS.has(slug);
}

/**
 * Derive the slug from the name (§5.7): lowercased, trimmed, spaces to
 * hyphens. Runs on every name keystroke, so returning to the name
 * field re-derives over an independent slug edit.
 */
export function deriveWorkspaceSlug(name: string): string {
  return name.toLowerCase().trim().replace(/ /g, "-");
}

/** Raw keystroke set the slug hint enforces: ASCII alphanumerics, hyphen, underscore. */
const SLUG_KEYSTROKE_PATTERN = /^[a-zA-Z0-9_-]+$/;

/**
 * Whether the charset hint shows: a non-empty slug outside the URL
 * keystroke set. Emptiness reports through the required message
 * instead, so the field never shows both at once.
 */
export function shouldShowInvalidSlugHint(slug: string): boolean {
  return slug !== "" && !SLUG_KEYSTROKE_PATTERN.test(slug);
}

/** Characters rejected outright from names and slugs. */
const INJECTION_PATTERN = /[<>'"{}[\]*^!#%]/;

/** Name text: unicode letters and numbers plus spaces, hyphens, underscores. */
const WORKSPACE_NAME_PATTERN = /^[\p{L}\p{N}\s_-]+$/u;

/** Slug text: unicode letters and numbers plus hyphens and underscores. */
const WORKSPACE_SLUG_PATTERN = /^[\p{L}\p{N}_-]+$/u;

/**
 * Name failure message, or null when the name passes (§5.7): required,
 * capped in length, then charset. The injection and charset messages
 * name the field, which the parity scenario asserts.
 */
export function validateWorkspaceCreateName(name: string): string | null {
  if (name.trim() === "") return "Enter a name for the workspace.";
  if (name.length > WORKSPACE_NAME_MAX_LENGTH) {
    return `Shorten the workspace name to ${WORKSPACE_NAME_MAX_LENGTH} characters or fewer.`;
  }
  if (INJECTION_PATTERN.test(name)) return "Workspace name cannot use < > ' \" { } [ ] * ^ ! # %.";
  if (!WORKSPACE_NAME_PATTERN.test(name)) {
    return "Workspace name allows letters, numbers, spaces, hyphens and underscores.";
  }
  return null;
}

/**
 * Slug failure message, or null when the slug passes (§5.7): required,
 * capped in length, then charset.
 */
export function validateWorkspaceCreateSlug(slug: string): string | null {
  if (slug.trim() === "") return "Enter a URL slug for the workspace.";
  if (slug.length > WORKSPACE_SLUG_MAX_LENGTH) {
    return `Shorten the slug to ${WORKSPACE_SLUG_MAX_LENGTH} characters or fewer.`;
  }
  if (INJECTION_PATTERN.test(slug)) return "Slug cannot use < > ' \" { } [ ] * ^ ! # %.";
  if (!WORKSPACE_SLUG_PATTERN.test(slug)) {
    return "Slug allows letters, numbers, hyphens and underscores.";
  }
  return null;
}

/** Inline message while no size band is picked (§5.7). */
export const WORKSPACE_CREATE_SIZE_REQUIRED_MESSAGE = "Choose an organization size.";

/**
 * Whether submit opens: the name and slug validate and a size band is
 * picked. A taken probe result never disables submit — submitting is
 * what runs the probe.
 */
export function isWorkspaceCreateSubmittable(name: string, slug: string, organizationSize: string): boolean {
  return (
    validateWorkspaceCreateName(name) === null &&
    validateWorkspaceCreateSlug(slug) === null &&
    ORGANIZATION_SIZE_BANDS.includes(organizationSize)
  );
}

/**
 * The create POST body: name, slug, and size. No owner field —
 * ownership is assigned server-side to the creating admin (§5.9).
 */
export function buildWorkspaceCreateInput(
  name: string,
  slug: string,
  organizationSize: string
): InstanceWorkspaceCreateInput {
  return { name, slug, organization_size: organizationSize };
}

/** The slug prefix facing the slug field: the web origin plus one slash (§5.7). */
export function formatWorkspaceSlugPrefix(origin: string): string {
  return `${origin.replace(/\/+$/, "")}/`;
}

/** The probe answered unavailable, or the slug is restricted: show the inline taken error. */
export class WorkspaceSlugTakenError extends Error {
  constructor(slug: string) {
    super(`Workspace slug is not available: ${slug}`);
    this.name = "WorkspaceSlugTakenError";
  }
}

/** The probe request itself failed: toast the error, create nothing. */
export class WorkspaceSlugProbeError extends Error {
  constructor(slug: string) {
    super(`Workspace slug probe failed: ${slug}`);
    this.name = "WorkspaceSlugProbeError";
  }
}
