// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// App shell (Architecture > shared/shell). Pure layout plus the three
// global wirings: command registration, the palette shortcut, and the
// session-expired redirect. Screens arrive as children; chrome arrives as
// props from the workspace route, which also owns data loading.

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
  children: React.ReactNode;
}

const PALETTE_SHORTCUT = "mod+k";

export function AppShell({ topBar, sidebar, commands = [], children }: AppShellProps): React.ReactElement {
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
        {sidebar}
        <main className="min-w-0 flex-1 overflow-auto" aria-label="Content">
          {children}
        </main>
      </div>
      <CommandPalette />
      <ToastHost manager={getToastManager()} />
    </div>
  );
}
