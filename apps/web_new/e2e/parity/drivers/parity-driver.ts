// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity driver interface (NEWFRONT-19). Scenarios are written against this
// interface only: user-level actions plus user-visible reads. Two drivers
// implement it (drivers/web against apps/web, drivers/web_new against
// apps/web_new) so the same scenario runs on both apps. Extend the
// interface when a new area needs an action; never fork a driver.
import type { Page } from "@playwright/test";

/** Which frontend a scenario run targets. */
export type ParityTarget = "web" | "web_new";

/** Facts every parity scenario may assume about the seeded stack. */
export interface ParitySeedFacts {
  email: string;
  password: string;
  workspaceSlug: string;
  workspaceName: string;
  /** UUID string of the seeded project. */
  projectId: string;
  projectName: string;
  /** Issue names in the order the seed created them. */
  issueNames: string[];
}

/** User-level actions plus reads shared by both frontend drivers. */
export interface ParityDriver {
  readonly target: ParityTarget;
  readonly page: Page;
  /** Open the app entry point as a signed-out visitor. */
  openEntry(): Promise<void>;
  /** Complete the email-plus-password sign-in flow; ends authenticated. */
  signInWithPassword(email: string, password: string): Promise<void>;
  /** Open the issues list of a project; requires an authenticated session. */
  openProjectIssues(workspaceSlug: string, projectId: string): Promise<void>;
  /** Names of the issues currently rendered in the list, in display order. */
  visibleIssueNames(): Promise<string[]>;

  // -------------------------------------------------------------------------
  // Projects list + lifecycle (NEWFRONT-124, rows SHELL-024..045).
  // Added additively to the interface (never forking a driver). Single-user
  // scenarios sign in through the UI; multi-user scenarios (join, leave,
  // non-member card state, guest gating) enter the app pre-authenticated by
  // injecting a minted user's session cookies, then drive the list UI.
  // -------------------------------------------------------------------------

  /** Enter the app at `path` already signed in as the owner of `cookies`. */
  openAuthenticatedAt(path: string, cookies: ParityBrowserCookie[]): Promise<void>;
  /** The current location's path (what the address bar shows). */
  currentUrlPath(): Promise<string>;
  /** Whether any element with this visible text is present. */
  hasText(text: string): Promise<boolean>;

  // --- list + responsive grid (SHELL-024, SHELL-025) ---
  /** Open the projects list of a workspace; requires an authenticated session. */
  openProjectsList(workspaceSlug: string): Promise<void>;
  /** Open the archived-projects view of a workspace. */
  openArchivedProjects(workspaceSlug: string): Promise<void>;
  /** Names shown on the project cards currently rendered, in display order. */
  visibleProjectCardNames(): Promise<string[]>;
  /** Wait until at least one project card (by name) is rendered. */
  awaitProjectCard(name: string): Promise<void>;
  /** Number of grid columns the card grid resolves to at the current viewport. */
  gridColumnCount(): Promise<number>;
  /** Set the browser viewport width (px), keeping a tall height. */
  setViewportWidth(width: number): Promise<void>;
  /** Whether the list is showing its loading skeleton (shimmer cards, no names). */
  isProjectsSkeletonVisible(): Promise<boolean>;

  // --- empty / no-match / archive-empty (SHELL-026, SHELL-027) ---
  /** The visible empty/no-match/archive-empty state heading, or null. */
  emptyStateHeading(): Promise<string | null>;
  /** Whether the empty-state primary action (create first project) is present. */
  isEmptyStateCreateVisible(): Promise<boolean>;
  /** Whether that empty-state action is enabled (guests get it disabled). */
  isEmptyStateCreateEnabled(): Promise<boolean>;
  /** Click the empty-state create action (opens the create modal). */
  clickEmptyStateCreate(): Promise<void>;

  // --- desktop + mobile header (SHELL-043, SHELL-044, SHELL-045) ---
  /** Whether the header create button is present (hidden from guests / archive view). */
  isHeaderCreateButtonVisible(): Promise<boolean>;
  /** The visible label on the header create button, or null when absent. */
  headerCreateButtonLabel(): Promise<string | null>;
  /** Click the header create button (opens the create modal). */
  clickHeaderCreateButton(): Promise<void>;
  /** The breadcrumb labels shown in the list header, in order. */
  breadcrumbLabels(): Promise<string[]>;
  /** Whether the mobile (touch) list header bar is the one shown at this width. */
  isMobileListHeaderVisible(): Promise<boolean>;
  /** Whether the desktop filter row is shown at this width. */
  isDesktopFilterRowVisible(): Promise<boolean>;
  /** Whether the terminal (last) breadcrumb crumb is a link rather than plain text. */
  breadcrumbTerminalIsLink(): Promise<boolean>;

  // --- sort (SHELL-028) ---
  openSortMenu(): Promise<void>;
  /** Choose a sort key by its visible label (e.g. "Name", "Manual"). */
  selectSortOption(label: string): Promise<void>;
  /** The current sort key label shown on the sort trigger. */
  currentSortLabel(): Promise<string>;
  /** Whether the direction items (Ascending/Descending) are disabled in the open menu. */
  isSortDirectionDisabled(): Promise<boolean>;
  closeMenu(): Promise<void>;

