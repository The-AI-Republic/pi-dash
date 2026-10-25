// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages area barrel (NEWFRONT-835 seeds the list shell and discovery
// surface; later slices add creation, row actions, the editor, live
// collaboration and nested pages).

export { pageKeys } from "./api/keys.js";
export {
  flipPageFavorite,
  flipPageFavoriteInList,
  PAGE_CREATE_FAILED_TITLE,
  PAGE_FAVORITE_ADDED_TITLE,
  PAGE_FAVORITE_ADDING_TITLE,
  PAGE_FAVORITE_FAILED_TITLE,
  PAGE_FAVORITE_REMOVE_FAILED_TITLE,
  PAGE_FAVORITE_REMOVED_TITLE,
  PAGE_FAVORITE_REMOVING_TITLE,
  useCreatePage,
  useDuplicatePage,
  usePageFavorite,
  useQuickCreatePage,
  type CreatePageVariables,
  type DuplicatePageVariables,
  type PageFavoriteTarget,
} from "./api/mutations.js";
export { pagesListQueryOptions } from "./api/queries.js";
export {
  PageCreateDialog,
  PAGE_CREATE_DIALOG_TITLE,
  PAGE_CREATE_SUBMIT_LABEL,
  type PageCreateDialogProps,
} from "./components/PageCreateDialog.js";
export { PageFavoriteStar, type PageFavoriteStarProps } from "./components/PageFavoriteStar.js";
export {
  PageMarkPicker,
  pageMarkColor,
  pageMarkTriggerGlyph,
  type PageMarkPickerProps,
} from "./components/PageMarkPicker.js";
export {
  PageCreateDialogEntry,
  PagesQuickCreate,
  CREATE_PAGE_COMMAND_ID,
  CREATE_PAGE_COMMAND_SEQUENCE,
  CREATE_PAGE_COMMAND_TITLE,
  type PageCreateDialogEntryProps,
  type PagesQuickCreateProps,
} from "./components/PagesCreateEntry.js";
export { PageRow, type PageRowProps } from "./components/PageRow.js";
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
  PAGE_ACTION_ARCHIVE,
  PAGE_ACTION_COPY_LINK,
  PAGE_ACTION_DELETE,
  PAGE_ACTION_DUPLICATE,
  PAGE_ACTION_LOCK,
  PAGE_ACTION_MAKE_PRIVATE,
  PAGE_ACTION_MAKE_PUBLIC,
  PAGE_ACTION_MOVE,
  PAGE_ACTION_OPEN_IN_NEW_TAB,
  PAGE_ACTION_RESTORE,
  PAGE_ACTION_UNLOCK,
  PAGE_ARCHIVED_TITLE,
  PAGE_ARCHIVE_FAILED_TITLE,
  PAGE_DELETED_TITLE,
  PAGE_DELETE_FAILED_TITLE,
  PAGE_DUPLICATE_FAILED_TITLE,
  PAGE_DUPLICATED_TITLE,
  PAGE_LOCKED_TITLE,
  PAGE_LOCK_FAILED_TITLE,
  PAGE_MOVED_TITLE,
  PAGE_MOVE_FAILED_TITLE,
  PAGE_RESTORED_TITLE,
  PAGE_RESTORE_FAILED_TITLE,
  PAGE_UNLOCKED_TITLE,
  PAGE_VISIBILITY_FAILED_TITLE,
  PAGE_VISIBILITY_PUBLIC_SUFFIX,
  PAGE_VISIBILITY_SUFFIX,
  pageRowActions,
  type PageRowCapabilities,
  type PageRowHandlers,
} from "./lib/pageActions.js";
export {
  PAGE_EVENT_ARCHIVED,
  PAGE_EVENT_DELETED,
  PAGE_EVENT_LOCKED,
  PAGE_EVENT_MADE_PRIVATE,
  PAGE_EVENT_MADE_PUBLIC,
  PAGE_EVENT_MOVED,
  PAGE_EVENT_UNARCHIVED,
  PAGE_EVENT_UNLOCKED,
  PAGES_REALTIME_TOPIC,
  patchPageRow,
  publishPageEvent,
  removePageRow,
  usePageArchiveWrite,
  usePageDeleteWrite,
  usePageLockWrite,
  usePageMoveWrite,
  usePageVisibilityWrite,
  type PageEventPayload,
  type PageRowTarget,
} from "./api/rowMutations.js";
export { PageDeleteDialog, type PageDeleteDialogProps } from "./components/PageDeleteDialog.js";
export { PageMoveDialog, type PageMoveDialogProps } from "./components/PageMoveDialog.js";
export { PageRowConnected, type PageRowConnectedProps } from "./components/PageRowConnected.js";
export {
  PAGE_QUICK_CREATE_NAME,
  PAGE_TITLE_MAX_LENGTH,
  PAGE_TITLE_TOO_LONG_MESSAGE,
  pageCreateInput,
  pageDraftSubmittable,
  pageMarkFromDraft,
  pageTitleLength,
  pageTitleMissing,
  pageTitleTooLong,
  quickCreateAccess,
} from "./lib/pageCreate.js";
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
