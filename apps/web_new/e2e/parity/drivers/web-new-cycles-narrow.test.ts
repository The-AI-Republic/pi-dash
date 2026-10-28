// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycles narrow-layout methods
// (NEWFRONT-715, CYC-048): the list opener, the inline/hover row
// mounts, the narrow list layout menu, the detail mobile switcher,
// the floating-sidebar dismiss, the visible-name reader, and the
// detail quick-actions menu plus the read-only notice. A recording
// fake stands in for Playwright: each locator remembers the path of
// calls that built it, scripted answers stand in for the page, and a
// test asserts both which selector the driver used and what it does
// with the answer. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "getByText", "locator", "filter"];
const ACTIONS = [
  "count",
  "click",
  "innerText",
  "textContent",
  "allTextContents",
  "hover",
  "waitFor",
  "isVisible",
  "getAttribute",
];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "function") return "<fn>";
      if (arg instanceof RegExp) return String(arg);
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") return JSON.stringify(arg);
      return String(arg);
    })
    .join(",");
}

const ROWS_PATH = `page.locator("div[data-testid=\\"cycle-row\\"]")`;
const MENUITEMS_PATH = `page.getByRole("menuitem")`;
const ROW_NAMES_PATH = `page.locator("div[data-testid=\\"cycle-row\\"] button[data-testid=\\"cycle-row-menu-trigger\\"], div[data-testid=\\"cycle-row\\"] button[data-testid=\\"cycle-row-menu-inline\\"]")`;
const SETTLED_PATH = `page.locator("[data-testid=\\"cycles-list-body\\"], [data-testid=\\"cycles-zero-state\\"], [data-testid=\\"cycles-load-error\\"], [data-testid=\\"cycles-feature-gate\\"]")`;
const SIDEBAR_PATH = `page.locator("aside[aria-label=\\"Workspace navigation\\"]")`;
const NOTICE_PATH = `page.getByText(/not editable/i).first()`;

function rowPath(name: string): string {
  return `${ROWS_PATH}.filter(${JSON.stringify({ hasText: name })}).first()`;
}

function rowMountPath(name: string, testId: string): string {
  return `${rowPath(name)}.getByTestId(${JSON.stringify(testId)})`;
}

function rowButtonPath(name: string, label: string): string {
  return `${rowPath(name)}.getByRole("button",${JSON.stringify({ name: label })})`;
}

