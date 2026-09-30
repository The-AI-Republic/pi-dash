// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it } from "vitest";
import {
  clearCommands,
  findCommand,
  listCommands,
  matchesShortcut,
  parseShortcut,
  registerCommands,
} from "./registry.js";
import { useCommandPalette } from "./store.js";

function keyEvent(key: string, modifiers: Partial<Record<"ctrl" | "meta" | "shift" | "alt", boolean>> = {}) {
  return {
    key,
    ctrlKey: modifiers.ctrl ?? false,
    metaKey: modifiers.meta ?? false,
    shiftKey: modifiers.shift ?? false,
    altKey: modifiers.alt ?? false,
  };
}

describe("command registry", () => {
  it("registers, lists and unregisters by owner", () => {
    clearCommands();
    const unregister = registerCommands([
      { id: "shell.sign-out", title: "Sign out", section: "Account", run: () => undefined },
      { id: "issues.go-to-list", title: "Go to issues", run: () => undefined },
    ]);
    expect(listCommands().map((command) => command.id)).toEqual(["shell.sign-out", "issues.go-to-list"]);
    expect(findCommand("shell.sign-out")?.title).toBe("Sign out");
    unregister();
    expect(listCommands()).toEqual([]);
  });

  it("replacing an id keeps the newest entry until its own cleanup", () => {
    clearCommands();
    const first = registerCommands([{ id: "x", title: "First", run: () => undefined }]);
    const second = registerCommands([{ id: "x", title: "Second", run: () => undefined }]);
    expect(findCommand("x")?.title).toBe("Second");
    second();
    expect(findCommand("x")).toBeUndefined();
    first();
    expect(listCommands()).toEqual([]);
  });

  it("parses single chords and rejects the rest", () => {
    expect(parseShortcut("mod+k")).toEqual({ mod: true, shift: false, alt: false, key: "k" });
    expect(parseShortcut("Mod+Shift+P")).toEqual({ mod: true, shift: true, alt: false, key: "p" });
    expect(parseShortcut("")).toBeNull();
    expect(parseShortcut("g i")).toBeNull();
    expect(parseShortcut("mod+mod+k")).toBeNull();
    expect(parseShortcut("cmd+k")).toBeNull();
  });

  it("matches mod on either ctrl or meta", () => {
    expect(matchesShortcut(keyEvent("k", { meta: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("k", { ctrl: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("K", { ctrl: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("k"), "mod+k")).toBe(false);
    expect(matchesShortcut(keyEvent("k", { ctrl: true, shift: true }), "mod+k")).toBe(false);
    expect(matchesShortcut(keyEvent("k", { ctrl: true }), "nonsense-chord")).toBe(false);
  });
});

describe("command palette store", () => {
  it("opens, filters and closes with a cleared query", () => {
    useCommandPalette.getState().openPalette();
    expect(useCommandPalette.getState().open).toBe(true);
    useCommandPalette.getState().setQuery("issue");
    expect(useCommandPalette.getState().query).toBe("issue");
    useCommandPalette.getState().togglePalette();
    expect(useCommandPalette.getState().open).toBe(false);
    expect(useCommandPalette.getState().query).toBe("");
  });
});
