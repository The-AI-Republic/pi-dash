// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth page guards and post-auth landing (AUTH-015, AUTH-016). Written
// from parity/specs/auth.md; later slices mount their routes on these
// deciders rather than forking a second guard framework. Pure, so routes
// and tests share them; route beforeLoad blocks only adapt decisions to
// redirects.

import type { QueryClient } from "@tanstack/react-query";
import { isApiError, type ApiClient, type Me, type Profile, type WorkspaceList } from "@pidash/api-client";

import {
  meQueryOptions,
  profileQueryOptions,
  selectWorkspace,
  settingsQueryOptions,
  workspacesQueryOptions,
} from "./queries.js";

/** Non-empty strings only; everything else is an absent param. */
export function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

/** Internal return paths only; anything else falls back to post-auth landing. */
export function safeNextPath(value: unknown): string | undefined {
  const path = asString(value);
  if (!path || !path.startsWith("/") || path.startsWith("//")) return undefined;
  return path;
}

export interface ParsedReturnPath {
  next?: string | undefined;
  unsafeNext?: string | undefined;
}

/**
 * The return path off a card URL. Reads both the `next` spelling and the
 * `next_path` spelling guards emit and old links carry (AUTH-015/016): a
 * safe value from either spelling becomes `next`, while a present but
 * unsafe value becomes `unsafeNext` — POST-only forwarding the server
 * sanitizes, never navigated to client-side.
 */
export function parseReturnPath(search: Record<string, unknown>): ParsedReturnPath {
  const next = safeNextPath(search["next"]) ?? safeNextPath(search["next_path"]);
  if (next !== undefined) return { next };
  const raw = asString(search["next"]) ?? asString(search["next_path"]);
  if (raw === undefined) return {};
  return { unsafeNext: raw };
}

/**
 * The five page types (AUTH-015). Public pages render for everyone;
 * signed-out-only pages (sign-in entry, sign-up, recovery) bounce members
 * away; the onboarding funnel renders only for unfinished members;
 * set-password renders for visitors and unfinished members and bounces
 * password-holding members away; signed-in-only pages (the default) bounce
 * visitors to sign-in and unfinished members to onboarding.
 */
export type AuthPageKind = "public" | "signed-out-only" | "onboarding-funnel" | "set-password" | "signed-in-only";

export type PageGateDecision =
  | { action: "render" }
  | { action: "sign-in"; nextPath: string }
  | { action: "onboarding" }
  | { action: "landing" };

/**
 * The progress flags the finish stamp writes (AUTH-039). When every one
 * is set, the profile counts as onboarded even when the onboarded marker
 * itself never landed — the crash window between the two finish writes.
 */
const ONBOARDING_DONE_FLAGS = ["profile_complete", "workspace_create", "workspace_invite", "workspace_join"] as const;

/**
 * Whether a profile counts as onboarded (shared definition): the
 * onboarded marker, or every progress flag set. An unreadable profile
 * counts as unfinished — callers fail closed toward onboarding, never
 * into the app. Every guard reads onboarded-ness through this helper.
 */
export function isOnboardedProfile(
  profile: { is_onboarded: boolean; onboarding_step?: Record<string, unknown> | null | undefined } | null | undefined
): boolean {
  if (!profile) return false;
  if (profile.is_onboarded) return true;
  const steps = profile.onboarding_step ?? {};
  return ONBOARDING_DONE_FLAGS.every((flag) => Boolean(steps[flag]));
}

/**
 * Gate one page type (AUTH-015). `onboarded` reads
 * isOnboardedProfile(profile) and `hasPassword` reads
 * me.is_password_autoset === false; callers pass the values, this
 * decides. A sign-in decision carries the return path the bounce must
 * preserve.
 */
