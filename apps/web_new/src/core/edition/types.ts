// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Edition contract (Platform and editions page). The cloud build provides
// its own module through the single @pidash/edition alias; everything else
// programs against this interface.

import type { Middleware } from "@pidash/api-client";

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
