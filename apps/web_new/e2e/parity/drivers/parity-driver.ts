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
  // Command palette / Power-K, search, help, browse, repo-star
  // (NEWFRONT-127, rows SHELL-080, 082, 083, 084, 085, 087, 089, 094, 103, 106).
  // Added additively to the interface (never forking a driver). The palette is
  // a cmdk surface inside a Headless-UI dialog: scenarios drive it with the
  // real keyboard chord and read what the user sees (placeholder, group
  // headings, command titles, the selected row, the server-search heading).
  // -------------------------------------------------------------------------

  /** The current location's path (what the address bar shows). */
  currentUrlPath(): Promise<string>;
  /** Navigate to an arbitrary path already authenticated (workspace pages). */
  goToPath(path: string): Promise<void>;
  /** Whether any element with this visible text is present. */
  hasVisibleText(text: string): Promise<boolean>;

  // --- palette open / close / reset (SHELL-080, SHELL-082) ---
  /** Press the global open chord (Ctrl/Cmd+K). */
  pressPaletteOpenChord(): Promise<void>;
  /** Whether the centered modal palette is open (dialog + command input shown). */
  isCommandPaletteOpen(): Promise<boolean>;
  /** The palette search input's placeholder (identifies root vs a sub-page), or null. */
  commandPalettePlaceholder(): Promise<string | null>;
  /** Focus the top-bar search input (an always-present text field) and type into it. */
  focusAndTypeTopBarSearch(text: string): Promise<void>;
  /** Click the modal backdrop (outside the panel) to close the palette. */
  closeCommandPaletteViaBackdrop(): Promise<void>;

  // --- palette query + keyboard flow (SHELL-083, SHELL-085) ---
  /** Type into the open palette's command input. */
  typeInCommandPalette(text: string): Promise<void>;
  /** The current value of the palette command input. */
  commandPaletteQueryValue(): Promise<string>;
  /** Press a key while the palette input is focused (Escape/Backspace/ArrowDown/Enter/etc.). */
  pressInCommandPalette(key: string): Promise<void>;
  /** Visible group headings currently rendered in the palette, in display order. */
  paletteGroupHeadings(): Promise<string[]>;
  /** Visible command item titles currently rendered in the palette, in display order. */
  paletteCommandTitles(): Promise<string[]>;
  /** Whether a command with this exact title is currently listed. */
  paletteHasCommand(title: string): Promise<boolean>;
  /** Activate (click) a palette command by its exact visible title. */
  activatePaletteCommand(title: string): Promise<void>;
  /** The text of the currently highlighted (aria-selected) palette item, or null. */
  paletteSelectedItemText(): Promise<string | null>;

  // --- server search (SHELL-084) ---
  /** The "Search results for …" heading text shown for a server search, or null. */
  paletteSearchResultsHeading(): Promise<string | null>;
  /** Whether the search-results heading is showing its in-flight pulse. */
  isPaletteSearchHeadingPulsing(): Promise<boolean>;
  /** Whether the footer workspace-level scope toggle is present. */
  paletteHasWorkspaceLevelToggle(): Promise<boolean>;
  /** Whether that scope toggle is enabled (disabled when no project is in context). */
  isWorkspaceLevelToggleEnabled(): Promise<boolean>;
  /** Flip the footer workspace-level scope toggle. */
  toggleWorkspaceLevel(): Promise<void>;
  /**
   * Count palette search requests to GET /workspaces/{slug}/search/ while
   * running `action` (proves debounce coalescing and the no-network blank case).
   */
  countSearchRequests(action: () => Promise<void>): Promise<number>;
  /** The query params of the most recent palette search request, or null. */
  lastSearchRequestParams(): Promise<Record<string, string> | null>;

  // --- shortcuts reference dialog (SHELL-094) ---
  /** Whether the keyboard-shortcuts reference dialog is open. */
  isShortcutsDialogOpen(): Promise<boolean>;
  /** Press the global chord that opens the shortcuts dialog (Ctrl/Cmd+/). */
  pressShortcutsDialogChord(): Promise<void>;
  /** Type into the shortcuts dialog's filter box. */
  typeShortcutsFilter(text: string): Promise<void>;
  /** Visible command titles listed in the shortcuts dialog, in display order. */
  shortcutsDialogCommandTitles(): Promise<string[]>;

  // --- repo-star action (SHELL-103) ---
  /** The repo-star link's {href,target,rel} attributes, or null when absent. */
  repoStarLinkAttributes(): Promise<{ href: string; target: string; rel: string } | null>;
  /** The src of the repo-star icon image (theme-adaptive asset), or null. */
  repoStarIconSrc(): Promise<string | null>;

  // --- preferences: theme, language, timezone, first day of week
  //     (SHELL-088, 095, 096, 097). The four "Change …" preference commands
  //     open cmdk sub-pages whose options are ordinary [cmdk-item] nodes, so the
  //     existing palette readers/activators (paletteHasCommand, paletteCommandTitles,
  //     activatePaletteCommand, typeInCommandPalette) drive them additively.
  //     These two reads observe the applied result the user sees: the theme is a
  //     class on <html>, the interface language is the <html> lang attribute. The
  //     persisted server state is read back through helpers/api. ---
  /** The class attribute of the document root (theme application is class-based), or "". */
  htmlClassList(): Promise<string>;
  /** The lang attribute of the document root (set when the interface language changes). */
  documentLang(): Promise<string>;

  // --- palette creation entries (SHELL-086) ---
  //     The "Create" group commands open their own scoped creation surface:
  //     work-item/page/view/cycle/module/project open a modal dialog (separate
  //     from the palette's cmdk dialog), while workspace creation routes to a
  //     dedicated page. Reuses the existing palette readers/activators; this
  //     one read observes that a non-palette dialog took over after a create
  //     command fired (the palette closes on select).
  /** Whether a dialog that is NOT the cmdk palette is currently open. */
  isNonPaletteDialogOpen(): Promise<boolean>;

  // --- palette pickers: empty / no-results / no-recents (SHELL-093) ---
  //     Picker sub-pages render a plain empty line (e.g. "No projects found")
  //     when a filter matches nothing, and a server search with no hits renders
  //     a no-results row ("No results found — Clear search"); no history/recents
  //     section ever appears. The empty line is plain text (not a cmdk item), so
  //     this scoped read finds any visible text inside the palette surface.
  /** Whether the open palette surface shows this visible text anywhere. */
  paletteHasText(text: string): Promise<boolean>;

  // --- browse route (SHELL-106, negative row) ---
  /** Open the workspace-level browse route for a work-item identifier (e.g. "PROJ-1"). */
  openBrowseWorkItem(workspaceSlug: string, identifier: string): Promise<void>;
  /** Whether the browse route rendered the project-scoped work-item detail view. */
  browseShowsWorkItemDetail(): Promise<boolean>;
  /** Whether any workspace-wide list/grid of work items exists on the browse route. */
  browseShowsWorkspaceWideList(): Promise<boolean>;
}
