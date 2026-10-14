// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App shell (Architecture > shared/shell). Pure layout plus the three
// global wirings: command registration, the palette shortcut, and the
// session-expired redirect. Screens arrive as children; chrome arrives as
// props from the workspace route, which also owns data loading. The rail,
// slide-over and dialog slots stay mounted by the shell so sibling slices
// fill them without reworking this composition.

import { ToastHost } from "@pidash/kit";
import * as React from "react";

import {
  matchesShortcut,
  registerCommands,
  useCommandPalette,
  type PaletteSearchScope,
  type PidashCommand,
  type SearchResultHit,
} from "../commands/index.js";
import { CommandPalette } from "./CommandPalette.js";
import { useSessionExpiredRedirect } from "./expiredRedirect.js";
import { getToastManager } from "./toaster.js";

export interface AppShellProps {
  topBar: React.ReactNode;
  sidebar: React.ReactNode;
  /** Global commands owned by the current screen (sign-out, navigation). */
  commands?: PidashCommand[];
  /** Where the palette searches; null renders it commands-only. */
  searchScope: PaletteSearchScope | null;
  /** Runs when a palette server hit activates (route-owned navigation). */
  onSelectSearchResult: (hit: SearchResultHit) => void;
  /** Optional far-left strip; mounted by the rail slice, suppressed otherwise. */
  rail?: React.ReactNode;
  /** Slide-over panels beside the outlet (overflow lists, detail peeks). */
  slideOver?: React.ReactNode;
  /** Workspace-scoped global dialogs, available on every workspace page. */
  dialogs?: React.ReactNode;
  children: React.ReactNode;
}

const PALETTE_SHORTCUT = "mod+k";

export function AppShell({
  topBar,
  sidebar,
  commands = [],
  searchScope,
  onSelectSearchResult,
  rail,
  slideOver,
  dialogs,
  children,
}: AppShellProps): React.ReactElement {
  const togglePalette = useCommandPalette((state) => state.togglePalette);
  useSessionExpiredRedirect();

  React.useEffect(() => registerCommands(commands), [commands]);

  React.useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (matchesShortcut(event, PALETTE_SHORTCUT)) {
        event.preventDefault();
        togglePalette();
      }
    };
    globalThis.window.addEventListener("keydown", onKeyDown);
    return () => {
      globalThis.window.removeEventListener("keydown", onKeyDown);
    };
  }, [togglePalette]);

  return (
    <div className="flex h-screen flex-col bg-(--bg) text-(--text)">
      {topBar}
      <div className="flex min-h-0 flex-1">
        {rail}
        {sidebar}
        <main className="min-w-0 flex-1 overflow-auto" aria-label="Content">
          {children}
        </main>
        {slideOver ? (
          <aside aria-label="Slide-over" className="min-h-0 shrink-0 overflow-auto">
            {slideOver}
          </aside>
        ) : null}
      </div>
      <CommandPalette scope={searchScope} onSelectSearchResult={onSelectSearchResult} />
      <ToastHost manager={getToastManager()} />
      {dialogs}
    </div>
  );
}
