// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { selectCycleContext } from "./cycles.js";
import { selectModuleContext } from "./modules.js";
import type { ModuleContextInput } from "./modules.js";
import { selectPageCapabilities, selectPageContext } from "./pages.js";

describe("selectCycleContext", () => {
  it("offers the favorite toggle plus copy-URL to editors", () => {
    const toggleFavorite = vi.fn();
    const copyUrl = vi.fn();
    const commands = selectCycleContext({
      cycleId: "cycle-1",
      cycleName: "Sprint 1",
      archived: false,
      favorite: false,
      standing: { memberRole: 15, archived: false },
      toggleFavorite,
      copyUrl,
    });
    expect(commands.map((command) => command.title)).toEqual(["Add to favorites", "Copy URL"]);
    expect(commands[0]!.section).toBe("Cycle actions");
    expect(commands[0]!.shortcut).toBe("shift+f");
    expect(commands[1]!.shortcut).toBe("mod+shift+,");
    expect(commands[0]!.context).toEqual({ kind: "cycle", indicator: "Cycle", subject: "Sprint 1" });
    commands[0]!.run();
    commands[1]!.run();
    expect(toggleFavorite).toHaveBeenCalledTimes(1);
    expect(copyUrl).toHaveBeenCalledTimes(1);
  });

  it("flips the favorite title and hides it for guests and archived cycles", () => {
    const starred = selectCycleContext({
      cycleId: "cycle-1",
      cycleName: "Sprint 1",
      archived: false,
      favorite: true,
      standing: { memberRole: 20, archived: false },
      toggleFavorite: () => undefined,
      copyUrl: () => undefined,
    });
    expect(starred[0]!.title).toBe("Remove from favorites");
    for (const over of [
      { standing: { memberRole: 5, archived: false }, archived: false },
      { standing: { memberRole: null, archived: false }, archived: false },
      { standing: { memberRole: 20, archived: false }, archived: true },
      { standing: { memberRole: 20, archived: true }, archived: false },
    ] as const) {
      const commands = selectCycleContext({
        cycleId: "cycle-1",
        cycleName: "Sprint 1",
        favorite: false,
        toggleFavorite: () => undefined,
        copyUrl: () => undefined,
        ...over,
      });
      expect(commands.map((command) => command.title)).toEqual(["Copy URL"]);
    }
  });
});

