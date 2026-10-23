// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition contract (Platform and editions page). The cloud build provides
// its own module through the single @pidash/edition alias; everything else
// programs against this interface.

import type { Middleware } from "@pidash/api-client";
import type { ComponentType } from "react";

/**
 * Extra route subtree contributed by an edition. `scope` says where the
 * path lives: a `workspace` route is a segment under the active workspace
 * (`{ path: "apps" }` → `/<workspace>/apps/`), while a `top-level` route
 * is an address from the site root (`{ path: "home", scope: "top-level" }`
 * → `/home`). Absent scope means `workspace`, so existing modules keep
 * their meaning.
 */
export interface RouteExtension {
  /** Route segment or top-level address the edition declares, matched exactly. */
  path: string;
  /** Where the route lives; only `top-level` differs from the default. */
  scope?: "workspace" | "top-level";
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
 * Cloud estimate family capability (EST-009). The estimate creation dialog
 * offers the time family only when the active edition sets this flag;
 * community builds omit time entirely rather than rendering it disabled.
 */
export const ESTIMATE_TIME_FAMILY_FLAG = "estimate-time-family";

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
  /**
   * Cloud sign-in card (CLOUD-002/CLOUD-011-014, NEWFRONT-652; Decisions
   * D22). Editions whose server runs the organizational-account provider
   * flow fill this with the real sign-in card; `routes/login.tsx` then
   * serves `/login` in place instead of forwarding it to the root. The
   * OSS edition leaves it absent and keeps the retired-address redirect.
   * The name is stable across editions; a rename is a breaking change.
   */
  cloudLoginCard?: ComponentType;
  /**
   * Saved-view access field (VIEW-019, NEWFRONT-721; Decisions D23). The
   * create/edit dialog's shared-vs-owner-only selector plus its write.
   * `access` is serializer read-only on the view endpoints, so the choice
   * cannot ride the OSS create or patch body in any edition — the slot
   * owns its own call (`storeAccess`) against the edition's server. The
   * OSS edition leaves it absent: no selector renders and every created
   * view keeps the shared model default. The name is stable across
   * editions; a rename is a breaking change.
   */
  viewAccess?: ViewAccessSlot;
  /**
   * Saved-view publish seam (VIEW-020, NEWFRONT-722; Decisions D24). The
   * row menu's publish entry, the publish dialog and the Live marker
   * render only where an edition fills this slot. The slot owns the two
   * writes (the server sets or clears the record's `anchor`) and the
   * public read-only link derivation (the anchor plus the edition's
   * public base URL); the app owns the menu entry, the dialog chrome,
   * the cache adoption and the notices. The OSS edition leaves it
   * absent: no publish entry renders and rows only reflect server data.
   * The name is stable across editions; a rename is a breaking change.
   */
  viewPublish?: ViewPublishSlot;
  /**
   * Workspace view layouts (VIEW-040, NEWFRONT-729; Decisions D25). The
   * server layout keys the edition offers on the workspace views
   * surface, in selector order. The app owns the selector chrome (the
   * issues layout switcher) and the persist (the workspace view's
   * display-filter PATCH, an OSS endpoint); the slot only decides which
   * layouts may be picked. The OSS edition leaves it absent: no
   * selector renders and the spreadsheet body renders. The name is
   * stable across editions; a rename is a breaking change.
   */
  workspaceViewLayouts?: WorkspaceViewLayoutsSlot;
}

/** Access values the slot trades in: 1 shared, 0 owner-only. */
export type ViewAccessValue = 0 | 1;

/**
 * Layouts an edition offers on the workspace views surface (VIEW-040).
 * Entries are server layout keys (`list`, `kanban`, `calendar`,
 * `spreadsheet`, `gantt_chart`); the app ignores keys it does not know.
 */
export interface WorkspaceViewLayoutsSlot {
  /** Offered layouts, in the order the selector shows them. */
  layouts: readonly string[];
}

/** Props the edition's access selector receives inside the dialog. */
export interface ViewAccessFieldProps {
  /** Current choice; the dialog defaults it to shared. */
  value: ViewAccessValue;
  onChange: (access: ViewAccessValue) => void;
  /** True while a submit is in flight; the selector takes no edits. */
  disabled: boolean;
}

/** Where the edition persists an owner-only choice (VIEW-019, VIEW-034). */
export interface ViewAccessStoreInput {
  workspaceSlug: string;
  /** The owning project; null for a workspace view, which spans projects. */
  projectId: string | null;
  viewId: string;
  access: ViewAccessValue;
}

/**
 * The cloud access seam: the selector UI plus the write that stores a
 * non-default choice after the OSS create/patch lands. The dialog calls
 * `storeAccess` only for a choice the model default would not produce.
 */
export interface ViewAccessSlot {
  Field: ComponentType<ViewAccessFieldProps>;
  storeAccess: (input: ViewAccessStoreInput) => Promise<unknown>;
}

/** Which saved view a publish write acts on (VIEW-020). */
export interface ViewPublishInput {
  workspaceSlug: string;
  projectId: string;
  viewId: string;
}

/** What a publish write returns: the anchor the server set (VIEW-020). */
export interface ViewPublishResult {
  anchor: string;
}

/**
 * The cloud publish seam: the two anchor writes plus the public link
 * derivation. The app calls `publish` for an unpublished view and
 * `unpublish` for a published one, then adopts the anchor (or its
 * absence) into the cached record so the Live marker follows.
 */
export interface ViewPublishSlot {
  publish: (input: ViewPublishInput) => Promise<ViewPublishResult>;
  unpublish: (input: ViewPublishInput) => Promise<void>;
  /** Public read-only address for an anchor the server set. */
  publicLink: (anchor: string) => string;
}

export interface Edition {
  id: "oss" | string;
  routes?: RouteExtension[];
  sidebar?: SidebarItem[];
  /** Far-left rail dock entries; the rail mounts only with APP_RAIL_FLAG. */
  rail?: SidebarItem[];
  settingsSections?: SettingsSection[];
  auth?: AuthProviderExtension[];
  /**
   * Request middleware, e.g. token refresh. Runs inside the api client,
   * in listed order, within the session-expiry layer (the expiry layer
   * reads each answer's final verdict). The OSS edition leaves it absent.
   */
  api?: Middleware[];
  flags: Record<string, boolean>;
  slots?: Partial<SlotComponents>;
}
