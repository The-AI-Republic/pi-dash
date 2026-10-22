// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { AppShell } from "./AppShell.js";
export type { AppShellProps } from "./AppShell.js";
export { CommandPalette } from "./CommandPalette.js";
export { bouncePathname, isBounceExempt, useSessionExpiredRedirect } from "./expiredRedirect.js";
export {
  EDITOR_PORTAL_ID,
  FULL_SCREEN_PORTAL_ID,
  MENU_PORTAL_ID,
  OverlayPortal,
  markInitialColorScheme,
  useDocumentHead,
} from "./document.js";
export type { DocumentHead, HeadTag, OverlayPortalProps, OverlayRootId } from "./document.js";
export { FAVORITES_SECTION_OPEN_KEY, isFavoritesSectionOpen, setFavoritesSectionOpen } from "./favorites.js";
export { NOT_FOUND_ART, NOT_FOUND_ROBOTS, NOT_FOUND_TITLE, NotFoundPage } from "./NotFoundPage.js";
export { PeekPanel } from "./PeekPanel.js";
export type { PeekPanelProps } from "./PeekPanel.js";
export { REPOSITORY_URL, RepoStar, starIconSrc } from "./RepoStar.js";
export { Sidebar } from "./Sidebar.js";
export type { SidebarEditionEntry, SidebarPersonalEntry, SidebarProps, SidebarWorkspace } from "./Sidebar.js";
export { SIDEBAR_PEEK_DISMISS_MS, SidebarShell } from "./SidebarShell.js";
export type { SidebarShellProps } from "./SidebarShell.js";
export { NARROW_VIEWPORT_BREAKPOINT_PX, useNarrowViewport } from "./narrowViewport.js";
export { isClientSideClick, isHomeRowActive } from "./sidebarLink.js";
export type { SidebarClick } from "./sidebarLink.js";
export { TitleBar } from "./TitleBar.js";
export type { TitleBarProps } from "./TitleBar.js";
export { TopBar } from "./TopBar.js";
export type { TopBarProps } from "./TopBar.js";
export { TopBarSearch, TOPBAR_SEARCH_PLACEHOLDER } from "./TopBarSearch.js";
export type { TopBarSearchProps } from "./TopBarSearch.js";
export { useWindowTitle } from "./windowTitle.js";
export {
  MAIN_SIDEBAR_PANEL,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  clampSidebarWidth,
  createSidebarPanelStore,
  loadSidebarPanel,
  sidebarPanelStorageKey,
  sidebarPanelStore,
  useShellChrome,
} from "./chromeState.js";
export type { SidebarPanelSnapshot, SidebarPanelState } from "./chromeState.js";
export { isRailUndocked, useRailState } from "./railState.js";
export type { RailDensity } from "./railState.js";
export { isDockItemActive, isSettingsEntryActive, WorkspaceRail } from "./WorkspaceRail.js";
export type { RailDockItem, WorkspaceRailProps } from "./WorkspaceRail.js";
export {
  getToastManager,
  notifyError,
  notifyInfo,
  notifyPending,
  notifySuccess,
  notifySuccessWithAction,
} from "./toaster.js";
export type { ToastFollowUp } from "./toaster.js";
export { loadPreference, preferenceStorage, removePreference, storePreference } from "./preferenceStorage.js";
export type { PreferenceStorage } from "./preferenceStorage.js";
export { buildAccountCommands, DOCS_URL, FORUM_URL, REPORT_BUG_URL } from "./entries/account.js";
export type { AccountEntryContext } from "./entries/account.js";
export { buildCreationCommands } from "./entries/creation.js";
export type { CreationEntryContext } from "./entries/creation.js";
export { buildNavigationCommands, PROJECT_PICKER_PAGE_ID, WORKSPACE_PICKER_PAGE_ID } from "./entries/navigation.js";
export type { NavigationEntryContext, NavigationProject, NavigationProjectViews } from "./entries/navigation.js";
export { projectPickerPage, workspacePickerPage } from "./entries/navigationPages.js";
export type { PickerProject, PickerWorkspace } from "./entries/navigationPages.js";
export {
  buildPreferenceCommands,
  FIRST_DAY_PAGE_ID,
  firstDayPickerPage,
  LANGUAGE_PAGE_ID,
  languagePickerPage,
  THEME_PAGE_ID,
  themePickerPage,
  TIMEZONE_PAGE_ID,
  timezonePickerPage,
} from "./entries/preferenceEntries.js";
export { PAGE_URL_COPIED_TITLE, PAGE_URL_COPY_FAILED_TITLE, copyCurrentPageUrl } from "./clipboard.js";
export {
  FIRST_DAY_CHOICES,
  PREFERENCES_FAILED_TITLE,
  PREFERENCES_UPDATED_TITLE,
  THEME_CHOICES,
  TIMEZONE_FAILED_TITLE,
  TIMEZONE_UPDATED_TITLE,
  preferenceKeys,
  timezonesQueryOptions,
  useUpdateFirstDay,
  useUpdateTimezone,
} from "./preferences.js";
export type { PreferenceChoice, TimezoneMutationHandlers } from "./preferences.js";
export { groupBoundCommands, ShortcutsDialog } from "./ShortcutsDialog.js";
export type { ShortcutGroup } from "./ShortcutsDialog.js";
export { useShortcutsDialog } from "./shortcutsDialogStore.js";
export { useTopSearchFocus } from "./topSearchFocus.js";
export { HelpMenu, SALES_EMAIL_URL } from "./HelpMenu.js";
export type { HelpMenuProps } from "./HelpMenu.js";
export { resolveFileUrl } from "./fileUrls.js";
export { APP_VERSION } from "./version.js";
export { WorkspaceLogo } from "./WorkspaceLogo.js";
export type { WorkspaceLogoProps } from "./WorkspaceLogo.js";
export { NOTIFICATIONS_PATH_SEGMENT, isNotificationsPath } from "./sidebarAutohide.js";
export {
  formatMemberCount,
  orderSwitcherWorkspaces,
  switcherRoleLabel,
  WorkspaceSwitcher,
} from "./WorkspaceSwitcher.js";
export type { SwitcherWorkspace, WorkspaceSwitcherProps } from "./WorkspaceSwitcher.js";
export { UserMenu } from "./UserMenu.js";
export type { UserMenuProps } from "./UserMenu.js";
export {
  CHANGELOG_LOAD_TIMEOUT_MS,
  CHANGELOG_URL,
  SUPPORT_EMAIL_URL,
  UpdatesDialog,
  changelogCategoryUrl,
} from "./UpdatesDialog.js";
export type { UpdatesDialogProps } from "./UpdatesDialog.js";
