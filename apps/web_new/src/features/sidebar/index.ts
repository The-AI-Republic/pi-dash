// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the sidebar feature. Other features import only from here.

export { sidebarKeys } from "./api/keys.js";
export {
  PERSONAL_CATALOG,
  applySidebarPatches,
  comparePersonalOrder,
  moveEntryOrder,
  reorderPatches,
  selectPersonalEntries,
  selectPersonalGroup,
  selectVisiblePersonalEntries,
  sidebarPreferencesQueryOptions,
  useSidebarPreferences,
  useUpdateSidebarPreferences,
} from "./api/preferences.js";
export type {
  PersonalCatalogEntry,
  PersonalEntryState,
  PersonalGroupState,
  PersonalSidebarEntry,
} from "./api/preferences.js";
export { PersonalEntriesList } from "./components/PersonalEntriesList.js";
export type { PersonalEntriesListProps, PersonalListEntry } from "./components/PersonalEntriesList.js";
export { PROJECT_CAP_MIN_ERROR, PersonalizeDialog } from "./components/PersonalizeDialog.js";
export type { PersonalizeDialogProps } from "./components/PersonalizeDialog.js";
export {
  FAVORITE_ORDER_STEP,
  FAVORITE_REMOVED_TITLE,
  FOLDER_DUPLICATE_TITLE,
  FOLDER_EMPTY_TITLE,
  applyFavoriteSequence,
  compareFavoriteSequence,
  favoriteGroupQueryOptions,
  favoriteOrderPatch,
  favoritesQueryOptions,
  moveFavoriteToIndex,
  selectOrderedFavorites,
  selectTopLevelFavorites,
  sequenceForMove,
  useCreateFavoriteFolder,
  useDeleteFavorite,
  useFavoriteGroup,
  useFavorites,
  useMoveFavorite,
  useRenameFavoriteFolder,
  validateFolderName,
} from "./api/favorites.js";
export type { FavoriteMoveInput, FolderNameIssue } from "./api/favorites.js";
export {
  SidebarFavoritesGroup,
  FAVORITE_REMOVE_LABEL,
  favoriteDisplayName,
} from "./components/SidebarFavoritesGroup.js";
export type { SidebarFavoritesGroupProps } from "./components/SidebarFavoritesGroup.js";
export { SidebarMoreGroup } from "./components/SidebarMoreGroup.js";
export type { SidebarMoreGroupProps } from "./components/SidebarMoreGroup.js";
export {
  MORE_DESTINATIONS,
  MORE_SECTION_OPEN_KEY,
  isMoreEntryActive,
  isMoreRoute,
  loadMoreOpen,
  moreHref,
  persistMoreOpen,
  selectMoreEntries,
} from "./more.js";
export type { MoreDestination, MoreDestinationDef } from "./more.js";
export { resolveFavoriteHref, resolveFavoriteIconKind } from "./favoriteLinks.js";
export type { FavoriteIconKind } from "./favoriteLinks.js";
