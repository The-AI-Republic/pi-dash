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
