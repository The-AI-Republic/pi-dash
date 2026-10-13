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
  /** Path prefix the subtree mounts under, e.g. "/billing". */
  path: string;
}

/** Extra sidebar entry contributed by an edition. */
export interface SidebarItem {
  id: string;
  label: string;
  to: string;
}

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
}

export interface Edition {
  id: "oss" | string;
  routes?: RouteExtension[];
  sidebar?: SidebarItem[];
  settingsSections?: SettingsSection[];
  auth?: AuthProviderExtension[];
  /** Request middleware, e.g. token refresh. Runs inside the api client. */
  api?: Middleware[];
  flags: Record<string, boolean>;
  slots?: Partial<SlotComponents>;
}
