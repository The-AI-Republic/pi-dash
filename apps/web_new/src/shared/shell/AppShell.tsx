// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App shell (Architecture > shared/shell). Pure layout plus the three
// global wirings: command registration, global chords and sequences, and
// the session-expired redirect. Screens arrive as children; chrome arrives as
// props from the workspace route, which also owns data loading. The rail,
// slide-over and dialog slots stay mounted by the shell so sibling slices
// fill them without reworking this composition.

import { ToastHost } from "@pidash/kit";
import * as React from "react";

import {
  registerCommands,
  useCommandPalette,
  type PaletteSearchScope,
  type PidashCommand,
  type SearchResultHit,
} from "../commands/index.js";
import { useSessionExpiredRedirect } from "./expiredRedirect.js";
import { useGlobalShortcuts } from "./globalShortcuts.js";
import { useShortcutsDialog } from "./shortcutsDialogStore.js";
import { getToastManager } from "./toaster.js";

// Deferred below the initial shell chain (Quality gates budget): the
// dialog content loads on first open (shell spec, SHELL-094).
const LazyShortcutsDialog = React.lazy(() =>
  import("./ShortcutsDialog.js").then((module) => ({ default: module.ShortcutsDialog }))
);

// The palette UI splits off the initial bundle (size gate): it mounts on
// first open and stays mounted after, so later opens and exit animations
// behave exactly as if it had always been there.
const LazyPalette = React.lazy(() =>
  import("./CommandPalette.js").then((module) => ({ default: module.CommandPalette }))
);

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
  /** False on screens that pin their key handling (PSET-062). */
  suppressPageShortcuts?: boolean;
  children: React.ReactNode;
}

export function AppShell({
  topBar,
  sidebar,
  commands = [],
  searchScope,
  onSelectSearchResult,
  rail,
  slideOver,
  dialogs,
  suppressPageShortcuts = false,
  children,
}: AppShellProps): React.ReactElement {
  useGlobalShortcuts({ pageShortcuts: !suppressPageShortcuts });
  const paletteOpen = useCommandPalette((state) => state.open);
  const [paletteMounted, setPaletteMounted] = React.useState(paletteOpen);
  useSessionExpiredRedirect();
  const shortcutsOpen = useShortcutsDialog((state) => state.open);

  React.useEffect(() => registerCommands(commands), [commands]);

  React.useEffect(() => {
    if (paletteOpen) setPaletteMounted(true);
  }, [paletteOpen]);

  return (
    <div className="flex h-screen flex-col bg-(--bg) text-(--text)">
      {/* First: the toast manager emits to current subscribers only, so
          the host subscribes before any page mount effect can notify
          (CLOUD-026's bounce). The viewport is fixed: order is not layout. */}
      <ToastHost manager={getToastManager()} />
      {topBar}
      {/* Positioned: the floating sidebar and its peek overlay anchor here, below the top bar. */}
      <div className="relative flex min-h-0 flex-1">
        {/* The workspace route lazy-loads the rail (size budget); the slot stays sync. */}
        <React.Suspense fallback={null}>{rail}</React.Suspense>
        {/* The sidebar renders its own collapsed strip (SHELL-067/068), so the slot always stays mounted. */}
        {sidebar}
        <main
          className={rail ? "min-w-0 flex-1 overflow-auto" : "min-w-0 flex-1 overflow-auto pl-(--space-4)"}
          aria-label="Content"
        >
          {children}
        </main>
        {slideOver ? (
          <aside aria-label="Slide-over" className="min-h-0 shrink-0 overflow-auto">
            {slideOver}
          </aside>
        ) : null}
      </div>
      {paletteMounted ? (
        <React.Suspense fallback={null}>
          <LazyPalette scope={searchScope} onSelectSearchResult={onSelectSearchResult} />
        </React.Suspense>
      ) : null}
      {shortcutsOpen ? (
        <React.Suspense fallback={null}>
          <LazyShortcutsDialog />
        </React.Suspense>
      ) : null}
      {dialogs}
    </div>
  );
}