describe("selectModuleContext", () => {
  function moduleInput(overrides: Partial<ModuleContextInput> = {}): ModuleContextInput {
    return {
      moduleId: "module-1",
      moduleName: "Auth",
      archived: false,
      status: "backlog",
      memberIds: [] as string[],
      favorite: false,
      standing: { memberRole: 15, archived: false },
      members: [{ userId: "user-1", role: 15, displayName: "Ada" }],
      membersLoading: false,
      toggleMember: () => undefined,
      setStatus: () => undefined,
      toggleFavorite: () => undefined,
      copyUrl: () => undefined,
      close: () => undefined,
      ...overrides,
    };
  }

  it("offers members, status, favorite and copy-URL to editors", () => {
    const { commands, pages } = selectModuleContext(moduleInput());
    expect(commands.map((command) => command.title)).toEqual([
      "Add/remove members",
      "Change status",
      "Add to favorites",
      "Copy URL",
    ]);
    expect(commands[0]!.section).toBe("Module actions");
    expect(pages.map((page) => page.placeholder)).toEqual(["Change members", "Change status"]);
    expect(commands[0]!.context).toEqual({ kind: "module", indicator: "Module", subject: "Auth" });
  });

  it("toggles members and statuses through the pickers", () => {
    const toggleMember = vi.fn();
    const setStatus = vi.fn();
    const { pages } = selectModuleContext(moduleInput({ toggleMember, setStatus }));
    const members = pages.find((page) => page.placeholder === "Change members")!;
    members.usePageState("").options[0]!.run();
    expect(toggleMember).toHaveBeenCalledWith("user-1", false);
    expect(members.usePageState("zzz").options).toHaveLength(0);
    expect(members.usePageState("").status).toBe("ready");
    const status = pages.find((page) => page.placeholder === "Change status")!;
    status
      .usePageState("")
      .options.find((option) => option.id === "backlog")!
      .run();
    expect(setStatus).not.toHaveBeenCalled();
    status
      .usePageState("")
      .options.find((option) => option.id === "in-progress")!
      .run();
    expect(setStatus).toHaveBeenCalledWith("in-progress");
    expect(status.usePageState("").options.find((option) => option.id === "in-progress")!.title).toBe("In Progress");
  });

  it("spins the member picker while loading and hides edits when gated", () => {
    const { pages } = selectModuleContext(moduleInput({ membersLoading: true }));
    expect(pages.find((page) => page.placeholder === "Change members")!.usePageState("").status).toBe("loading");
    const gated = selectModuleContext(moduleInput({ standing: { memberRole: 5, archived: false } }));
    expect(gated.commands.map((command) => command.title)).toEqual(["Copy URL"]);
    expect(gated.pages).toEqual([]);
    const archived = selectModuleContext(moduleInput({ archived: true }));
    expect(archived.commands.map((command) => command.title)).toEqual(["Copy URL"]);
  });

  it("runs the favorite toggle and copy", () => {
    const toggleFavorite = vi.fn();
    const copyUrl = vi.fn();
    const { commands } = selectModuleContext(moduleInput({ favorite: true, toggleFavorite, copyUrl }));
    expect(commands.map((command) => command.title)).toContain("Remove from favorites");
    commands.find((command) => command.title === "Remove from favorites")!.run();
    commands.find((command) => command.title === "Copy URL")!.run();
    expect(toggleFavorite).toHaveBeenCalledTimes(1);
    expect(copyUrl).toHaveBeenCalledTimes(1);
  });

  it("marks every command with the module context, including copy-URL", () => {
    for (const selection of [
      selectModuleContext(moduleInput()),
      selectModuleContext(moduleInput({ standing: { memberRole: 5, archived: false } })),
    ]) {
      expect(selection.commands.length).toBeGreaterThan(0);
      expect(
        selection.commands.every(
          (command) =>
            command.context?.kind === "module" &&
            command.context.indicator === "Module" &&
            command.context.subject === "Auth"
        )
      ).toBe(true);
    }
  });

  it("closes on status picks and stays open on member toggles", () => {
    const close = vi.fn();
    const { pages } = selectModuleContext(moduleInput({ close }));
    pages
      .find((page) => page.placeholder === "Change status")!
      .usePageState("")
      .options.find((option) => option.id === "in-progress")!
      .run();
    expect(close).toHaveBeenCalledTimes(1);
    close.mockClear();
    pages
      .find((page) => page.placeholder === "Change status")!
      .usePageState("")
      .options.find((option) => option.id === "backlog")!
      .run();
    expect(close).toHaveBeenCalledTimes(1);
    close.mockClear();
    pages
      .find((page) => page.placeholder === "Change members")!
      .usePageState("")
      .options[0]!.run();
    expect(close).not.toHaveBeenCalled();
  });
});