  // --- filter panel + applied strip (SHELL-029, SHELL-030) ---
  openFilterMenu(): Promise<void>;
  /** Type into the filter panel's value-search box. */
  typeFilterSearch(text: string): Promise<void>;
  /** Whether the filter panel currently lists an option with this visible text. */
  filterMenuHasOption(text: string): Promise<boolean>;
  /** Choose a filter option by its visible label in the open panel. */
  selectFilterOption(label: string): Promise<void>;
  /** Whether the filter trigger shows its active-filters badge/dot. */
  isFilterBadgeVisible(): Promise<boolean>;
  /** Texts of the applied-filter chips currently in the strip. */
  appliedFilterChipTexts(): Promise<string[]>;
  /** Remove one applied-filter value chip by its visible text. */
  removeAppliedFilterChip(text: string): Promise<void>;
  /** Click the strip's "Clear all". */
  clickClearAllFilters(): Promise<void>;
  /** The match-count text shown beside the strip (e.g. "1/3"), or null. */
  filterMatchCountText(): Promise<string | null>;

  // --- list search (SHELL-031) ---
  /** Expand the list search from its magnifier icon. */
  openListSearch(): Promise<void>;
  typeListSearch(text: string): Promise<void>;
  listSearchValue(): Promise<string>;
  /** Whether the search input is currently expanded/visible. */
  isListSearchExpanded(): Promise<boolean>;
  pressEscapeInListSearch(): Promise<void>;
  clickListSearchClear(): Promise<void>;
  /** Click a neutral area outside the search to test outside-click collapse. */
  clickOutsideListSearch(): Promise<void>;

  // --- card contents (SHELL-032) ---
  /** The short code (identifier) shown on a project card, or null. */
  cardShortCode(name: string): Promise<string | null>;
  /** Whether the card shows the private (lock) mark. */
  cardHasPrivateMark(name: string): Promise<boolean>;
  /** The card's description-or-generated sub-line text, or null. */
  cardSubText(name: string): Promise<string | null>;
  /** Whether the favorite star control is rendered on the card. */
  cardHasFavoriteStar(name: string): Promise<boolean>;
  /** Toggle the favorite star on a card. */
  clickFavoriteStar(name: string): Promise<void>;

  // --- routing + quick actions (SHELL-033, SHELL-034) ---
  /** Click a project card body (routing / intercept depends on membership). */
  clickProjectCard(name: string): Promise<void>;
  /** Open a card's right-click context menu. */
  openCardContextMenu(name: string): Promise<void>;
  /** Labels of the items in the currently open context menu. */
  contextMenuItemLabels(): Promise<string[]>;
  /** Click a context-menu item by its visible label. */
  clickContextMenuItem(label: string): Promise<void>;
  /** Labels of the inline footer actions on a card (Join / Settings / Restore / Archived …). */
  cardFooterLabels(name: string): Promise<string[]>;
  /** Click a card's inline footer "Join" control. */
  clickCardJoin(name: string): Promise<void>;

  // --- join flow (SHELL-035) ---
  isJoinDialogVisible(): Promise<boolean>;
  /** The join dialog heading text, or null. */
  joinDialogHeading(): Promise<string | null>;
  confirmJoin(): Promise<void>;

  // --- leave flow (SHELL-036) ---
  /** Open the leave-project dialog for a project (via its reachable entry point). */
  openLeaveProjectDialog(projectName: string): Promise<void>;
  fillLeaveProjectName(text: string): Promise<void>;
  fillLeaveConfirmPhrase(text: string): Promise<void>;
  submitLeave(): Promise<void>;
  /** The visible leave error/toast text after a failed submit, or null. */
  leaveErrorText(): Promise<string | null>;
  isLeaveDialogVisible(): Promise<boolean>;

  // --- archive / restore (SHELL-037, SHELL-038) ---
  /** Open the archive dialog for a project (via its reachable entry point). */
  openArchiveProjectDialog(projectName: string): Promise<void>;
  /** The archive/restore dialog body text, or null. */
  archiveDialogBodyText(): Promise<string | null>;
  confirmArchive(): Promise<void>;
  /** Click the inline restore control on an archived card. */
  clickCardRestore(name: string): Promise<void>;
  confirmRestore(): Promise<void>;
  /** Whether an archived card exposes inline restore/delete admin actions. */
  archivedCardHasAdminActions(name: string): Promise<boolean>;
  /** Whether an archived card shows its muted "Archived" marker. */
  cardShowsArchivedMarker(name: string): Promise<boolean>;

  // --- delete (SHELL-039) ---
  /** Open the delete dialog from an archived card (menu or footer). */
  openDeleteProjectDialog(name: string): Promise<void>;
  fillDeleteProjectName(text: string): Promise<void>;
  fillDeleteConfirmPhrase(text: string): Promise<void>;
  isDeleteSubmitDisabled(): Promise<boolean>;
  submitDelete(): Promise<void>;

  // --- create flow (SHELL-040, SHELL-041, SHELL-042) ---
  isCreateProjectDialogVisible(): Promise<boolean>;
  fillCreateProjectName(text: string): Promise<void>;
  /** The current value of the auto-derived short-code field. */
  createProjectShortCodeValue(): Promise<string>;
  fillCreateProjectShortCode(text: string): Promise<void>;
  submitCreateProject(): Promise<void>;
  /** The visible create error text (duplicate name/code, upload warning), or null. */
  createProjectErrorText(): Promise<string | null>;
}
