// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  countFittingTabs,
  DEFAULT_TAB_KEY,
  isProjectAdmin,
  isProjectAuthorized,
  isTabbedMode,
  parseNavigationPrefs,
  resolveActiveTabKey,
  resolveOverflow,
  selectCopyPath,
  selectDefaultTarget,
  selectProjectTabs,
  selectQuickActions,
  selectSwitcherTarget,
  tabHref,
  type ProjectTab,
  type ProjectTabFlags,
} from "./tabs.js";

const ALL_FLAGS: ProjectTabFlags = {
  cycle_view: true,
  module_view: true,
  issue_views_view: true,
  page_view: true,
  inbox_view: true,
};

const NO_FLAGS: ProjectTabFlags = {
  cycle_view: false,
  module_view: false,
  issue_views_view: false,
  page_view: false,
  inbox_view: false,
};

function tabs(role: number | null = 20, flags: ProjectTabFlags = ALL_FLAGS): ProjectTab[] {
  return selectProjectTabs({ workspaceSlug: "acme", projectId: "p1", flags, role });
}

describe("selectProjectTabs", () => {
  it("renders every tab in fixed order for members with all flags on", () => {
    const got = tabs();
    expect(got.map((tab) => tab.name)).toEqual([
      "Work Items",
      "Cycles",
      "Modules",
      "Views",
      "Pages",
      "Intake",
      "Schedulers",
    ]);
    expect(got.map((tab) => tab.order)).toEqual([1, 2, 3, 4, 5, 6, 7]);
    expect(got[0]?.href).toBe("/acme/projects/p1/issues");
    expect(got[6]?.href).toBe("/acme/projects/p1/schedulers");
  });

  it("drops flag-gated tabs while keeping the always-on ones", () => {
    const got = tabs(20, NO_FLAGS);
    expect(got.map((tab) => tab.key)).toEqual(["work_items", "schedulers"]);
  });

  it("drops a single disabled feature", () => {
    const got = tabs(20, { ...ALL_FLAGS, page_view: false });
    expect(got.map((tab) => tab.key)).not.toContain("pages");
    expect(got.map((tab) => tab.key)).toContain("cycles");
  });

  it("shows guests tracking, views, pages, intake and schedulers only", () => {
    const got = tabs(5);
    expect(got.map((tab) => tab.key)).toEqual(["work_items", "views", "pages", "intake", "schedulers"]);
  });

  it("reads a null role as guest level", () => {
    expect(tabs(null).map((tab) => tab.key)).toEqual(["work_items", "views", "pages", "intake", "schedulers"]);
  });

  it("shows members the member-plus tabs", () => {
    expect(tabs(15).map((tab) => tab.key)).toContain("cycles");
    expect(tabs(15).map((tab) => tab.key)).toContain("modules");
  });
});

describe("tabHref", () => {
  it("builds the tab address without a trailing slash", () => {
    expect(tabHref("acme", "p1", "issues")).toBe("/acme/projects/p1/issues");
  });
});

describe("resolveActiveTabKey", () => {
  it("matches the exact tab address", () => {
    expect(
      resolveActiveTabKey({ tabs: tabs(), pathname: "/acme/projects/p1/pages", projectId: "p1", workItem: null })
    ).toBe("pages");
  });

  it("keeps the parent highlighted on nested routes", () => {
    expect(
      resolveActiveTabKey({ tabs: tabs(), pathname: "/acme/projects/p1/cycles/abc", projectId: "p1", workItem: null })
    ).toBe("cycles");
    expect(
      resolveActiveTabKey({ tabs: tabs(), pathname: "/acme/projects/p1/issues/7/", projectId: "p1", workItem: null })
    ).toBe("work_items");
  });

  it("reports nothing on the bare project address and foreign paths", () => {
    expect(resolveActiveTabKey({ tabs: tabs(), pathname: "/acme/projects/p1", projectId: "p1", workItem: null })).toBe(
      null
    );
    expect(
      resolveActiveTabKey({
        tabs: tabs(),
        pathname: "/acme/projects/p1/archives/issues",
        projectId: "p1",
        workItem: null,
      })
    ).toBe(null);
  });

  it("highlights tracking for a plain work item of this project", () => {
    expect(
      resolveActiveTabKey({
        tabs: tabs(),
        pathname: "/acme/browse/ACME-1",
        projectId: "p1",
        workItem: { id: "w1", projectId: "p1", isEpic: false },
      })
    ).toBe("work_items");
  });

  it("matches the epic key when a rendered tab carries it", () => {
    const withEpics: ProjectTab[] = [
      ...tabs(),
      {
        key: "epics",
        name: "Epics",
        segment: "epics",
        order: 8,
        flag: null,
        memberPlus: false,
        href: "/acme/projects/p1/epics",
      },
    ];
    expect(
      resolveActiveTabKey({
        tabs: withEpics,
        pathname: "/acme/browse/ACME-1",
        projectId: "p1",
        workItem: { id: "w1", projectId: "p1", isEpic: true },
      })
    ).toBe("epics");
  });

  it("highlights nothing for an epic while no epic tab renders", () => {
    expect(
      resolveActiveTabKey({
        tabs: tabs(),
        pathname: "/acme/browse/ACME-1",
        projectId: "p1",
        workItem: { id: "w1", projectId: "p1", isEpic: true },
      })
    ).toBe(null);
  });

  it("ignores work items of other projects", () => {
    expect(
      resolveActiveTabKey({
        tabs: tabs(),
        pathname: "/acme/browse/ACME-1",
        projectId: "p1",
        workItem: { id: "w1", projectId: "p2", isEpic: false },
      })
    ).toBe(null);
  });
});