describe("selectPageContext", () => {
  function pageInput(overrides = {}) {
    return {
      pageId: "page-1",
      pageName: "Roadmap",
      locked: false,
      access: 0,
      archived: false,
      favorite: false,
      capabilities: { lock: true, changeAccess: true, archive: true, favorite: true },
      setLocked: () => undefined,
      setAccess: () => undefined,
      setArchived: () => undefined,
      toggleFavorite: () => undefined,
      copyUrl: () => undefined,
      ...overrides,
    };
  }

  it("offers every toggle plus copy-URL at full capability", () => {
    const commands = selectPageContext(pageInput());
    expect(commands.map((command) => command.title)).toEqual([
      "Lock",
      "Make private",
      "Archive",
      "Add to favorites",
      "Copy URL",
    ]);
    expect(commands[0]!.section).toBe("Page actions");
    expect(commands.map((command) => command.shortcut)).toEqual([
      "shift+l",
      "shift+a",
      "shift+r",
      "shift+f",
      "mod+shift+,",
    ]);
    expect(commands[0]!.context).toEqual({ kind: "page", indicator: "Page", subject: "Roadmap" });
  });

  it("flips every toggle title from state and invokes the writes", () => {
    const setLocked = vi.fn();
    const setAccess = vi.fn();
    const setArchived = vi.fn();
    const toggleFavorite = vi.fn();
    const commands = selectPageContext(
      pageInput({
        locked: true,
        access: 1,
        archived: true,
        favorite: true,
        setLocked,
        setAccess,
        setArchived,
        toggleFavorite,
      })
    );
    expect(commands.map((command) => command.title)).toEqual([
      "Unlock",
      "Make public",
      "Restore",
      "Remove from favorites",
      "Copy URL",
    ]);
    commands[0]!.run();
    commands[1]!.run();
    commands[2]!.run();
    commands[3]!.run();
    expect(setLocked).toHaveBeenCalledWith(false);
    expect(setAccess).toHaveBeenCalledWith(0);
    expect(setArchived).toHaveBeenCalledWith(false);
    expect(toggleFavorite).toHaveBeenCalledTimes(1);
  });

  it("locks and archives from the unlocked state", () => {
    const setLocked = vi.fn();
    const setArchived = vi.fn();
    const commands = selectPageContext(pageInput({ setLocked, setArchived }));
    commands[0]!.run();
    commands[2]!.run();
    expect(setLocked).toHaveBeenCalledWith(true);
    expect(setArchived).toHaveBeenCalledWith(true);
  });

  it("makes public pages private and copies the URL", () => {
    const setAccess = vi.fn();
    const copyUrl = vi.fn();
    const commands = selectPageContext(pageInput({ access: 0, setAccess, copyUrl }));
    expect(commands.map((command) => command.title)).toContain("Make private");
    commands.find((command) => command.title === "Make private")!.run();
    expect(setAccess).toHaveBeenCalledWith(1);
    commands.find((command) => command.title === "Copy URL")!.run();
    expect(copyUrl).toHaveBeenCalledTimes(1);
  });

  it("shows only the allowed entries", () => {
    const commands = selectPageContext(
      pageInput({ capabilities: { lock: false, changeAccess: false, archive: false, favorite: true } })
    );
    expect(commands.map((command) => command.title)).toEqual(["Add to favorites", "Copy URL"]);
    const none = selectPageContext(
      pageInput({ capabilities: { lock: false, changeAccess: false, archive: false, favorite: false } })
    );
    expect(none.map((command) => command.title)).toEqual(["Copy URL"]);
  });
});

describe("entity context markers", () => {
  it("marks every cycle and page command with its context, including copy-URL", () => {
    const cycles = selectCycleContext({
      cycleId: "cycle-1",
      cycleName: "Sprint 1",
      archived: false,
      favorite: false,
      standing: { memberRole: 15, archived: false },
      toggleFavorite: () => undefined,
      copyUrl: () => undefined,
    });
    expect(cycles.length).toBeGreaterThan(0);
    expect(cycles.every((command) => command.context?.kind === "cycle")).toBe(true);
    const pages = selectPageContext({
      pageId: "page-1",
      pageName: "Roadmap",
      locked: false,
      access: 0,
      archived: false,
      favorite: false,
      capabilities: { lock: true, changeAccess: true, archive: true, favorite: true },
      setLocked: () => undefined,
      setAccess: () => undefined,
      setArchived: () => undefined,
      toggleFavorite: () => undefined,
      copyUrl: () => undefined,
    });
    expect(pages.length).toBeGreaterThan(0);
    expect(pages.every((command) => command.context?.kind === "page")).toBe(true);
  });
});

describe("selectPageCapabilities", () => {
  it("grants management to owners and project admins, favorites to members", () => {
    expect(selectPageCapabilities({ isOwner: false, highestProjectRole: 20 })).toEqual({
      lock: true,
      changeAccess: true,
      archive: true,
      favorite: true,
    });
    expect(selectPageCapabilities({ isOwner: true, highestProjectRole: 5 })).toEqual({
      lock: true,
      changeAccess: true,
      archive: true,
      favorite: true,
    });
    expect(selectPageCapabilities({ isOwner: false, highestProjectRole: 15 })).toEqual({
      lock: false,
      changeAccess: false,
      archive: false,
      favorite: true,
    });
    expect(selectPageCapabilities({ isOwner: false, highestProjectRole: 5 })).toEqual({
      lock: false,
      changeAccess: false,
      archive: false,
      favorite: false,
    });
    expect(selectPageCapabilities({ isOwner: false, highestProjectRole: null })).toEqual({
      lock: false,
      changeAccess: false,
      archive: false,
      favorite: false,
    });
  });
});
