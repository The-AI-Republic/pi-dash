// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace onboarding logic (AUTH-034–041). Pure helpers for the
// create-or-join step and the invite-members step: the workspace
// name/slug validators, slug derivation, the reserved-slug list, team
// sizes, the default sub-view resolution, the pending-request lookup,
// the invite-role labels, and the admin-request mail draft for the
// creation-disabled screen. Written from parity/specs/auth.md.

import type { WorkspaceJoinRequestList } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/index.js";

// --- workspace name and slug checks (AUTH-035) -------------------------------

/** Workspace names hold at most eighty characters. */
export const WORKSPACE_NAME_MAX_LENGTH = 80;

/** Workspace slugs hold at most forty-eight characters. */
export const WORKSPACE_SLUG_MAX_LENGTH = 48;

/** Characters that can never appear in a workspace name or slug (injection-risk set). */
const WORKSPACE_FORBIDDEN_CHARS = /[<>'"{}[\]*^!#%]/;

/** Name charset: unicode letters and numbers, whitespace, hyphens, underscores. */
const WORKSPACE_NAME_CHARS = /^[\p{L}\p{N}\s_-]+$/u;

/** Slug charset: unicode letters and numbers, hyphens, underscores. No spaces. */
const WORKSPACE_SLUG_CHARS = /^[\p{L}\p{N}_-]+$/u;

/**
 * Workspace-name check (AUTH-035): required, length-checked, no
 * injection-risk characters, name charset. Null when valid.
 */
export function validateWorkspaceName(name: string): string | null {
  if (name.trim() === "") return "Enter a workspace name.";
  if (name.length > WORKSPACE_NAME_MAX_LENGTH)
    return `Keep the workspace name within ${WORKSPACE_NAME_MAX_LENGTH} characters.`;
  if (WORKSPACE_FORBIDDEN_CHARS.test(name))
    return "Workspace names cannot contain symbols like < > ' \" { } [ ] * ^ ! # %.";
  if (!WORKSPACE_NAME_CHARS.test(name)) return "Use only letters, numbers, spaces, hyphens, and underscores.";
  return null;
}

/** Malformed-slug message. The oracle pins the "URLs can contain only" prefix. */
export const SLUG_FORMAT_MESSAGE = "URLs can contain only letters, numbers, hyphens, and underscores.";

/** Taken/reserved-slug message. The oracle pins the "already taken" phrase. */
export const SLUG_TAKEN_MESSAGE = "This workspace URL is already taken. Try another.";

/**
 * Workspace-slug format check (AUTH-035): required, length-checked, and
 * slug charset (which also rejects the injection-risk set). Null when
 * valid. Availability and the reserved list are enforced separately at
 * submit time, since they need the server answer.
 */
export function validateWorkspaceSlug(slug: string): string | null {
  if (slug.trim() === "") return "Enter a workspace URL.";
  if (slug.length > WORKSPACE_SLUG_MAX_LENGTH)
    return `Keep the workspace URL within ${WORKSPACE_SLUG_MAX_LENGTH} characters.`;
  if (!WORKSPACE_SLUG_CHARS.test(slug)) return SLUG_FORMAT_MESSAGE;
  return null;
}

/** The name as a URL slug: lowercased, trimmed, spaces turned to dashes. */
export function deriveWorkspaceSlug(name: string): string {
  return name.toLowerCase().trim().replace(/ /g, "-");
}

/**
 * Slugs the router owns: creating under one would shadow app routes, so
 * submit rejects them inline even when the server reports them free.
 * Transcribed from parity/specs/auth.md (AUTH-035).
 */
export const RESERVED_WORKSPACE_SLUGS: readonly string[] = [
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
  "onboarding",
  "one",
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
];

/** True when the slug collides with the router-owned list (case-insensitive). */
export function isReservedWorkspaceSlug(slug: string): boolean {
  return RESERVED_WORKSPACE_SLUGS.includes(slug.toLowerCase());
}

// --- team sizes (AUTH-034, AUTH-038, AUTH-040) --------------------------------
// The labels are the stored organization_size values, so they stay stable:
// the pills show them verbatim and the solo label drives the shortcut.

/** The six team sizes, in display order. */
export const WORKSPACE_TEAM_SIZES: readonly string[] = ["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"];

/** The size that finishes onboarding immediately after creation. */
export const SOLO_TEAM_SIZE = "Just myself";

/** True for the solo size that skips the invite step. */
export function isSoloTeamSize(size: string): boolean {
  return size === SOLO_TEAM_SIZE;
}

// --- workspace-step sub-views (AUTH-034, AUTH-036) ----------------------------

/** The four sub-views of the workspace create-or-join step. */
export type WorkspaceStepView = "create" | "invites" | "join-by-email" | "pending";

/**
 * The default sub-view (AUTH-034): a pending join request forces the
 * holding view; else outstanding invites default to the invites view;
 * else the create view. Join-by-email is only reached by navigation.
 */
export function resolveWorkspaceDefaultView(args: {
  hasPendingRequest: boolean;
  hasInvitations: boolean;
}): WorkspaceStepView {
  if (args.hasPendingRequest) return "pending";
  if (args.hasInvitations) return "invites";
  return "create";
}

/** The caller's outstanding join request, if any — they wait on the holding view. */
export function selectPendingJoinRequest(requests: WorkspaceJoinRequestList) {
  return requests.find((request) => request.status === "PENDING") ?? null;
}

/**
 * The address the holding view names: the just-requested address first (so
 * it shows before the join-request list refetches), else the outstanding
 * request's address, else empty for the generic message.
 */
export function resolvePendingEmail(requestedEmail: string | null, pending: { admin_email: string } | null): string {
  return requestedEmail ?? pending?.admin_email ?? "";
}

// --- invite-members step (AUTH-037) -------------------------------------------

/** Blank address-plus-role rows the step opens with. */
export const INVITE_INITIAL_ROW_COUNT = 3;

/** The roles one invite row can carry (backend workspace roles). */
export const WORKSPACE_INVITE_ROLES: readonly { value: number; label: string }[] = [
  { value: ROLE_ADMIN, label: "Admin" },
  { value: ROLE_MEMBER, label: "Member" },
  { value: ROLE_GUEST, label: "Guest" },
];

/** The role shown on invite cards and rows; unknown values read as Member. */
export function workspaceRoleLabel(role: number): string {
  switch (role) {
    case ROLE_ADMIN:
      return "Admin";
    case ROLE_GUEST:
      return "Guest";
    default:
      return "Member";
  }
}

// --- creation-disabled admin request (AUTH-041) ------------------------------

/** Subject of the prefilled mail asking the instance admin for a workspace. */
export const ADMIN_REQUEST_SUBJECT = "Requesting a new workspace";

/**
 * The ask-the-admin mail draft (AUTH-041): no recipient — the requester
 * addresses it to whoever administers their instance — with a prefilled
 * subject and a body template naming the workspace URL and purpose slots
 * plus the requester's own name and address as the signature.
 */
export function buildAdminRequestMailto(requester: { firstName: string; lastName: string; email: string }): string {
  const name = `${requester.firstName} ${requester.lastName}`.trim();
  const signature = name === "" ? requester.email : `${name}\n${requester.email}`;
  const body = `Hello,\n\nCould you set up a workspace for me? I would like the URL [/workspace-name] to use for [what the workspace is for].\n\nThank you,\n${signature}`;
  return `mailto:?subject=${encodeURIComponent(ADMIN_REQUEST_SUBJECT)}&body=${encodeURIComponent(body)}`;
}
