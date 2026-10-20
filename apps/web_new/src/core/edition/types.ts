// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition contract (Platform and editions page). The cloud build provides
// its own module through the single @pidash/edition alias; everything else
// programs against this interface.

import type { Middleware } from "@pidash/api-client";
import type { ComponentType } from "react";

/** Extra route subtree contributed by an edition. */
export interface RouteExtension {
  /** Route segment the edition declares, matched exactly (workspace routes use the bare segment, e.g. "apps"). */
  path: string;
}

/** Extra sidebar entry contributed by an edition. */
export interface SidebarItem {
  id: string;
  label: string;
  to: string;
}

/**
 * Well-known edition flags read by the shell (SHELL-063, SHELL-066, from
 * parity/specs/shell.md §5). Flags stay opt-in per key: an absent key
 * means the community default, never an error.
 */
export const APP_RAIL_FLAG = "app-rail";
export const SIDEBAR_TOGGLE_FLAG = "sidebar-toggle";
/**
 * Cloud de-dupe (ISS-121, ISS-231; parity/specs/issues.md §23). The
 * create modal's similar-issue search mounts only where a build sets
 * this; the community edition leaves it absent and renders nothing.
 */
export const ISSUE_DEDUPE_FLAG = "issue-dedupe";
/**
 * Workspace plan badge (CLOUD-038, parity/specs/cloud.md §8). Editions
 * whose server publishes a plan snapshot and an external account center
 * set this; the community edition leaves it absent because its billing
 * surface is the in-app upgrade entry, not an external account.
 */
export const PLAN_BADGE_FLAG = "plan-badge";
/**
 * Cloud API path (CLOUD-040, parity/specs/cloud.md §8). Editions whose web
 * server proxies only `/api/*` set this: the desktop agent then reads its
 * CSRF token from `/api/auth/get-csrf-token/` — the same proxying reason
 * cloud sign-out uses — instead of the client's default `/auth/` path.
 * The community edition leaves it absent and keeps the default path.
 */
export const CLOUD_CSRF_FLAG = "cloud-csrf";

/** Extra settings section contributed by an edition. */
export interface SettingsSection {
  id: string;
  label: string;
}

/** Extra sign-in method contributed by an edition. */
export interface AuthProviderExtension {
  id: string;
  label: string;
}

/** Named UI slots an edition may fill. Kept explicit: adding a slot is a
 * design decision recorded in the PR and on the Decisions page. */
export interface SlotComponents {
  issueDetailSidebarAfter?: () => null;
  /**
   * Desktop sign-in card (AUTH-022/AUTH-023, NEWFRONT-486). Editions whose
   * server implements the desktop hand-off fill this with their
   * browser-opening card; the OSS edition leaves it absent and the desktop
   * entry renders the unavailable card instead. The edition card is
   * recognized by the "Sign in to Pi Dash" heading; the community card
   * deliberately avoids that wording so a misbuilt bundle fails its build
   * check instead of passing silently.
   */
  desktopSignInCard?: ComponentType;
  /**
   * Cloud god-mode entry (ADM-074-079, NEWFRONT-519; Decisions D19).
   * Editions whose server authenticates instance admins over OIDC fill
   * this with the authorization resolver (the admin feature's
   * CloudAdminEntry: spinner, OIDC bounce, terminal denied, or retryable
   * error — never a password or setup form). The OSS edition leaves it
   * absent and the god-mode entry renders the bootstrap gate instead.
   * The name is stable across editions; a rename is a breaking change.
   */
  godModeCloudEntry?: ComponentType;
}

export interface Edition {
  id: "oss" | string;
  routes?: RouteExtension[];
  sidebar?: SidebarItem[];
  /** Far-left rail dock entries; the rail mounts only with APP_RAIL_FLAG. */
  rail?: SidebarItem[];
  settingsSections?: SettingsSection[];
  auth?: AuthProviderExtension[];
  /** Request middleware, e.g. token refresh. Runs inside the api client. */
  api?: Middleware[];
  flags: Record<string, boolean>;
  slots?: Partial<SlotComponents>;
}
