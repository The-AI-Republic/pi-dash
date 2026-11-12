// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  QUICKSTART_ACTIONS,
  QUICKSTART_CARD_IDS,
  QUICKSTART_TITLES,
  canShowQuickstartAction,
  countJoinedProjects,
  isQuickstartCardDone,
  isQuickstartPanelVisible,
  quickstartLinkHref,
  quickstartStorageKey,
  quickstartVisitFlag,
  readQuickstartFlags,
  resolveQuickstartStorage,
  writeQuickstartFlags,
  type QuickstartCardId,
  type QuickstartFlags,
} from "./quickstart.js";

const FLAGS: QuickstartFlags = { hide: false, visited_workspace: false, visited_profile: false };

function memoryStorage(entries: Record<string, string> = {}): Pick<Storage, "getItem" | "setItem"> {
  const store = new Map<string, string>(Object.entries(entries));
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, value);
    },
  };
}

describe("quickstart copy", () => {
  it("pins the oracle titles and actions for all four cards", () => {
    expect(QUICKSTART_CARD_IDS).toEqual([
      "create-project",
      "invite-team",
      "configure-workspace",
      "personalize-account",
    ]);
    expect(QUICKSTART_TITLES["create-project"]).toMatch(/create a project/i);
    expect(QUICKSTART_TITLES["invite-team"]).toMatch(/invite your team/i);
    expect(QUICKSTART_TITLES["configure-workspace"]).toMatch(/set up your workspace/i);
    expect(QUICKSTART_TITLES["personalize-account"]).toMatch(/make pi dash yours/i);
    expect(QUICKSTART_ACTIONS["create-project"]).toMatch(/get started/i);
    expect(QUICKSTART_ACTIONS["invite-team"]).toMatch(/get them in/i);
    expect(QUICKSTART_ACTIONS["configure-workspace"]).toMatch(/configure this workspace/i);
    expect(QUICKSTART_ACTIONS["personalize-account"]).toMatch(/personalize now/i);
  });
});

describe("quickstartStorageKey", () => {
  it("scopes the record per workspace", () => {
    expect(quickstartStorageKey("acme")).toBe("web_new.home.quickstart.acme");
    expect(quickstartStorageKey("beta")).not.toBe(quickstartStorageKey("acme"));
  });
});

describe("resolveQuickstartStorage", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns null outside the browser", () => {
    expect(resolveQuickstartStorage()).toBeNull();
  });

  it("resolves the browser store when available", () => {
    const storage = memoryStorage();
    vi.stubGlobal("window", { localStorage: storage });
    expect(resolveQuickstartStorage()).toBe(storage);
  });

  it("returns null when the store throws", () => {
    vi.stubGlobal("window", {
      get localStorage(): Storage {
        throw new Error("denied");
      },
    });
    expect(resolveQuickstartStorage()).toBeNull();
  });
});

describe("readQuickstartFlags", () => {
  it("defaults without a store", () => {
    expect(readQuickstartFlags(null, "acme")).toEqual(FLAGS);
    expect(readQuickstartFlags(undefined, "acme")).toEqual(FLAGS);
  });

  it("defaults when the record is missing", () => {
    expect(readQuickstartFlags(memoryStorage(), "acme")).toEqual(FLAGS);
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": "" }), "acme")).toEqual(FLAGS);
  });

  it("reads a stored record", () => {
    const storage = memoryStorage({
      "web_new.home.quickstart.acme": JSON.stringify({
        hide: true,
        visited_workspace: true,
        visited_profile: false,
      }),
    });
    expect(readQuickstartFlags(storage, "acme")).toEqual({
      hide: true,
      visited_workspace: true,
      visited_profile: false,
    });
  });

  it("treats partial and mistyped records as unset fields", () => {
    const storage = memoryStorage({
      "web_new.home.quickstart.acme": JSON.stringify({ hide: "yes", visited_profile: 1 }),
    });
    expect(readQuickstartFlags(storage, "acme")).toEqual(FLAGS);
  });

  it("defaults on corrupt and non-object records", () => {
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": "{oops" }), "acme")).toEqual(FLAGS);
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": "null" }), "acme")).toEqual(FLAGS);
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": "[true]" }), "acme")).toEqual(FLAGS);
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": '"just-a-string"' }), "acme")).toEqual(
      FLAGS
    );
    expect(readQuickstartFlags(memoryStorage({ "web_new.home.quickstart.acme": "42" }), "acme")).toEqual(FLAGS);
  });

  it("defaults when the store throws", () => {
    const storage: Pick<Storage, "getItem" | "setItem"> = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {},
    };
    expect(readQuickstartFlags(storage, "acme")).toEqual(FLAGS);
  });
});

