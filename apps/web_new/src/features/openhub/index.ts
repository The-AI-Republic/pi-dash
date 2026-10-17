// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the Apps catalog feature. Other features import only from here.

export { openhubKeys } from "./api/keys.js";
export {
  AGENT_TOOLS_SAVE_FAILED_MESSAGE,
  AGENT_TOOLS_SAVE_FAILED_TITLE,
  agentToolsPatch,
  applyAgentToolsFlip,
  useAgentToolsEnabled,
  useSetAgentToolsEnabled,
} from "./api/mutations.js";
export {
  APPS_CATALOG_PAGE_SIZE,
  APPS_SEARCH_DEBOUNCE_MS,
  marketplacePageQueryOptions,
  openhubStatusQueryOptions,
  useMarketplacePage,
  useOpenhubStatus,
} from "./api/queries.js";
export type { MarketplacePageParams } from "./api/queries.js";
export { AppCard } from "./components/AppCard.js";
export {
  APPS_CATALOG_INTRO,
  APPS_EMPTY_DESCRIPTION,
  APPS_EMPTY_TITLE,
  APPS_NO_MATCHES_TITLE,
  APPS_SEARCH_LABEL,
  APPS_SEARCH_PLACEHOLDER,
  AppsCatalog,
  appsNoMatchesDescription,
} from "./components/AppsCatalog.js";
export { AGENT_TOOLS_DISCLOSURE, AGENT_TOOLS_LABEL, AppsToggle } from "./components/AppsToggle.js";
export type { AppsToggleProps } from "./components/AppsToggle.js";
export {
  APPS_DISCONNECTED_FALLBACK,
  APPS_STALE_BODY,
  APPS_STALE_HEADING,
  APPS_UNAVAILABLE_HEADING,
  AppsView,
} from "./components/AppsView.js";
export { catalogFailureMessage } from "./lib/errors.js";
export {
  createCatalogPager,
  currentPagerCursor,
  nextCatalogPage,
  pagerPage,
  previousCatalogPage,
  useCatalogPager,
} from "./lib/pager.js";
export type { CatalogPager, CatalogPagerState } from "./lib/pager.js";
