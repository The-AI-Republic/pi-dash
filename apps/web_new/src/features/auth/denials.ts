// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Authorization-failure deciders (AUTH-024, from parity/specs/auth.md).
// Project screens key off the details-fetch status plus admin standing;
// workspace screens split on whether the slug is known in the workspace
// list. Pure, so routes, gates, and tests share them.

import { isApiError } from "@pidash/api-client";

/** Project denial screens: join where allowed, explain otherwise, else not-found. */
export type ProjectDenial = "join" | "no-access" | "not-found";

/**
 * Which project screen a fetch outcome renders. Non-members (409) and
 * admins facing a private project (403 plus admin standing) may join;
 * anyone else refused (403) sees the no-access screen; unknown or
 * otherwise failed projects (404 and anything else) report not-found.
 */
export function decideProjectDenial(args: { status: number | undefined; isAdmin: boolean }): ProjectDenial {
  if (args.status === 409 || (args.status === 403 && args.isAdmin)) return "join";
  if (args.status === 403) return "no-access";
  return "not-found";
}

/** Workspace denial screens: unknown slug (Case A) vs known slug without membership (Case B). */
export type WorkspaceDenial = "not-found" | "not-member";

/**
 * Which workspace screen a gate outcome renders, or null when the visitor
 * belongs: an unknown slug reports not-found, a known slug without
 * membership info renders the not-a-member screen.
 */
export function decideWorkspaceDenial(args: { known: boolean; membershipPresent: boolean }): WorkspaceDenial | null {
  if (!args.known) return "not-found";
  if (!args.membershipPresent) return "not-member";
  return null;
}

/**
 * True for fetch failures the project gate owns (403/404/409): project
 * routes let these through to the gate instead of throwing, so the denial
 * screen renders rather than the load error.
 */
export function isProjectGateError(error: unknown): boolean {
  if (!isApiError(error)) return false;
  return error.status === 403 || error.status === 404 || error.status === 409;
}