function testIdPath(id: string): string {
  return `page.getByTestId(${JSON.stringify(id)})`;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly gotos: string[] = [];
  currentUrl = "http://x/acme/projects/pid/cycles/cid";
  narrow = false;
  inlinePresent = true;
  desktopTriggerPresent = true;
  togglePresent = true;
  sidebarShown = true;
  cycleCollapsed: boolean | null = null;
  inlineShown = true;
  hoverShown = false;
  listMenuShown = true;
  detailMenuShown = true;
  menuOpen = false;
  menuTexts: string[] = [];
  rowLabels: Array<string | null> = [];
  noticePresent = true;
  noticeText: string | null = "This cycle has finished and is not editable.";
  noticeRejects = false;

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path, method, args });
        return this.locator(`${path}.${method}(${render(args)})`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        const call = { path, method, args };
        this.calls.push(call);
        return this.respond(call);
      };
    }
    return node as unknown as Locator;
  }

  respond(call: Call): unknown {
    const { path, method, args } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText" || method === "textContent") return this.text(path);
    if (method === "allTextContents") return [...this.menuTexts];
    if (method === "isVisible") return this.visible(path);
    if (method === "getAttribute") return this.attribute(path, args[0]);
    if (method === "waitFor" || method === "click" || method === "hover") return undefined;
    throw new Error(`unexpected action ${method} at ${path}`);
  }

  private count(path: string): number {
    if (path === MENUITEMS_PATH) return this.menuOpen ? this.menuTexts.length : 0;
    if (path === `${MENUITEMS_PATH}.locator("h5")`) return this.menuTexts.length;
    if (path.endsWith('.getByTestId("cycle-row-menu-inline")')) return this.inlinePresent ? 1 : 0;
    if (path === testIdPath("cycle-row-menu-trigger")) return this.desktopTriggerPresent ? 1 : 0;
    if (path === testIdPath("sidebar-toggle")) return this.togglePresent ? 1 : 0;
    if (path === testIdPath("cycle-sidebar-toggle-narrow")) return this.cycleCollapsed === null ? 0 : 1;
    if (path === SIDEBAR_PATH) return 1;
    if (path === NOTICE_PATH) return this.noticePresent ? 1 : 0;
    if (path === ROW_NAMES_PATH) return this.rowLabels.length;
    return 1;
  }

  private text(path: string): string {
    if (path === NOTICE_PATH) {
      if (this.noticeRejects) throw new Error("detached");
      if (!this.noticePresent) throw new Error("unexpected text read with no notice");
      return this.noticeText ?? "";
    }
    return "";
  }

  private visible(path: string): boolean {
    if (path === `${SIDEBAR_PATH}.first()`) return this.sidebarShown;
    if (path.endsWith('.getByTestId("cycle-row-menu-inline").first()')) return this.inlineShown;
    if (path.endsWith('.getByTestId("cycle-row-menu-trigger").first()')) return this.hoverShown;
    if (path === `${testIdPath("cycles-list-layout-menu")}.first()`) return this.listMenuShown;
    if (path === `${testIdPath("mobile-layout-trigger")}.first()`) return this.detailMenuShown;
    return true;
  }

  private attribute(path: string, name: unknown): string | null {
    if (path.startsWith(`${ROW_NAMES_PATH}.nth(`)) {
      const match = /\.nth\((\d+)\)$/.exec(path);
      const index = match === null ? -1 : Number(match[1]);
      return this.rowLabels[index] ?? null;
    }
    if (path === `${testIdPath("cycle-sidebar-toggle-narrow")}.first()`) {
      // The toggle reports aria-expanded (the inverse of collapsed).
      if (name !== "aria-expanded") return null;
      return this.cycleCollapsed === null ? null : String(!this.cycleCollapsed);
    }
    return null;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByTestId", "getByText", "locator"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.goto = async (url: string) => {
      this.gotos.push(url);
      return undefined;
    };
    node.keyboard = {
      press: async (key: string) => {
        this.calls.push({ path: "keyboard", method: "press", args: [key] });
        return undefined;
      },
    };
    node.waitForLoadState = async () => undefined;
    node.evaluate = async () => this.narrow;
    node.url = () => this.currentUrl;
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }
}