export function decidePageGate(args: {
  page: AuthPageKind;
  signedIn: boolean;
  onboarded: boolean;
  hasPassword: boolean;
  currentPath: string;
}): PageGateDecision {
  if (args.page === "public") return { action: "render" };
  if (!args.signedIn) {
    if (args.page === "signed-out-only") return { action: "render" };
    return { action: "sign-in", nextPath: args.currentPath };
  }
  if (args.page === "signed-out-only") {
    return args.onboarded ? { action: "landing" } : { action: "onboarding" };
  }
  if (args.page === "onboarding-funnel") {
    return args.onboarded ? { action: "landing" } : { action: "render" };
  }
  if (args.page === "set-password") {
    return args.hasPassword && args.onboarded ? { action: "landing" } : { action: "render" };
  }
  // The remaining kind is signed-in-only.
  return args.onboarded ? { action: "render" } : { action: "onboarding" };
}

/** Where the entry route sends a signed-out visitor who just signed in. */
export const ONBOARDING_PATH = "/onboarding";

/** Where users with nowhere to go land (AUTH-016). */
export const CREATE_WORKSPACE_PATH = "/create-workspace";

/**
 * Post-authentication landing (AUTH-016): a safe return path first, else
 * the last or fallback workspace still available to the user, else
 * workspace creation. The return path is filtered here, so callers pass
 * the raw value and never pre-validate.
 */
export function resolveLanding(args: {
  nextPath?: string | undefined;
  lastSlug: string | null;
  fallbackSlug: string | null;
  workspaces: WorkspaceList | undefined;
}): string {
  const safe = safeNextPath(args.nextPath);
  if (safe !== undefined) return safe;
  if (args.lastSlug !== null && selectWorkspace(args.workspaces, args.lastSlug) !== undefined) {
    return `/${args.lastSlug}`;
  }
  if (args.fallbackSlug !== null && selectWorkspace(args.workspaces, args.fallbackSlug) !== undefined) {
    return `/${args.fallbackSlug}`;
  }
  return CREATE_WORKSPACE_PATH;
}

/**
 * Whether a gate read failed with a definitive session rejection (a
 * 401/403 response). Anything else — transport, timeout, server error —
 * says nothing about the session (CLOUD-015, from parity/specs/cloud.md
 * §4).
 */
export function isSessionRejection(error: unknown): boolean {
  return isApiError(error) && (error.status === 401 || error.status === 403);
}

/**
 * Resolve a failed gate read: a rejected session reads as signed-out (the
 * guards bounce with the location preserved); a transient failure is
 * re-raised so the caller keeps its cached user instead of recording the
 * failure as signed-out.
 */
function nullOnRejection<T>(promise: Promise<T>): Promise<T | null> {
  return promise.catch((error: unknown) => {
    if (isSessionRejection(error)) return null;
    throw error;
  });
}

/**
 * Gate inputs for a route beforeLoad: the signed-in user and their
 * profile, each null when the read is rejected. Without a session there
 * is no profile to read. Callers treat a null profile as unfinished (fail
 * closed toward onboarding, never into the app).
 */
export async function loadGateInputs(
  queryClient: QueryClient,
  apiClient: ApiClient
): Promise<{ me: Me | null; profile: Profile | null }> {
  const me = await nullOnRejection(queryClient.ensureQueryData(meQueryOptions(apiClient)));
  if (me === null) return { me, profile: null };
  const profile = await nullOnRejection(queryClient.ensureQueryData(profileQueryOptions(apiClient)));
  return { me, profile };
}

/**
 * Landing target for a route beforeLoad: a safe return path needs no
 * reads; otherwise settings plus the workspace list (each defaulting to
 * empty when the read fails) resolve per AUTH-016.
 */
export async function loadLandingTarget(
  queryClient: QueryClient,
  apiClient: ApiClient,
  nextPath?: string | undefined
): Promise<string> {
  const safe = safeNextPath(nextPath);
  if (safe !== undefined) return safe;
  const settings = await queryClient.ensureQueryData(settingsQueryOptions(apiClient)).catch(() => null);
  const workspaces = await queryClient.ensureQueryData(workspacesQueryOptions(apiClient)).catch(() => undefined);
  return resolveLanding({
    nextPath,
    lastSlug: settings?.workspace.last_workspace_slug ?? null,
    fallbackSlug: settings?.workspace.fallback_workspace_slug ?? null,
    workspaces,
  });
}
