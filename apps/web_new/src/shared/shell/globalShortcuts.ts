// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Global shortcut firing (shell spec, SHELL-080/086/087/089). One keydown
// listener for the shell: the palette chord toggles from anywhere (even
// while typing), while registered chords and two-key sequences fire only
// outside typing surfaces. Page-bound sequences open the palette onto
// their page; anything else runs. Unregistered entries never fire, so
// permission gating falls out of registration (SHELL-092).

import * as React from "react";

import {
  findCommandBySequence,
  isTypingTarget,
  listCommands,
  matchesShortcut,
  useCommandPalette,
  type PidashCommand,
} from "../commands/index.js";

/** Palette chord; the one binding exempt from the typing guard (SHELL-080). */
export const PALETTE_SHORTCUT = "mod+k";

/** Idle window before a partial sequence is forgotten. */
export const SEQUENCE_WINDOW_MS = 1000;

function fireCommand(
  command: PidashCommand,
  actions: { openPalette: () => void; openPage: (page: string) => void }
): void {
  if (command.page !== undefined) {
    actions.openPalette();
    actions.openPage(command.page);
  } else {
    void command.run();
  }
}

/**
 * Attaches the shell's global keydown handling. Mounted once by AppShell.
 * `pageShortcuts: false` keeps the palette chord but drops every page
 * chord and two-key sequence, for screens that pin their key handling
 * (PSET-062).
 */
export function useGlobalShortcuts({ pageShortcuts = true }: { pageShortcuts?: boolean } = {}): void {
  const togglePalette = useCommandPalette((state) => state.togglePalette);
  const openPalette = useCommandPalette((state) => state.openPalette);
  const openPage = useCommandPalette((state) => state.openPage);

  React.useEffect(() => {
    let sequence = "";
    let windowTimer: ReturnType<typeof setTimeout> | null = null;
    const clearWindow = () => {
      sequence = "";
      if (windowTimer !== null) {
        clearTimeout(windowTimer);
        windowTimer = null;
      }
    };
    const armWindow = () => {
      if (windowTimer !== null) clearTimeout(windowTimer);
      windowTimer = setTimeout(clearWindow, SEQUENCE_WINDOW_MS);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (matchesShortcut(event, PALETTE_SHORTCUT)) {
        event.preventDefault();
        togglePalette();
        return;
      }
      if (!pageShortcuts) return;
      if (isTypingTarget(event.target)) return;
      const bare = !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey;
      if (bare && event.key.length === 1) {
        // Bare keys accumulate a two-key sequence first: a registered
        // sequence wins over any single-key chord on its second key
        // (SHELL-090: "op" still opens the project picker on entity
        // screens with a bare "p" command). Single-key chords fire only
        // as the first accumulated key.
        sequence = `${sequence}${event.key.toLowerCase()}`.slice(-2);
        const sequenced = findCommandBySequence(sequence);
        if (sequenced) {
          event.preventDefault();
          clearWindow();
          fireCommand(sequenced, { openPalette, openPage });
          return;
        }
        if (sequence.length === 1) {
          const chord = listCommands().find(
            (command) => command.shortcut !== undefined && matchesShortcut(event, command.shortcut)
          );
          if (chord) {
            event.preventDefault();
            clearWindow();
            fireCommand(chord, { openPalette, openPage });
            return;
          }
        }
        armWindow();
        return;
      }
      const chord = listCommands().find(
        (command) => command.shortcut !== undefined && matchesShortcut(event, command.shortcut)
      );
      if (chord) {
        event.preventDefault();
        fireCommand(chord, { openPalette, openPage });
      }
    };

    globalThis.window.addEventListener("keydown", onKeyDown);
    return () => {
      globalThis.window.removeEventListener("keydown", onKeyDown);
      clearWindow();
    };
  }, [togglePalette, openPalette, openPage, pageShortcuts]);
}