describe("cyclesHeroOpenList", () => {
  it("opens the list address and settles on whatever it renders", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenList("acme", "pid");
    expect(script.gotos).toEqual(["/acme/projects/pid/cycles"]);
    expect(script.callsFor(`${SETTLED_PATH}.first()`, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHeroVisibleNames", () => {
  it("reads row names from either trigger mount in order", async () => {
    const script = new Recorder();
    script.rowLabels = ["Cycle actions for Alpha", "Cycle actions for Beta", null];
    expect(await script.driver().cyclesHeroVisibleNames()).toEqual(["Alpha", "Beta"]);
    expect(script.callsFor(ROW_NAMES_PATH, "count")).toHaveLength(1);
  });

  it("reads no names when no rows render", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroVisibleNames()).toEqual([]);
  });
});

describe("cyclesHeroOpenRowMenu", () => {
  it("clicks the inline trigger on narrow screens without hovering", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenRowMenu("Alpha");
    expect(script.callsFor(`${rowMountPath("Alpha", "cycle-row-menu-inline")}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(rowPath("Alpha"), "hover")).toHaveLength(0);
    expect(script.callsFor(`${MENUITEMS_PATH}.first()`, "waitFor")).toHaveLength(1);
  });

  it("hovers the row and clicks the trigger on desktop", async () => {
    const script = new Recorder();
    script.inlinePresent = false;
    await script.driver().cyclesHeroOpenRowMenu("Alpha");
    expect(script.callsFor(rowPath("Alpha"), "hover")).toHaveLength(1);
    expect(script.callsFor(rowButtonPath("Alpha", "Cycle actions for Alpha"), "click")).toHaveLength(1);
    expect(script.callsFor(`${MENUITEMS_PATH}.first()`, "waitFor")).toHaveLength(1);
  });

  it("dismisses an open peek before touching the row", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenRowMenu("Alpha");
    expect(script.callsFor("keyboard", "press")).toHaveLength(1);
    expect(script.callsFor(`page.getByTestId("cycle-preview")`, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesHeroRowMenuEntries", () => {
  it("reads the entry headings and drops blanks", async () => {
    const script = new Recorder();
    script.menuTexts = ["  Edit  ", "", "Copy link"];
    expect(await script.driver().cyclesHeroRowMenuEntries()).toEqual(["Edit", "Copy link"]);
  });
});

describe("cyclesHeroInlineActionsVisible", () => {
  it("reads the inline mount inside the named row", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroInlineActionsVisible("Alpha")).toBe(true);
    script.inlineShown = false;
    expect(await script.driver().cyclesHeroInlineActionsVisible("Alpha")).toBe(false);
  });
});

describe("cyclesHeroHoverActionsVisible", () => {
  it("reads the hover mount inside the named row", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroHoverActionsVisible("Alpha")).toBe(false);
    script.hoverShown = true;
    expect(await script.driver().cyclesHeroHoverActionsVisible("Alpha")).toBe(true);
  });
});

describe("cyclesHeroLayoutMenuVisible", () => {
  it("reads the narrow list menu", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroLayoutMenuVisible()).toBe(true);
    script.listMenuShown = false;
    expect(await script.driver().cyclesHeroLayoutMenuVisible()).toBe(false);
  });
});

describe("cyclesHeroLayoutOptions", () => {
  it("opens the menu and reads its options in order", async () => {
    const script = new Recorder();
    script.menuTexts = ["List layout", "Gallery layout"];
    expect(await script.driver().cyclesHeroLayoutOptions()).toEqual(["List layout", "Gallery layout"]);
    expect(script.callsFor(testIdPath("cycles-list-layout-trigger"), "click")).toHaveLength(1);
  });

  it("skips the trigger when the menu already stands open", async () => {
    const script = new Recorder();
    script.menuOpen = true;
    script.menuTexts = ["List layout", "Gallery layout"];
    expect(await script.driver().cyclesHeroLayoutOptions()).toEqual(["List layout", "Gallery layout"]);
    expect(script.callsFor(testIdPath("cycles-list-layout-trigger"), "click")).toHaveLength(0);
  });
});

describe("cyclesHeroPickLayout", () => {
  it("picks the named option, opening the menu first", async () => {
    const script = new Recorder();
    script.menuTexts = ["List layout", "Gallery layout"];
    await script.driver().cyclesHeroPickLayout("Gallery layout");
    expect(script.callsFor(testIdPath("cycles-list-layout-trigger"), "click")).toHaveLength(1);
    const picks = script.callsFor(
      `page.getByRole("menuitem",${JSON.stringify({ name: "Gallery layout" })}).first()`,
      "click"
    );
    expect(picks).toHaveLength(1);
  });
});

describe("cyclesHeroDetailLayoutMenuVisible", () => {
  it("reads the mobile layout trigger", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesHeroDetailLayoutMenuVisible()).toBe(true);
    script.detailMenuShown = false;
    expect(await script.driver().cyclesHeroDetailLayoutMenuVisible()).toBe(false);
  });
});

describe("cyclesHeroDetailLayoutOptions", () => {
  it("opens the mobile menu and reads its options in order", async () => {
    const script = new Recorder();
    script.menuTexts = ["List", "Board", "Calendar"];
    expect(await script.driver().cyclesHeroDetailLayoutOptions()).toEqual(["List", "Board", "Calendar"]);
    expect(script.callsFor(testIdPath("mobile-layout-trigger"), "click")).toHaveLength(1);
  });

  it("skips the trigger when the menu already stands open", async () => {
    const script = new Recorder();
    script.menuOpen = true;
    script.menuTexts = ["List", "Board", "Calendar"];
    expect(await script.driver().cyclesHeroDetailLayoutOptions()).toEqual(["List", "Board", "Calendar"]);
    expect(script.callsFor(testIdPath("mobile-layout-trigger"), "click")).toHaveLength(0);
  });
});

describe("cyclesHeroDetailPickLayout", () => {
  it("picks the named layout, opening the menu first", async () => {
    const script = new Recorder();
    script.menuTexts = ["List", "Board", "Calendar"];
    await script.driver().cyclesHeroDetailPickLayout("Board");
    expect(script.callsFor(testIdPath("mobile-layout-trigger"), "click")).toHaveLength(1);
    const picks = script.callsFor(`page.getByRole("menuitem",${JSON.stringify({ name: "Board" })}).first()`, "click");
    expect(picks).toHaveLength(1);
  });
});

describe("cyclesHeroDismissSidebar", () => {
  it("does nothing on desktop viewports", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(testIdPath("sidebar-toggle"), "click")).toHaveLength(0);
  });

  it("does nothing when the narrow sidebar already rests collapsed", async () => {
    const script = new Recorder();
    script.narrow = true;
    script.sidebarShown = false;
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(testIdPath("sidebar-toggle"), "click")).toHaveLength(0);
  });

  it("collapses the floating sidebar through the top-bar toggle", async () => {
    const script = new Recorder();
    script.narrow = true;
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(`${testIdPath("sidebar-toggle")}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(`${SIDEBAR_PATH}.first()`, "waitFor")).toHaveLength(1);
  });

  it("fails loudly when the open drawer has no toggle", async () => {
    const script = new Recorder();
    script.narrow = true;
    script.togglePresent = false;
    await expect(script.driver().cyclesHeroDismissSidebar()).rejects.toThrow(
      "[parity] floating sidebar open with no sidebar toggle."
    );
  });

  it("expands a collapsed cycle sidebar to the oracle state", async () => {
    const script = new Recorder();
    script.narrow = true;
    script.sidebarShown = false;
    script.cycleCollapsed = true;
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "click")).toHaveLength(1);
  });

  it("leaves an open cycle sidebar alone", async () => {
    const script = new Recorder();
    script.narrow = true;
    script.sidebarShown = false;
    script.cycleCollapsed = false;
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "click")).toHaveLength(0);
  });

  it("waits for the toggle on detail addresses and skips it on the list", async () => {
    const script = new Recorder();
    script.narrow = true;
    script.sidebarShown = false;
    script.cycleCollapsed = true;
    await script.driver().cyclesHeroDismissSidebar();
    expect(script.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "waitFor")).toHaveLength(1);
    expect(script.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "click")).toHaveLength(1);
    const list = new Recorder();
    list.narrow = true;
    list.sidebarShown = false;
    list.cycleCollapsed = true;
    list.currentUrl = "http://x/acme/projects/pid/cycles";
    await list.driver().cyclesHeroDismissSidebar();
    expect(list.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "waitFor")).toHaveLength(0);
    expect(list.callsFor(`${testIdPath("cycle-sidebar-toggle-narrow")}.first()`, "click")).toHaveLength(0);
  });
});

