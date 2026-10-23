// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Features import { edition } from here. The value comes from the single
// @pidash/edition alias (oss.ts in this repo), which the cloud build
// replaces with its own module implementing the same interface.

export type {
  AuthProviderExtension,
  Edition,
  RouteExtension,
  SettingsSection,
  SidebarItem,
  SlotComponents,
  ViewAccessFieldProps,
  ViewAccessSlot,
  ViewAccessStoreInput,
  ViewAccessValue,
  ViewPublishInput,
  ViewPublishResult,
  ViewPublishSlot,
  WorkspaceViewLayoutsSlot,
} from "./types.js";
export { APP_RAIL_FLAG, ESTIMATE_TIME_FAMILY_FLAG, ISSUE_DEDUPE_FLAG, SIDEBAR_TOGGLE_FLAG } from "./types.js";
export { isAppRailEnabled, isSidebarToggleEnabled } from "./flags.js";
export {
  APPS_CATALOG_ROUTE_PATH,
  CLOUD_LOGIN_PATH,
  MARKETING_HOME_PATH,
  PUBLIC_MARKETING_PATHS,
  ROOT_ROUTE_PATH,
  SITEMAP_MARKETING_PATHS,
  findEditionSettingsSection,
  isEditionMarketingPath,
  isEditionProviderFreePath,
  isEditionPublicRouteEnabled,
  isEditionRootEnabled,
  isEditionRouteEnabled,
  isEditionSettingsSectionEnabled,
  isTopLevelRoute,
  publicMarketingPathOf,
  resolveEditionSidebarHref,
  signedOutRootTarget,
  type PublicMarketingPath,
} from "./routes.js";
export { edition } from "@pidash/edition";
