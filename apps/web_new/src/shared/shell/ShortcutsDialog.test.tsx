// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shortcut reference dialog (shell spec, SHELL-094).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { registerCommands, type PidashCommand } from "../commands/index.js";
import { formatChordKeys, groupBoundCommands, isMacPlatform, ShortcutsDialog } from "./ShortcutsDialog.js";
import { useShortcutsDialog } from "./shortcutsDialogStore.js";

function command(overrides: Partial<PidashCommand> & { id: string; title: string }): PidashCommand {
  return { run: () => undefined, ...overrides };
}

afterEach(() => {
  useShortcutsDialog.getState().closeShortcuts();
});

describe("chord glyphs", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("adapts chord glyphs to the platform", () => {
    expect(formatChordKeys("mod+shift+c", true)).toEqual(["⌘", "⇧", "C"]);
    expect(formatChordKeys("mod+shift+c", false)).toEqual(["Ctrl", "Shift", "C"]);
    expect(formatChordKeys("ctrl+alt+/", true)).toEqual(["⌃", "⌥", "/"]);
    expect(formatChordKeys("mod+k", false)).toEqual(["Ctrl", "K"]);
  });

  it("renders unknown chord parts uppercased instead of vanishing", () => {
    expect(formatChordKeys("bogus", false)).toEqual(["BOGUS"]);
    expect(formatChordKeys("mod+ ", true)).toEqual(["⌘", "Space"]);
  });

  it("detects macOS from the platform string", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    expect(isMacPlatform()).toBe(true);
    vi.stubGlobal("navigator", { platform: "Win32" });
    expect(isMacPlatform()).toBe(false);
  });

  it("treats a missing navigator as non-Mac", () => {
    vi.stubGlobal("navigator", undefined);
    expect(isMacPlatform()).toBe(false);
  });

  it("treats a navigator without a platform string as non-Mac", () => {
    vi.stubGlobal("navigator", {});
    expect(isMacPlatform()).toBe(false);
  });

  it("covers every modifier alias on both platforms", () => {
    expect(formatChordKeys("meta+cmd+option", true)).toEqual(["⌘", "⌘", "⌥"]);
    expect(formatChordKeys("ctrl+alt", false)).toEqual(["Ctrl", "Alt"]);
  });
});

describe("groupBoundCommands", () => {
  it("groups consecutive families and defaults the section", () => {
    const groups = groupBoundCommands(
      [
        command({ id: "a", title: "Go to home", section: "Navigate", sequence: "gh" }),
        command({ id: "b", title: "Go away", section: "Navigate", sequence: "ga" }),
        command({ id: "c", title: "Copy", shortcut: "mod+shift+c" }),
      ],
      ""
    );
    expect(groups.map((group) => group.section)).toEqual(["Navigate", "General"]);
    expect(groups[0]!.commands.map((entry) => entry.id)).toEqual(["a", "b"]);
  });

  it("filters by title substring, dropping empty families", () => {
    const groups = groupBoundCommands(
      [
        command({ id: "a", title: "Go to home", section: "Navigate", sequence: "gh" }),
        command({ id: "b", title: "Copy current page URL", section: "Miscellaneous", shortcut: "mod+shift+c" }),
      ],
      "  HOME "
    );
    expect(groups.map((group) => group.section)).toEqual(["Navigate"]);
    expect(groupBoundCommands([], "")).toEqual([]);
  });
});

describe("ShortcutsDialog", () => {
  it("lists bound commands grouped by family with their bindings", () => {
    const unregister = registerCommands([
      command({ id: "shell.go-home", title: "Go to home", section: "Navigate", sequence: "gh" }),
      command({
        id: "shell.copy-url",
        title: "Copy current page URL",
        section: "Miscellaneous",
        shortcut: "mod+shift+c",
      }),
      command({ id: "shell.sign-out", title: "Sign out", section: "Account" }),
    ]);
    try {
      useShortcutsDialog.getState().openShortcuts();
      render(<ShortcutsDialog />);
      expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).not.toBeNull();
      expect(screen.getByText("Navigate")).not.toBeNull();
      expect(screen.getByText("Miscellaneous")).not.toBeNull();
      expect(screen.getByText("Go to home")).not.toBeNull();
      expect(screen.getByText("Copy current page URL")).not.toBeNull();
      // Unbound commands stay out of the reference.
      expect(screen.queryByText("Sign out")).toBeNull();
      // Sequence chips and platform-adapted chord glyphs.
      expect(screen.getByLabelText("G then H")).not.toBeNull();
      expect(screen.getByLabelText("Ctrl+Shift+C")).not.toBeNull();
    } finally {
      unregister();
    }
  });

  it("narrows to the matching family on filter, then clears the filter on close", async () => {
    const unregister = registerCommands([
      command({ id: "shell.go-home", title: "Go to home", section: "Navigate", sequence: "gh" }),
      command({
        id: "shell.copy-url",
        title: "Copy current page URL",
        section: "Miscellaneous",
        shortcut: "mod+shift+c",
      }),
    ]);
    try {
      useShortcutsDialog.getState().openShortcuts();
      render(<ShortcutsDialog />);
      fireEvent.change(screen.getByPlaceholderText("Search for shortcuts"), { target: { value: "home" } });
      expect(screen.getByText("Go to home")).not.toBeNull();
      expect(screen.queryByText("Copy current page URL")).toBeNull();
      expect(screen.queryByText("Miscellaneous")).toBeNull();
      fireEvent.change(screen.getByPlaceholderText("Search for shortcuts"), { target: { value: "zzz" } });
      expect(screen.getByText("No matching shortcuts.")).not.toBeNull();
      // Closing clears the filter, so reopening shows everything again.
      fireEvent.click(screen.getByRole("button", { name: "Close dialog" }));
      expect(useShortcutsDialog.getState().open).toBe(false);
      useShortcutsDialog.getState().openShortcuts();
      await waitFor(() => expect(screen.getByText("Go to home")).not.toBeNull());
      expect(screen.getByText("Copy current page URL")).not.toBeNull();
    } finally {
      unregister();
    }
  });

  it("renders no groups while closed", () => {
    const unregister = registerCommands([
      command({ id: "shell.go-home", title: "Go to home", section: "Navigate", sequence: "gh" }),
    ]);
    try {
      render(<ShortcutsDialog />);
      expect(screen.queryByText("Go to home")).toBeNull();
    } finally {
      unregister();
    }
  });
});