describe("cyclesEditOpenDetailMenu", () => {
  it("opens the detail quick-actions menu from its desktop trigger", async () => {
    const script = new Recorder();
    await script.driver().cyclesEditOpenDetailMenu();
    expect(script.callsFor(`${testIdPath("cycle-row-menu-trigger")}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(`${MENUITEMS_PATH}.first()`, "waitFor")).toHaveLength(1);
  });

  it("falls back to the inline trigger on narrow screens", async () => {
    const script = new Recorder();
    script.desktopTriggerPresent = false;
    await script.driver().cyclesEditOpenDetailMenu();
    expect(script.callsFor(`${testIdPath("cycle-row-menu-inline")}.first()`, "click")).toHaveLength(1);
  });
});

describe("cyclesEditDetailReadOnlyNotice", () => {
  it("reads the finished-cycle notice text", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesEditDetailReadOnlyNotice()).toBe("This cycle has finished and is not editable.");
  });

  it("returns null when no notice renders", async () => {
    const script = new Recorder();
    script.noticePresent = false;
    expect(await script.driver().cyclesEditDetailReadOnlyNotice()).toBeNull();
  });

  it("returns null when the notice detaches mid-read", async () => {
    const script = new Recorder();
    script.noticeRejects = true;
    expect(await script.driver().cyclesEditDetailReadOnlyNotice()).toBeNull();
  });

  it("returns null when the notice carries no text", async () => {
    const script = new Recorder();
    script.noticeText = "   ";
    expect(await script.driver().cyclesEditDetailReadOnlyNotice()).toBeNull();
  });
});