describe("selectDefaultTarget", () => {
  it("lands on the stored default when available", () => {
    expect(selectDefaultTarget(tabs(), "cycles")?.href).toBe("/acme/projects/p1/cycles");
  });

  it("falls back to tracking for unknown stored keys", () => {
    expect(selectDefaultTarget(tabs(), "nope")?.key).toBe(DEFAULT_TAB_KEY);
  });

  it("falls back to tracking when the stored tab is gated away", () => {
    expect(selectDefaultTarget(tabs(20, NO_FLAGS), "cycles")?.key).toBe(DEFAULT_TAB_KEY);
  });

  it("reports null without tabs or without tracking", () => {
    expect(selectDefaultTarget([], "cycles")).toBe(null);
    const noTracking = tabs().filter((tab) => tab.key !== DEFAULT_TAB_KEY);
    expect(selectDefaultTarget(noTracking, "nope")).toBe(null);
  });
});

describe("selectSwitcherTarget", () => {
  it("opens the picked project at the current default tab", () => {
    expect(
      selectSwitcherTarget({ currentTabs: tabs(), currentDefaultKey: "cycles", workspaceSlug: "acme", projectId: "p2" })
    ).toBe("/acme/projects/p2/cycles");
  });

  it("falls back to tracking for unknown keys and empty tab lists", () => {
    expect(
      selectSwitcherTarget({ currentTabs: tabs(), currentDefaultKey: "nope", workspaceSlug: "acme", projectId: "p2" })
    ).toBe("/acme/projects/p2/issues");
    expect(
      selectSwitcherTarget({ currentTabs: [], currentDefaultKey: "cycles", workspaceSlug: "acme", projectId: "p2" })
    ).toBe("/acme/projects/p2/issues");
  });
});

describe("parseNavigationPrefs", () => {
  it("reads the stored default and hidden keys", () => {
    expect(parseNavigationPrefs({ navigation: { default_tab: "cycles", hide_in_more_menu: ["pages"] } })).toEqual({
      defaultTab: "cycles",
      hiddenTabs: ["pages"],
    });
  });

  it("falls back on missing or malformed payloads", () => {
    expect(parseNavigationPrefs(undefined)).toEqual({ defaultTab: DEFAULT_TAB_KEY, hiddenTabs: [] });
    expect(parseNavigationPrefs(null)).toEqual({ defaultTab: DEFAULT_TAB_KEY, hiddenTabs: [] });
    expect(parseNavigationPrefs({})).toEqual({ defaultTab: DEFAULT_TAB_KEY, hiddenTabs: [] });
    expect(parseNavigationPrefs({ navigation: null })).toEqual({ defaultTab: DEFAULT_TAB_KEY, hiddenTabs: [] });
    expect(parseNavigationPrefs({ navigation: { default_tab: 7, hide_in_more_menu: "pages" } })).toEqual({
      defaultTab: DEFAULT_TAB_KEY,
      hiddenTabs: [],
    });
  });

  it("drops non-string hidden keys", () => {
    expect(parseNavigationPrefs({ navigation: { hide_in_more_menu: ["pages", 7, null] } })).toEqual({
      defaultTab: DEFAULT_TAB_KEY,
      hiddenTabs: ["pages"],
    });
  });
});

describe("isTabbedMode", () => {
  it("mounts the strip in tabbed mode only", () => {
    expect(isTabbedMode("TABBED")).toBe(true);
    expect(isTabbedMode("ACCORDION")).toBe(false);
    expect(isTabbedMode("")).toBe(false);
  });
});

describe("project standing", () => {
  it("gates admin standing at the admin role", () => {
    expect(isProjectAdmin(20)).toBe(true);
    expect(isProjectAdmin(15)).toBe(false);
    expect(isProjectAdmin(5)).toBe(false);
    expect(isProjectAdmin(null)).toBe(false);
  });

  it("gates authorized standing at the member role", () => {
    expect(isProjectAuthorized(20)).toBe(true);
    expect(isProjectAuthorized(15)).toBe(true);
    expect(isProjectAuthorized(5)).toBe(false);
    expect(isProjectAuthorized(null)).toBe(false);
  });
});

