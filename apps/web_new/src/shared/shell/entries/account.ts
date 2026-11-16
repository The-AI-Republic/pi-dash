// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette account, utility and help entries (shell spec, SHELL-089):
// workspace invites, the sidebar toggle, copy-URL, focus-search, the
// shortcut viewer and the external help links. Sign-out stays owned by
// the auth area (features/auth/commands.ts); the route registers that
// descriptor beside this catalog. Open to every signed-in user.

import type { PidashCommand } from "../../commands/index.js";
import { useShellChrome } from "../chromeState.js";
import { copyCurrentPageUrl } from "../clipboard.js";
import { useShortcutsDialog } from "../shortcutsDialogStore.js";
import { useTopSearchFocus } from "../topSearchFocus.js";

/**
 * External help destinations (SHELL-089); all leave the app. Lazy-only
 * mirror of shared/shell/externalLinks.ts (same values; account.test.ts
 * pins them together): eager components must use the leaf, because a
 * module imported from both a route's eager closure and its lazy
 * chunks collapses that route's TanStack component split (NEWFRONT-287).
 */
export const DOCS_URL = "https://github.com/The-AI-Republic/pi-dash#readme";
export const FORUM_URL = "https://github.com/The-AI-Republic/pi-dash/discussions";
export const REPORT_BUG_URL = "https://github.com/The-AI-Republic/pi-dash/issues/new/choose";

export interface AccountEntryContext {
  /** Routes to the workspace invitations inbox. */
  onWorkspaceInvites: () => void;
  /** Opens a URL outside the app (system browser on desktop). */
  onOpenExternal: (url: string) => void;
}

/** The account, utility and help entries for the current screen. */
export function buildAccountCommands(context: AccountEntryContext): PidashCommand[] {
  return [
    {
      id: "shell.workspace-invites",
      title: "Workspace invites",
      section: "Account",
      run: context.onWorkspaceInvites,
    },
    {
      id: "shell.toggle-sidebar",
      title: "Toggle app sidebar",
      section: "Miscellaneous",
      shortcut: "mod+b",
      run: () => {
        // Same flip as the top-bar toggle (SHELL-066): peek never survives it.
        useShellChrome.getState().clearPeek();
        useShellChrome.getState().toggleSidebarCollapsed();
      },
    },
    {
      id: "shell.copy-page-url",
      title: "Copy current page URL",
      section: "Miscellaneous",
      shortcut: "mod+shift+c",
      keywords: ["clipboard", "link"],
      run: () => void copyCurrentPageUrl(),
    },
    {
      id: "shell.focus-search",
      title: "Focus search input",
      section: "Miscellaneous",
      shortcut: "mod+f",
      keywords: ["find"],
      run: () => useTopSearchFocus.getState().requestTopSearchFocus(),
    },
    {
      id: "shell.open-shortcuts",
      title: "Open keyboard shortcuts",
      section: "Help",
      shortcut: "mod+/",
      keywords: ["hotkeys", "keys"],
      run: () => useShortcutsDialog.getState().openShortcuts(),
    },
    {
      id: "shell.docs",
      title: "Open Pi Dash documentation",
      section: "Help",
      run: () => context.onOpenExternal(DOCS_URL),
    },
    {
      id: "shell.forum",
      title: "Join our Forum",
      section: "Help",
      run: () => context.onOpenExternal(FORUM_URL),
    },
    {
      id: "shell.report-bug",
      title: "Report a bug",
      section: "Help",
      run: () => context.onOpenExternal(REPORT_BUG_URL),
    },
  ];
}
