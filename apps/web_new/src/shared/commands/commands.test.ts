// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";
import {
  clearCommands,
  clearPalettePages,
  filterCommands,
  filterPageOptions,
  findCommand,
  findCommandBySequence,
  findPalettePage,
  listBoundCommands,
  listCommands,
  matchesShortcut,
  parseShortcut,
  registerCommands,
  registerPalettePages,
} from "./registry.js";
import { resetRegisteredStores } from "../../core/session/store.js";
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
  afterEach(() => {
    clearCommands();
    clearPalettePages();
  });

  it("registers, lists and unregisters by owner", () => {
    const unregister = registerCommands([
      { id: "shell.sign-out", title: "Sign out", section: "Account", run: () => undefined },
      { id: "issues.go-to-list", title: "Go to issues", run: () => undefined },
    ]);
    // General outranks Account in the pinned palette order (SHELL-087).
    expect(listCommands().map((command) => command.id)).toEqual(["issues.go-to-list", "shell.sign-out"]);
    expect(findCommand("shell.sign-out")?.title).toBe("Sign out");
    unregister();
    expect(listCommands()).toEqual([]);
  });

  it("replacing an id keeps the newest entry until its own cleanup", () => {
    const first = registerCommands([{ id: "x", title: "First", run: () => undefined }]);
    const second = registerCommands([{ id: "x", title: "Second", run: () => undefined }]);
    expect(findCommand("x")?.title).toBe("Second");
    second();
    expect(findCommand("x")).toBeUndefined();
    first();
    expect(listCommands()).toEqual([]);
  });

  it("orders sections by the pinned palette order, then title", () => {
    registerCommands([
      { id: "help", title: "Help", section: "Help", run: () => undefined },
      { id: "account", title: "Account", section: "Account", run: () => undefined },
      { id: "create-b", title: "Create b", section: "Create", run: () => undefined },
      { id: "create-a", title: "Create a", section: "Create", run: () => undefined },
      { id: "context", title: "Context", section: "contextual", run: () => undefined },
    ]);
    expect(listCommands().map((command) => command.id)).toEqual(["context", "create-a", "create-b", "account", "help"]);
  });

  it("sorts unknown sections alphabetically after the pinned ones", () => {
    registerCommands([
      { id: "help", title: "Help", section: "Help", run: () => undefined },
      { id: "zeta", title: "Zeta", section: "Zeta", run: () => undefined },
      { id: "alpha", title: "Alpha", section: "Alpha", run: () => undefined },
    ]);
    expect(listCommands().map((command) => command.id)).toEqual(["help", "alpha", "zeta"]);
  });

  it("ranks context-marked commands ahead of every generic group", () => {
    const context = { kind: "work-item" as const, indicator: "Work item", subject: "ACME-1" };
    registerCommands([
      { id: "create", title: "Create", section: "Create", run: () => undefined },
      {
        id: "ctx.copy",
        title: "Copy URL",
        section: "Work item actions",
        context,
        run: () => undefined,
      },
      {
        id: "ctx.state",
        title: "Change state",
        section: "Work item actions",
        context,
        run: () => undefined,
      },
    ]);
    expect(listCommands().map((command) => command.id)).toEqual(["ctx.state", "ctx.copy", "create"]);
  });

  it("parses single chords and rejects the rest", () => {
    expect(parseShortcut("mod+k")).toEqual({ mod: true, shift: false, alt: false, key: "k" });
    expect(parseShortcut("Mod+Shift+P")).toEqual({ mod: true, shift: true, alt: false, key: "p" });
    expect(parseShortcut("mod+Backspace")).toEqual({ mod: true, shift: false, alt: false, key: "backspace" });
    expect(parseShortcut("")).toBeNull();
    expect(parseShortcut("g i")).toBeNull();
    expect(parseShortcut("mod+mod+k")).toBeNull();
    expect(parseShortcut("cmd+k")).toBeNull();
    expect(parseShortcut("mod+spacebar")).toBeNull();
  });

  it("matches mod on either ctrl or meta", () => {
    expect(matchesShortcut(keyEvent("k", { meta: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("k", { ctrl: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("K", { ctrl: true }), "mod+k")).toBe(true);
    expect(matchesShortcut(keyEvent("k"), "mod+k")).toBe(false);
    expect(matchesShortcut(keyEvent("k", { ctrl: true, shift: true }), "mod+k")).toBe(false);
    expect(matchesShortcut(keyEvent("k", { ctrl: true }), "nonsense-chord")).toBe(false);
  });

  it("matches named non-printable keys case-insensitively", () => {
    expect(matchesShortcut(keyEvent("Backspace", { meta: true }), "mod+backspace")).toBe(true);
    expect(matchesShortcut(keyEvent("Backspace", { ctrl: true }), "mod+backspace")).toBe(true);
    expect(matchesShortcut(keyEvent("Backspace"), "mod+backspace")).toBe(false);
    expect(matchesShortcut(keyEvent("Delete", { meta: true }), "mod+backspace")).toBe(false);
  });
});

describe("palette page registry", () => {
  afterEach(() => {
    clearPalettePages();
  });

  it("registers, finds and unregisters pages by owner", () => {
    const page = {
      id: "issues.picker",
      placeholder: "Pick",
      usePageState: () => ({ status: "ready" as const, options: [], emptyText: "None" }),
    };
    const unregister = registerPalettePages([page]);
    expect(findPalettePage("issues.picker")).toBe(page);
    expect(findPalettePage("missing")).toBeUndefined();
    unregister();
    expect(findPalettePage("issues.picker")).toBeUndefined();
  });

  it("replacing an id keeps the newest entry until its own cleanup", () => {
    const first = registerPalettePages([
      { id: "x", placeholder: "First", usePageState: () => ({ status: "ready" as const, options: [], emptyText: "" }) },
    ]);
    const second = registerPalettePages([
      {
        id: "x",
        placeholder: "Second",
        usePageState: () => ({ status: "ready" as const, options: [], emptyText: "" }),
      },
    ]);
    expect(findPalettePage("x")?.placeholder).toBe("Second");
    second();
    expect(findPalettePage("x")).toBeUndefined();
    first();
    expect(findPalettePage("x")).toBeUndefined();
  });
});

describe("filterCommands", () => {
  it("returns the snapshot unchanged on a blank query", () => {
    const commands = [
      { id: "a", title: "Alpha", run: () => undefined },
      { id: "b", title: "Beta", run: () => undefined },
    ];
    expect(filterCommands(commands, "   ")).toEqual(commands);
  });

  it("matches titles, hints and keywords case-insensitively", () => {
    const commands = [
      { id: "sign-out", title: "Sign out", keywords: ["logout", "exit"], run: () => undefined },
      { id: "go-home", title: "Go to workspace home", hint: "Acme", run: () => undefined },
      { id: "other", title: "Unrelated", run: () => undefined },
    ];
    expect(filterCommands(commands, "SIGN").map((command) => command.id)).toEqual(["sign-out"]);
    expect(filterCommands(commands, "logo").map((command) => command.id)).toEqual(["sign-out"]);
    expect(filterCommands(commands, "acme").map((command) => command.id)).toEqual(["go-home"]);
    expect(filterCommands(commands, "nothing-matches").map((command) => command.id)).toEqual([]);
  });

  it("finds sequenced commands case-insensitively, and nothing unregistered", () => {
    const unregister = registerCommands([
      { id: "shell.open-project", title: "Open a project", sequence: "op", run: () => undefined },
      { id: "shell.plain", title: "Plain", run: () => undefined },
    ]);
    try {
      expect(findCommandBySequence("op")?.id).toBe("shell.open-project");
      expect(findCommandBySequence("OP")?.id).toBe("shell.open-project");
      expect(findCommandBySequence("zz")).toBeUndefined();
    } finally {
      unregister();
    }
  });

  it("filters page options by title and hint, blank returns all", () => {
    const options = [
      { id: "a", title: "Website relaunch", hint: "WEB", run: () => undefined },
      { id: "b", title: "Mobile app", run: () => undefined },
    ];
    expect(filterPageOptions(options, "   ").map((option) => option.id)).toEqual(["a", "b"]);
    expect(filterPageOptions(options, "web").map((option) => option.id)).toEqual(["a"]);
    expect(filterPageOptions(options, "mobile").map((option) => option.id)).toEqual(["b"]);
    expect(filterPageOptions(options, "zzz")).toEqual([]);
  });

  it("lists only chord- or sequence-bound commands, in palette order", () => {
    const unregister = registerCommands([
      { id: "help.bound", title: "Bound", section: "Help", shortcut: "mod+/", run: () => undefined },
      { id: "plain", title: "Plain", run: () => undefined },
      { id: "create.bound", title: "Bound", section: "Create", sequence: "np", run: () => undefined },
    ]);
    try {
      // Create outranks Help in the pinned palette order (SHELL-087).
      expect(listBoundCommands().map((command) => command.id)).toEqual(["create.bound", "help.bound"]);
    } finally {
      unregister();
    }
  });
});

describe("command palette store", () => {
  afterEach(() => {
    useCommandPalette.getState().closePalette();
  });

  it("opens, filters and closes with a cleared query", () => {
    useCommandPalette.getState().openPalette();
    expect(useCommandPalette.getState().open).toBe(true);
    useCommandPalette.getState().setQuery("issue");
    expect(useCommandPalette.getState().query).toBe("issue");
    useCommandPalette.getState().togglePalette();
    expect(useCommandPalette.getState().open).toBe(false);
    expect(useCommandPalette.getState().query).toBe("");
  });

  it("steps into and back from sub-pages with a cleared query", () => {
    const store = useCommandPalette.getState();
    store.openPalette();
    store.setQuery("proj");
    store.openPage("issues.picker");
    expect(useCommandPalette.getState().page).toBe("issues.picker");
    expect(useCommandPalette.getState().query).toBe("");
    useCommandPalette.getState().setQuery("at");
    useCommandPalette.getState().closePage();
    expect(useCommandPalette.getState().page).toBeNull();
    expect(useCommandPalette.getState().query).toBe("");
  });

  it("resets the page and the search scope on sign-out", () => {
    const store = useCommandPalette.getState();
    store.openPalette();
    store.setQuery("par");
    store.openPage("issues.picker");
    store.setWorkspaceSearch(true);
    resetRegisteredStores();
    expect(useCommandPalette.getState()).toMatchObject({ open: false, query: "", page: null, workspaceSearch: false });
  });

  it("resets the page and the search scope on every close and open", () => {
    const store = useCommandPalette.getState();
    store.openPalette();
    store.openPage("issues.picker");
    store.setWorkspaceSearch(true);
    useCommandPalette.getState().closePalette();
    expect(useCommandPalette.getState()).toMatchObject({ open: false, query: "", page: null, workspaceSearch: false });
    useCommandPalette.getState().openPalette();
    expect(useCommandPalette.getState()).toMatchObject({ open: true, query: "", page: null, workspaceSearch: false });
    useCommandPalette.getState().setWorkspaceSearch(true);
    useCommandPalette.getState().togglePalette();
    expect(useCommandPalette.getState()).toMatchObject({ open: false, query: "", page: null, workspaceSearch: false });
  });
});
