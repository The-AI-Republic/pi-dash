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
import { useRouter } from "@tanstack/react-router";
import * as React from "react";

import { matchesShortcut, registerCommands, useCommandPalette, type PidashCommand } from "../commands/index.js";
import { useSessionStore } from "../../core/session/store.js";
import { CommandPalette } from "./CommandPalette.js";
import { getToastManager, notifyError } from "./toaster.js";

export interface AppShellProps {
  topBar: React.ReactNode;
  sidebar: React.ReactNode;
  /** Global commands owned by the current screen (sign-out, navigation). */
  commands?: PidashCommand[];
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
  rail,
  slideOver,
  dialogs,
  children,
}: AppShellProps): React.ReactElement {
  const router = useRouter();
  const status = useSessionStore((state) => state.status);
  const returnUrl = useSessionStore((state) => state.returnUrl);
  const togglePalette = useCommandPalette((state) => state.togglePalette);

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

  const expiredNoticed = React.useRef(false);
  React.useEffect(() => {
    if (status !== "expired" || expiredNoticed.current) return;
    expiredNoticed.current = true;
    notifyError("Session expired", "Sign in again to continue.");
    const current = router.state.location.href;
    void router.navigate({ to: "/sign-in", search: { next: returnUrl ?? current } });
  }, [status, returnUrl, router]);

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
      <CommandPalette />
      <ToastHost manager={getToastManager()} />
      {dialogs}
    </div>
  );
}