describe("selectQuickActions", () => {
  it("offers admins publish, copy, archives and settings", () => {
    expect(selectQuickActions({ isAdmin: true, isAuthorized: true }).map((entry) => entry.label)).toEqual([
      "Publish project",
      "Copy link",
      "Archives",
      "Settings",
    ]);
  });

  it("offers members copy, archives and settings", () => {
    expect(selectQuickActions({ isAdmin: false, isAuthorized: true }).map((entry) => entry.label)).toEqual([
      "Copy link",
      "Archives",
      "Settings",
    ]);
  });

  it("offers outsiders copy, settings and leave", () => {
    expect(selectQuickActions({ isAdmin: false, isAuthorized: false }).map((entry) => entry.label)).toEqual([
      "Copy link",
      "Settings",
      "Leave project",
    ]);
  });
});

describe("selectCopyPath", () => {
  it("copies the active tab route", () => {
    expect(selectCopyPath({ activeHref: "/acme/projects/p1/pages", workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/pages"
    );
  });

  it("falls back to tracking without an active tab", () => {
    expect(selectCopyPath({ activeHref: null, workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/issues"
    );
  });
});

describe("countFittingTabs", () => {
  it("counts nothing without items", () => {
    expect(countFittingTabs({ itemWidths: [], gap: 4, overflowWidth: 40, containerWidth: 500 })).toBe(0);
  });

  it("fits every tab while the row holds them", () => {
    expect(countFittingTabs({ itemWidths: [100, 100, 100], gap: 4, overflowWidth: 40, containerWidth: 500 })).toBe(3);
  });

  it("reserves the trigger while tabs remain", () => {
    // The third tab needs 308 total against 250, so it pools with room kept.
    expect(countFittingTabs({ itemWidths: [100, 100, 100], gap: 4, overflowWidth: 40, containerWidth: 250 })).toBe(2);
    // Exact fit lands without pooling.
    expect(countFittingTabs({ itemWidths: [100, 100], gap: 4, overflowWidth: 40, containerWidth: 204 })).toBe(2);
  });

  it("keeps one tab visible when nothing fits but the trigger would", () => {
    expect(countFittingTabs({ itemWidths: [400, 400], gap: 4, overflowWidth: 40, containerWidth: 200 })).toBe(1);
  });

  it("shows nothing when even the trigger overflows", () => {
    expect(countFittingTabs({ itemWidths: [400, 400], gap: 4, overflowWidth: 40, containerWidth: 30 })).toBe(0);
  });
});

describe("resolveOverflow", () => {
  const all = tabs();

  it("shows everything while it fits", () => {
    const got = resolveOverflow({ candidates: all, hidden: [], visibleCount: 7, activeKey: "work_items" });
    expect(got.visible.map((tab) => tab.key)).toHaveLength(7);
    expect(got.overflow).toEqual([]);
    expect(got.hasOverflow).toBe(false);
  });

  it("pools trailing tabs when narrowed", () => {
    const got = resolveOverflow({ candidates: all, hidden: [], visibleCount: 3, activeKey: "work_items" });
    expect(got.visible.map((tab) => tab.key)).toEqual(["work_items", "cycles", "modules"]);
    expect(got.overflow.map((tab) => tab.key)).toEqual(["views", "pages", "intake", "schedulers"]);
    expect(got.hasOverflow).toBe(true);
  });

  it("pins the active tab into view with a swap", () => {
    const got = resolveOverflow({ candidates: all, hidden: [], visibleCount: 3, activeKey: "pages" });
    expect(got.visible.map((tab) => tab.key)).toEqual(["work_items", "cycles", "pages"]);
    expect(got.overflow.map((tab) => tab.key)).toEqual(["modules", "views", "intake", "schedulers"]);
  });

  it("pools everything at zero width without swapping", () => {
    const got = resolveOverflow({ candidates: all, hidden: [], visibleCount: 0, activeKey: "pages" });
    expect(got.visible).toEqual([]);
    expect(got.overflow.map((tab) => tab.key)).toHaveLength(7);
  });

  it("pools user-hidden tabs after the space-pooled ones", () => {
    const candidates = all.filter((tab) => tab.key !== "intake");
    const hidden = all.filter((tab) => tab.key === "intake");
    const got = resolveOverflow({ candidates, hidden, visibleCount: 3, activeKey: "work_items" });
    expect(got.visible.map((tab) => tab.key)).toEqual(["work_items", "cycles", "modules"]);
    expect(got.overflow.map((tab) => tab.key)).toEqual(["views", "pages", "schedulers", "intake"]);
    expect(got.hasOverflow).toBe(true);
  });

  it("shows the trigger for hidden tabs alone", () => {
    const candidates = all.filter((tab) => tab.key !== "intake");
    const hidden = all.filter((tab) => tab.key === "intake");
    const got = resolveOverflow({ candidates, hidden, visibleCount: 6, activeKey: "work_items" });
    expect(got.overflow.map((tab) => tab.key)).toEqual(["intake"]);
    expect(got.hasOverflow).toBe(true);
  });
});