describe("writeQuickstartFlags", () => {
  it("round-trips a record", () => {
    const storage = memoryStorage();
    const flags: QuickstartFlags = { hide: true, visited_workspace: true, visited_profile: false };
    writeQuickstartFlags(storage, "acme", flags);
    expect(readQuickstartFlags(storage, "acme")).toEqual(flags);
  });

  it("ignores a missing or throwing store", () => {
    const flags: QuickstartFlags = { hide: true, visited_workspace: false, visited_profile: false };
    expect(() => writeQuickstartFlags(null, "acme", flags)).not.toThrow();
    expect(() => writeQuickstartFlags(undefined, "acme", flags)).not.toThrow();
    const storage: Pick<Storage, "getItem" | "setItem"> = {
      getItem: () => null,
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(() => writeQuickstartFlags(storage, "acme", flags)).not.toThrow();
  });
});

describe("quickstartLinkHref", () => {
  it("targets members, settings and the global profile page", () => {
    expect(quickstartLinkHref("invite-team", "acme")).toBe("/acme/settings/members");
    expect(quickstartLinkHref("invite-team", "acme")).toContain("members");
    expect(quickstartLinkHref("configure-workspace", "acme")).toBe("/acme/settings");
    expect(quickstartLinkHref("personalize-account", "acme")).toBe("/acme/settings/account/general/");
  });
});

describe("quickstartVisitFlag", () => {
  it("records settings and profile visits only", () => {
    expect(quickstartVisitFlag("configure-workspace")).toBe("visited_workspace");
    expect(quickstartVisitFlag("personalize-account")).toBe("visited_profile");
    expect(quickstartVisitFlag("create-project")).toBeNull();
    expect(quickstartVisitFlag("invite-team")).toBeNull();
  });
});

describe("countJoinedProjects", () => {
  it("counts rows carrying a membership role", () => {
    expect(countJoinedProjects([])).toBe(0);
    expect(countJoinedProjects([{ member_role: 15 }, { member_role: null }, { member_role: 5 }])).toBe(2);
  });
});

describe("isQuickstartCardDone", () => {
  const input = { joinedProjectCount: 0, totalMembers: 1, flags: FLAGS };

  it("completes the project card on the first join", () => {
    expect(isQuickstartCardDone("create-project", input)).toBe(false);
    expect(isQuickstartCardDone("create-project", { ...input, joinedProjectCount: 1 })).toBe(true);
  });

  it("completes the team card at two members", () => {
    expect(isQuickstartCardDone("invite-team", input)).toBe(false);
    expect(isQuickstartCardDone("invite-team", { ...input, totalMembers: 2 })).toBe(true);
  });

  it("completes the workspace card on a settings visit", () => {
    expect(isQuickstartCardDone("configure-workspace", input)).toBe(false);
    expect(
      isQuickstartCardDone("configure-workspace", {
        ...input,
        flags: { ...FLAGS, visited_workspace: true },
      })
    ).toBe(true);
  });

  it("completes the profile card on a profile visit", () => {
    expect(isQuickstartCardDone("personalize-account", input)).toBe(false);
    expect(
      isQuickstartCardDone("personalize-account", {
        ...input,
        flags: { ...FLAGS, visited_profile: true },
      })
    ).toBe(true);
  });
});

describe("isQuickstartPanelVisible", () => {
  it("shows fresh workspaces", () => {
    expect(isQuickstartPanelVisible({ joinedProjectCount: 0, totalMembers: 1, flags: FLAGS })).toBe(true);
  });

  it("hides on dismissal", () => {
    expect(
      isQuickstartPanelVisible({
        joinedProjectCount: 0,
        totalMembers: 1,
        flags: { ...FLAGS, hide: true },
      })
    ).toBe(false);
  });

  it("auto-hides once lived-in", () => {
    expect(isQuickstartPanelVisible({ joinedProjectCount: 2, totalMembers: 3, flags: FLAGS })).toBe(false);
  });

  it("stays while either lived-in signal is missing", () => {
    expect(isQuickstartPanelVisible({ joinedProjectCount: 2, totalMembers: 1, flags: FLAGS })).toBe(true);
    expect(isQuickstartPanelVisible({ joinedProjectCount: 0, totalMembers: 3, flags: FLAGS })).toBe(true);
  });
});

describe("canShowQuickstartAction", () => {
  const admin = { canEdit: true, isAdmin: true };
  const member = { canEdit: true, isAdmin: false };
  const guest = { canEdit: false, isAdmin: false };

  it("shows every action to admins", () => {
    const cards: readonly QuickstartCardId[] = QUICKSTART_CARD_IDS;
    for (const card of cards) {
      expect(canShowQuickstartAction(card, admin)).toBe(true);
    }
  });

  it("withholds invite and setup from non-admins", () => {
    expect(canShowQuickstartAction("create-project", member)).toBe(true);
    expect(canShowQuickstartAction("invite-team", member)).toBe(false);
    expect(canShowQuickstartAction("configure-workspace", member)).toBe(false);
    expect(canShowQuickstartAction("personalize-account", member)).toBe(true);
  });

  it("leaves guests the profile action only", () => {
    expect(canShowQuickstartAction("create-project", guest)).toBe(false);
    expect(canShowQuickstartAction("invite-team", guest)).toBe(false);
    expect(canShowQuickstartAction("configure-workspace", guest)).toBe(false);
    expect(canShowQuickstartAction("personalize-account", guest)).toBe(true);
  });
});
