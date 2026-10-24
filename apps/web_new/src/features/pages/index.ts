// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages area barrel (NEWFRONT-835 seeds the list shell and discovery
// surface; later slices add creation, row actions, the editor, live
// collaboration and nested pages).

export { pageKeys } from "./api/keys.js";
export { pagesListQueryOptions } from "./api/queries.js";
export { PageRow, type PageRowMenuStrings, type PageRowProps } from "./components/PageRow.js";
export { PagesEmptyState, type PagesEmptyStateProps } from "./components/PagesEmptyState.js";
export { PagesFeatureGate, type PagesFeatureGateProps } from "./components/PagesFeatureGate.js";
export { PagesFilterChips, type PagesFilterChipsProps } from "./components/PagesFilterChips.js";
export { PagesFilterMenu, type PagesFilterMenuProps } from "./components/PagesFilterMenu.js";
export { PagesListScreen, type PagesListScreenProps } from "./components/PagesListScreen.js";
export { PagesListSkeleton } from "./components/PagesListSkeleton.js";
export { PagesListTabs, type PagesListTabsProps } from "./components/PagesListTabs.js";
export { PagesNoResults, type PagesNoResultsProps } from "./components/PagesNoResults.js";
export { PagesSearch, PAGES_SEARCH_PLACEHOLDER, type PagesSearchProps } from "./components/PagesSearch.js";
export { PagesSortMenu, type PagesSortMenuProps } from "./components/PagesSortMenu.js";
export {
  comparePageTitles,
  DEFAULT_PAGE_SORT,
  EMPTY_PAGE_FILTERS,
  hasPageFilters,
  matchesCreationPreset,
  matchesPageFilters,
  matchesPageSearch,
  PAGE_DATE_PRESETS,
  PAGE_LIST_TABS,
  PAGE_SORT_DIRECTIONS,
  PAGE_SORT_KEYS,
  PAGE_UNTITLED,
  pageDetailHref,
  pageDisplayTitle,
  pageFilterChips,
  pageCreatorName,
  pageSessionUserId,
  pageInTab,
  pageSortKeyLabel,
  pageTabHref,
  pagesFeatureHref,
  parsePageListSearch,
  removePageFilter,
  selectPageCreators,
  sortPageRows,
  visiblePageRows,
  type PageChipId,
  type PageCreatorOption,
  type PageDatePreset,
  type PageDateCondition,
  type PageFilterChip,
  type PageFilters,
  type PageListSearch,
  type PageListTab,
  type PageSort,
  type PageSortDirection,
  type PageSortKey,
} from "./lib/pageList.js";
export {
  PAGES_CRUMB,
  pagesTitle,
  resolvePagesBody,
  type PagesBodyInput,
  type PagesBodyState,
} from "./lib/pageScreen.js";
export { copyPageLink, PAGE_LINK_COPIED_TITLE, PAGE_LINK_COPY_FAILED_TITLE, pageLinkUrl } from "./lib/share.js";
