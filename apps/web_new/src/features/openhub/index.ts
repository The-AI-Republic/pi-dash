// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the Apps catalog feature. Other features import only from here.

export {
  COULD_NOT_CONNECT_TITLE,
  HUB_REFUSED_DESCRIPTION,
  INSTALL_FAILED_TITLE,
  INSTALLED_UNCONNECTED_TITLE,
  POPUP_BLOCKED_DESCRIPTION,
  UNINSTALL_FAILED_TITLE,
  connectedTitle,
  installedTitle,
  refreshMarketplace,
  uninstalledTitle,
  useAppCardActions,
} from "./api/actions.js";
export type { AppCardActions, ConnectDialogState } from "./api/actions.js";
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
export { APP_INSTALLED_BADGE, APP_NEEDS_CONNECTION_BADGE, AppCard } from "./components/AppCard.js";
export type { AppCardProps } from "./components/AppCard.js";
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
  CONNECTION_FAILED_MESSAGE,
  CONNECTION_FAILED_TITLE,
} from "./components/AppsView.js";
export {
  CONNECT_DIALOG_CANCEL,
  CONNECT_DIALOG_SETUP_LINK,
  CONNECT_DIALOG_SUBMIT,
  ConnectDialog,
  connectDialogTitle,
} from "./components/ConnectDialog.js";
export type { ConnectDialogProps } from "./components/ConnectDialog.js";
export {
  CONNECT_ERROR_PARAM,
  CONNECT_OUTCOME_PARAM,
  CONNECT_TAB_TITLE,
  CREDENTIAL_FORMAT_ERROR,
  CREDENTIAL_REQUIRED_ERROR,
  FALLBACK_CREDENTIAL_FIELD,
  appMechanism,
  cleanInstructions,
  cleanedBounceHref,
  connectReturnPath,
  dialogFields,
  hasBounceError,
  hasOutcomeParams,
  hubAppUrl,
  isSafeSetupUrl,
  isSecretField,
  reserveConnectTab,
  stripOutcomeParams,
  trimCredentialValues,
  validateCredentialValues,
} from "./lib/connect.js";
export type { AppMechanism } from "./lib/connect.js";
export {
  appCardActions,
  appCardBadge,
  appIconSource,
  appLetter,
  appNeedsConnection,
  firstIconStage,
  isAppInstalled,
  nextIconStage,
  svgGraphicSource,
} from "./lib/cards.js";
export type { AppCardAction, AppCardBadge, AppIconStage } from "./lib/cards.js";
export { catalogFailureMessage, writeFailureMessage } from "./lib/errors.js";
export {
  createCatalogPager,
  currentPagerCursor,
  nextCatalogPage,
  pagerPage,
  previousCatalogPage,
  useCatalogPager,
} from "./lib/pager.js";
export type { CatalogPager, CatalogPagerState } from "./lib/pager.js";
