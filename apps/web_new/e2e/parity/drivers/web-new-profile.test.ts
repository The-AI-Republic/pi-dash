// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver profile-viewing methods
// (NEWFRONT-573, PROF-001..008). A stub Page answers each selector with a
// canned Locator so the assertions are about the value or selector the
// method reads, never about a browser. profileTabHref, profileLayoutChoices
// and the export stall/file branches are exercised end to end here; the
// remaining methods share the same read pattern and are covered through
// their role in the profile route tests.
import type { Download, Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string | null) => Promise<string | null>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  evaluateAll?: (fn: (elements: never[]) => unknown) => Promise<unknown>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  filter?: (options?: unknown) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    allInnerTexts: async () => [],
    evaluateAll: async () => [],
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByText: () => node,
    locator: () => node,
    filter: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByTestId: () => locator(),
    getByRole: () => locator(),
    locator: () => locator(),
    keyboard: { press: async () => undefined },
    waitForTimeout: async () => undefined,
    viewportSize: () => ({ width: 1280, height: 720 }),
    ...overrides,
  } as unknown as Page;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("profileTabHref", () => {
  it("reads the href of the exact tab link and asks for no substitute when absent", async () => {
    const links = locator({ getAttribute: async () => "/acme/profile/u-1/assigned/" });
    const page = pageOf({ getByTestId: () => locator({ getByRole: () => links }) });
    const driver = new WebNewDriver(page);
    expect(await driver.profileTabHref("Assigned")).toBe("/acme/profile/u-1/assigned/");
  });

  it("answers null when the tab is not offered", async () => {
    const page = pageOf({
      getByTestId: () => locator({ getByRole: () => locator({ count: async () => 0 }) }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.profileTabHref("Subscribed")).toBeNull();
  });
});

describe("profileVisibleTabs", () => {
  it("trims the tab strip's link labels", async () => {
    const links = locator({ allInnerTexts: async () => [" Summary ", " Assigned  ", "Activity"] });
    const page = pageOf({ getByTestId: () => locator({ locator: () => links }) });
    const driver = new WebNewDriver(page);
    expect(await driver.profileVisibleTabs()).toEqual(["Summary", "Assigned", "Activity"]);
  });
});

describe("profileBreadcrumbLabel", () => {
  it("reads the last breadcrumb's text", async () => {
    const crumbs = locator({ innerText: async () => "  Work " });
    const page = pageOf({
      getByRole: () => locator({ locator: () => crumbs }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.profileBreadcrumbLabel()).toBe("Work");
  });
});

describe("profileLayoutChoices", () => {
  it("opens the visible layout trigger and reads the offered layout kinds", async () => {
    const click = vi.fn(async () => undefined);
    const offered = locator({
      evaluateAll: async () => ["list", "kanban", "", "board"],
    });
    const page = pageOf({
      locator: () => locator({ click, first: () => locator({ click }) }),
      getByTestId: () => locator(),
      keyboard: { press: vi.fn(async () => undefined) },
    });
    // The choices locator is a plain CSS lookup; answer it specially.
    const original = page.locator;
    (page as unknown as { locator: (selector: string) => Locator }).locator = (selector: string) =>
      selector === "[data-profile-layout-choice]" ? offered : original(selector);
    const driver = new WebNewDriver(page);
    expect(await driver.profileIssuesOfferedLayouts()).toEqual(["list", "kanban"]);
    expect(click).toHaveBeenCalled();
  });
});

describe("profileActivityLoadMore", () => {
  it("clicks load-more and waits for the row count to grow", async () => {
    const click = vi.fn(async () => undefined);
    let rows = 3;
    const page = pageOf({
      getByTestId: () => locator({ click }),
      locator: () => locator({ count: async () => rows++ }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.profileActivityLoadMore()).resolves.toBeUndefined();
    expect(click).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });
});

describe("profileSidebarProjectRows", () => {
  it("reads each project's name and completion", async () => {
    const rows = [
      locator({
        getAttribute: async (name: string | null) => (name === "data-profile-project-row" ? "p-1" : null),
        locator: (selector: string) =>
          selector === "button"
            ? locator({ getAttribute: async () => "Alpha, expand" })
            : locator({ innerText: async () => "0%" }),
      }),
      locator({
        getAttribute: async (name: string | null) => (name === "data-profile-project-row" ? "p-2" : null),
        locator: (selector: string) =>
          selector === "button" ? locator({ getAttribute: async () => "Beta" }) : locator({ count: async () => 0 }),
      }),
    ];
    const page = pageOf({
      locator: () => locator({ count: async () => rows.length, nth: (index: number) => rows[index] ?? rows[0] }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.profileSidebarProjectRows()).toEqual([
      { name: "Alpha", completion: "0%" },
      { name: "Beta", completion: null },
    ]);
  });
});

describe("profileSidebarProjectCounts", () => {
  it("maps dt/dd pairs to created/assigned/due/completed", async () => {
    const terms = {
      count: async () => 4,
      nth: (index: number) =>
        locator({ innerText: async () => ["Created", "Assigned", "Due", "Completed"][index] ?? "" }),
    };
    const values = {
      nth: (index: number) => locator({ innerText: async () => ["4", "2", "1", "0"][index] ?? "" }),
    };
    const counts = locator({ locator: (selector: string) => (selector === "dt" ? terms : values) });
    const row = locator({ locator: () => counts, filter: () => locator({ locator: () => counts }) });
    const page = pageOf({ locator: () => row });
    const driver = new WebNewDriver(page);
    expect(await driver.profileSidebarProjectCounts("Alpha")).toEqual({
      created: "4",
      assigned: "2",
      due: "1",
      completed: "0",
    });
  });
});

describe("profileOverviewCards", () => {
  it("reads title, value and tab href from each card", async () => {
    const card = locator({
      getAttribute: async (name: string | null) =>
        name === "data-profile-card" ? "assigned" : "/acme/profile/u-1/assigned/",
      locator: (selector: string) =>
        selector === "span"
          ? locator({ first: () => locator({ innerText: async () => " Assigned " }) })
          : locator({ innerText: async () => " 12 " }),
    });
    const cards = locator({ count: async () => 1, nth: () => card });
    const page = pageOf({ locator: () => cards });
    const driver = new WebNewDriver(page);
    expect(await driver.profileOverviewCards()).toEqual([
      { title: "Assigned", value: "12", href: "/acme/profile/u-1/assigned/" },
    ]);
  });
});

describe("profileActivityStallExport / release", () => {
  function stalledDriver(): {
    driver: WebNewDriver;
    installed: () => ((route: Route) => Promise<void>) | null;
    route: ReturnType<typeof vi.fn>;
  } {
    let installed: ((route: Route) => Promise<void>) | null = null;
    const route = vi.fn(async (_pattern: string, handler: (route: Route) => Promise<void>) => {
      installed = handler;
    });
    const driver = new WebNewDriver(pageOf({ route }));
    return { driver, installed: () => installed, route };
  }

  function exportRoute(method: string, fallback: () => Promise<void>): Route {
    return { request: () => ({ method: () => method }), fallback } as unknown as Route;
  }

  it("installs one route, holds stalled POSTs on a gate, then releases them", async () => {
    const { driver, installed, route } = stalledDriver();
    await driver.profileActivityStallExport();
    await driver.profileActivityStallExport();
    expect(route).toHaveBeenCalledTimes(1);
    expect(route.mock.calls[0][0]).toBe("**/api/workspaces/*/user-activity/*/export/**");

    const fallback = vi.fn(async () => undefined);
    let held = false;
    const pending = installed()!(exportRoute("POST", fallback)).then(() => {
      held = true;
    });
    await Promise.resolve();
    expect(held).toBe(false);
    expect(fallback).not.toHaveBeenCalled();
    await driver.profileActivityReleaseExport();
    await pending;
    expect(held).toBe(true);
    expect(fallback).toHaveBeenCalledTimes(1);
  });

  it("lets non-POST requests and post-release POSTs fall through", async () => {
    const { driver, installed } = stalledDriver();
    await driver.profileActivityStallExport();
    const getFallback = vi.fn(async () => undefined);
    await installed()!(exportRoute("GET", getFallback));
    expect(getFallback).toHaveBeenCalledTimes(1);
    await driver.profileActivityReleaseExport();
    const postFallback = vi.fn(async () => undefined);
    await installed()!(exportRoute("POST", postFallback));
    expect(postFallback).toHaveBeenCalledTimes(1);
  });

  it("releases nothing when no stall was installed", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.profileActivityReleaseExport();
    expect(route).not.toHaveBeenCalled();
  });
});

describe("profileActivityDownload", () => {
  it("posts the day, reads the download's bytes and filename", async () => {
    const download = {
      suggestedFilename: () => "workspace-user-activity.csv",
      createReadStream: async () =>
        (async function* () {
          yield Buffer.from("actor,issue\n");
          yield Buffer.from("Opal,PP-1");
        })(),
    } as unknown as Download;
    const request = {
      method: () => "POST",
      url: () => "https://app.test/api/workspaces/acme/user-activity/u-1/export/",
      postDataJSON: () => ({ date: "2026-10-19" }),
    };
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      waitForRequest: async () => request,
      waitForEvent: async () => download,
      getByTestId: () => locator({ click }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.profileActivityDownload()).toEqual({
      file: { fileName: "workspace-user-activity.csv", content: "actor,issue\nOpal,PP-1" },
      requestBody: { date: "2026-10-19" },
    });
    expect(click).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("refuses a download with no bytes", async () => {
    const download = { suggestedFilename: () => "x.csv", createReadStream: async () => null } as unknown as Download;
    const page = pageOf({
      waitForRequest: async () => ({ method: () => "POST", url: () => "/export/", postDataJSON: () => null }),
      waitForEvent: async () => download,
      getByTestId: () => locator({ click: async () => undefined }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.profileActivityDownload()).rejects.toThrow("produced no bytes");
  });
});

describe("profileActivityDownloadVisible / Label", () => {
  it("answers false and null when the button is withheld", async () => {
    const page = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    expect(await driver.profileActivityDownloadVisible()).toBe(false);
    expect(await driver.profileActivityDownloadLabel()).toBeNull();
  });

  it("answers true and the trimmed label otherwise", async () => {
    const button = locator({ innerText: async () => " Download today's activity " });
    const page = pageOf({ getByTestId: () => button });
    const driver = new WebNewDriver(page);
    expect(await driver.profileActivityDownloadVisible()).toBe(true);
    expect(await driver.profileActivityDownloadLabel()).toBe("Download today's activity");
  });
});

describe("profileAccessDenied / profileTabRendersNothing", () => {
  it("treats a missing frame as nothing rendered", async () => {
    const page = pageOf({ getByTestId: () => locator({ count: async () => 0, isVisible: async () => false }) });
    const driver = new WebNewDriver(page);
    expect(await driver.profileAccessDenied()).toBe(false);
    expect(await driver.profileTabRendersNothing()).toBe(false);
  });

  it("reports a frame with no body surface as nothing rendered", async () => {
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "profile-page") return locator({ isVisible: async () => true });
        return locator({ count: async () => 0, isVisible: async () => false });
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.profileTabRendersNothing()).toBe(true);
  });
});

describe("shared peek methods on the profile list", () => {
  it("reads the peek title from the labelled field and closes by its accessible control", async () => {
    const field = locator({ inputValue: async () => "Prof peek row" });
    const close = vi.fn(async () => undefined);
    const preview = locator({ getByLabel: () => field });
    const page = pageOf({
      getByTestId: () => preview,
      getByRole: () => locator({ first: () => locator({ click: close }) }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.peekOpen()).toBe(true);
    expect(await driver.peekTitle()).toBe("Prof peek row");
    await driver.closePeek();
    expect(close).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("answers a missing peek field as no title", async () => {
    const page = pageOf({
      getByTestId: () => locator({ getByLabel: () => locator({ count: async () => 0 }) }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.peekTitle()).toBeNull();
  });

  it("clicks the row that names the work item in either list", async () => {
    const click = vi.fn(async () => undefined);
    const filters: unknown[] = [];
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="issue-list"] a[id^="issue-"], [data-testid="profile-list"] button[data-profile-row]'
          ? locator({
              filter: (options?: unknown) => {
                filters.push(options);
                return locator({ first: () => locator({ click }) });
              },
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.clickListRow("Prof persist urgent");
    expect(filters).toEqual([{ hasText: "Prof persist urgent" }]);
    expect(click).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });
});

describe("shared display methods on the profile tab", () => {
  function displayPage(overrides: { pressed?: string | null; closeFails?: boolean } = {}) {
    const click = vi.fn(async () => undefined);
    const radioClick = vi.fn(async () => undefined);
    const escape = vi.fn(async () => undefined);
    let panelWaits = 0;
    const button = locator({
      click,
      getAttribute: async () => overrides.pressed ?? "false",
      waitFor: async () => undefined,
    });
    const panel = locator({
      getByRole: (role: string) => (role === "radio" ? locator({ click: radioClick }) : button),
      waitFor: async (options?: unknown) => {
        panelWaits += 1;
        if (overrides.closeFails === true && panelWaits === 1) throw new Error("still open");
        return options;
      },
    });
    const trigger = locator({ click, waitFor: async () => undefined });
    const page = pageOf({
      getByTestId: (id: string) => (id === "profile-display-panel" ? panel : trigger),
      keyboard: { press: escape },
    });
    return { driver: new WebNewDriver(page), click, radioClick, escape };
  }

  it("opens the visible trigger and waits for the panel", async () => {
    const { driver, click } = displayPage();
    await driver.openDisplayOptions();
    expect(click).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("closes on Escape and falls back to the trigger when the panel stays open", async () => {
    const { driver, click, escape } = displayPage({ closeFails: true });
    await driver.closeDisplayOptions();
    expect(escape).toHaveBeenCalledWith("Escape");
    expect(click).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("stops after Escape when the panel closes", async () => {
    const { driver, click, escape } = displayPage();
    await driver.closeDisplayOptions();
    expect(escape).toHaveBeenCalledWith("Escape");
    expect(click).not.toHaveBeenCalled();
  });

  it("chooses a group-by radio", async () => {
    const { driver, radioClick } = displayPage();
    await driver.setDisplayGroupBy("Priority");
    expect(radioClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("toggles an option only when it is not already in the wanted state", async () => {
    const offPage = displayPage({ pressed: "false" });
    await offPage.driver.setDisplayExtraOption("Show empty groups", true);
    expect(offPage.click).toHaveBeenCalledTimes(1);
    const onPage = displayPage({ pressed: "true" });
    await onPage.driver.setDisplayExtraOption("Show empty groups", true);
    expect(onPage.click).not.toHaveBeenCalled();
    expect(await onPage.driver.isDisplayOptionChecked("Show empty groups")).toBe(true);
  });
});

describe("shared layout methods on the profile tab", () => {
  it("reads the live layout from the trigger's label", async () => {
    const trigger = locator({ getAttribute: async () => "Issue layout, current Board" });
    const page = pageOf({ getByRole: () => locator({ filter: () => locator({ first: () => trigger }) }) });
    const driver = new WebNewDriver(page);
    expect(await driver.layoutsActiveLayout()).toBe("kanban");
  });

  it("rejects a layout label it does not know", async () => {
    const trigger = locator({ getAttribute: async () => "Issue layout, current Gantt" });
    const page = pageOf({ getByRole: () => locator({ filter: () => locator({ first: () => trigger }) }) });
    const driver = new WebNewDriver(page);
    await expect(driver.layoutsActiveLayout()).rejects.toThrow("unrecognised layout trigger label");
  });

  it("switches through the menu choice and waits for the requested surface", async () => {
    const triggerClick = vi.fn(async () => undefined);
    const choiceClick = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () => locator({ filter: () => locator({ first: () => locator({ click: triggerClick }) }) }),
      locator: (selector: string) =>
        selector === '[data-profile-layout-choice="list"]' ? locator({ click: choiceClick }) : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.layoutsSwitchTo("list");
    expect(triggerClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
    expect(choiceClick).toHaveBeenCalledWith({ timeout: WebNewDriver.WAIT_MS });
  });

  it("reloads and waits for the tab's surface", async () => {
    const reload = vi.fn(async () => undefined);
    const page = pageOf({ reload, getByTestId: () => locator({ waitFor: async () => undefined }) });
    const driver = new WebNewDriver(page);
    await driver.layoutsReloadIssues();
    expect(reload).toHaveBeenCalledWith({ waitUntil: "domcontentloaded" });
  });

  it("reads the list and board surfaces", async () => {
    const page = pageOf({ getByTestId: () => locator({ count: async () => 1, isVisible: async () => true }) });
    const driver = new WebNewDriver(page);
    expect(await driver.layoutsListVisible()).toBe(true);
    expect(await driver.layoutsKanbanVisible()).toBe(true);
    expect(await driver.layoutsMobileDisplayVisible()).toBe(true);
  });

  it("reads the list's group titles, dropping blank ones", async () => {
    const groupNodes = [
      locator({ getAttribute: async () => "Urgent" }),
      locator({ getAttribute: async () => "Low" }),
      locator({ getAttribute: async () => " " }),
    ];
    const groups = locator({
      count: async () => groupNodes.length,
      nth: (index: number) => groupNodes[index] ?? groupNodes[0],
    });
    const page = pageOf({ locator: () => groups });
    const driver = new WebNewDriver(page);
    expect(await driver.layoutsListGroups()).toEqual(["Urgent", "Low"]);
  });

  it("reads a group's row names from the name column", async () => {
    const row = locator({
      locator: (selector: string) =>
        selector === "span"
          ? locator({
              nth: (index: number) =>
                locator({ innerText: async () => (index === 1 ? " Prof row " : index === 2 ? "  " : "PP-7") }),
            })
          : locator(),
    });
    const rows = locator({ count: async () => 1, nth: () => row });
    const group = locator({ locator: (selector: string) => (selector === "[data-profile-row]" ? rows : locator()) });
    const page = pageOf({ locator: () => locator({ first: () => group }) });
    const driver = new WebNewDriver(page);
    expect(await driver.layoutsListGroupIssueNames("Urgent")).toEqual(["Prof row"]);
  });

  it("reads the current address", async () => {
    const page = pageOf({ url: () => "https://app.test/acme/profile/u-1/" });
    const driver = new WebNewDriver(page);
    expect(await driver.layoutsCurrentUrl()).toBe("https://app.test/acme/profile/u-1/");
  });
});

describe("profileSidebarAvatar", () => {
  function sidebarPage(initial: StubLocator | null, photo: StubLocator | null): Page {
    const sidebar = locator({
      locator: (selector: string) => {
        if (selector === '[data-profile-avatar="initial"]' && initial !== null) return initial;
        if (selector === '[data-profile-avatar="image"]' && photo !== null) return photo;
        return locator({ count: async () => 0 });
      },
    });
    return pageOf({ getByTestId: () => sidebar });
  }

  it("reads the initial fallback", async () => {
    const page = sidebarPage(locator({ innerText: async () => " O " }), null);
    const driver = new WebNewDriver(page);
    expect(await driver.profileSidebarAvatar()).toEqual({ kind: "initial", initial: "O" });
  });

  it("reads the photo src attribute without depending on the load", async () => {
    const page = sidebarPage(null, locator({ getAttribute: async () => "https://api.test/api/assets/v2/static/a-1/" }));
    const driver = new WebNewDriver(page);
    expect(await driver.profileSidebarAvatar()).toEqual({
      kind: "image",
      src: "https://api.test/api/assets/v2/static/a-1/",
    });
  });

  it("refuses a sidebar with neither marker", async () => {
    const driver = new WebNewDriver(sidebarPage(null, null));
    await expect(driver.profileSidebarAvatar()).rejects.toThrow("neither an avatar image nor an initial");
  });
});

describe("settings shell reads (NEWFRONT-586, PROF-009/010/026)", () => {
  function settingsPage(overrides: Partial<Record<string, unknown>> = {}): {
    page: Page;
    goto: ReturnType<typeof vi.fn>;
    addCookies: ReturnType<typeof vi.fn>;
  } {
    const goto = vi.fn(async () => undefined);
    const addCookies = vi.fn(async () => undefined);
    const page = pageOf({
      context: () => ({ addCookies }),
      goto,
      waitForLoadState: async () => undefined,
      waitForFunction: async () => undefined,
      url: () => "https://app.test/settings/profile/general",
      ...overrides,
    });
    return { page, goto, addCookies };
  }

  function sidebarShell(selectors: Record<string, StubLocator>): (id: string) => Locator {
    return (id: string) => {
      if (id === "profile-settings-sidebar" || id === "profile-settings-shell") {
        return locator({
          count: async () => 1,
          isVisible: async () => true,
          locator: (selector: string) => selectors[selector] ?? locator({ count: async () => 0 }),
          getByRole: (_role: string, options?: unknown) =>
            (selectors[`role:${JSON.stringify(options)}`] as Locator | undefined) ?? locator({ count: async () => 0 }),
          getByText: (_text: string) =>
            (selectors["title"] as Locator | undefined) ?? locator({ count: async () => 0 }),
          allInnerTexts: async () => selectors["headings"]?.allInnerTexts?.() ?? [],
        });
      }
      return locator({ count: async () => 0 });
    };
  }

  it("opens a tab address with cookies and settles on the shell", async () => {
    const waitFor = vi.fn(async () => undefined);
    const { page, goto, addCookies } = settingsPage({
      getByTestId: () => locator({ waitFor }),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileOpen("security", [{ name: "s", value: "v", domain: "app.test", path: "/" }]);
    expect(addCookies).toHaveBeenCalledTimes(1);
    expect(goto).toHaveBeenCalledWith("/settings/profile/security", { waitUntil: "domcontentloaded" });
    expect(waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
  });

  it("opens tab shortcuts and query variants at their addresses", async () => {
    const { page, goto } = settingsPage();
    const driver = new WebNewDriver(page);
    await driver.settingsProfileOpenTab("integrations", []);
    await driver.settingsProfileOpenTabWithQuery("ai-assistant", "keep=1", []);
    expect(goto).toHaveBeenNthCalledWith(1, "/settings/profile/integrations", { waitUntil: "domcontentloaded" });
    expect(goto).toHaveBeenNthCalledWith(2, "/settings/profile/ai-assistant?keep=1", { waitUntil: "domcontentloaded" });
  });

  it("reads the shell, loader, path and query", async () => {
    const { page } = settingsPage({
      url: () => "https://app.test/settings/profile/general?keep=1",
      getByTestId: sidebarShell({}),
      locator: () => locator({ count: async () => 2 }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileShellVisible()).toBe(true);
    expect(await driver.settingsProfileLoaderOnly()).toBe(false);
    expect(await driver.settingsProfileCurrentPath()).toBe("/settings/profile/general");
    expect(await driver.settingsProfileCurrentQuery()).toBe("keep=1");
  });

  it("reports loader-only only when the shell is gone and a pulse shows", async () => {
    const noShell = () => locator({ count: async () => 0 });
    const noPulse = () => locator({ count: async () => 0 });
    const pulsed = new WebNewDriver(settingsPage({ getByTestId: noShell, locator: noPulse }).page);
    expect(await pulsed.settingsProfileLoaderOnly()).toBe(false);
    const withPulse = new WebNewDriver(
      settingsPage({ getByTestId: noShell, locator: () => locator({ count: async () => 1 }) }).page
    );
    expect(await withPulse.settingsProfileLoaderOnly()).toBe(true);
  });

  it("reads group labels, group tabs and shortcuts from their sections", async () => {
    const section = (heading: string, links: string[]): StubLocator => ({
      locator: (selector: string) =>
        selector === "h2"
          ? locator({ textContent: async () => heading })
          : locator({ allInnerTexts: async () => links }),
    });
    const sections = [
      section("Your Profile", ["Profile", "Security"]),
      section("Workspace", ["Acme", "Create workspace"]),
    ];
    const sidebar = locator({
      locator: (selector: string) =>
        selector === "h2"
          ? locator({ allTextContents: async () => ["Your Profile", "Workspace"] })
          : locator({ count: async () => sections.length, nth: (index: number) => sections[index]! }),
    });
    const driver = new WebNewDriver(settingsPage({ getByTestId: () => sidebar }).page);
    expect(await driver.settingsProfileGroupLabels()).toEqual(["Your Profile", "Workspace"]);
    expect(await driver.settingsProfileGroupTabs("Your Profile")).toEqual(["Profile", "Security"]);
    expect(await driver.settingsProfileGroupTabs("Missing")).toEqual([]);
    expect(await driver.settingsProfileWorkspaceShortcuts()).toEqual(["Acme", "Create workspace"]);
  });

  it("reads the back label and sidebar title with absent twins", async () => {
    const present = new WebNewDriver(
      settingsPage({
        getByTestId: sidebarShell({
          'role:{"name":"Back to home"}': locator({ getAttribute: async () => "Back to home" }),
          title: locator({ innerText: async () => "Profile settings" }),
        }),
      }).page
    );
    expect(await present.settingsProfileBackLabel()).toBe("Back to home");
    expect(await present.settingsProfileSidebarTitle()).toBe("Profile settings");
    const absent = new WebNewDriver(settingsPage({ getByTestId: sidebarShell({}) }).page);
    expect(await absent.settingsProfileBackLabel()).toBeNull();
    expect(await absent.settingsProfileSidebarTitle()).toBeNull();
  });

  it("switches tabs by link and reports the landed address", async () => {
    const click = vi.fn(async () => undefined);
    const link = locator({ click });
    const sidebar = locator({ getByRole: () => link });
    const { page, goto } = settingsPage({
      getByTestId: () => sidebar,
      url: () => "https://app.test/settings/profile/security/?keep=2",
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSelectTab("Security");
    expect(click).toHaveBeenCalledTimes(1);
    expect(await driver.settingsProfileOpenPath("/settings?keep=2", [])).toBe("/settings/profile/security/?keep=2");
    expect(goto).toHaveBeenCalledWith("/settings?keep=2", { waitUntil: "domcontentloaded" });
  });

  it("refuses a tab switch that never navigates", async () => {
    const { page } = settingsPage({
      waitForFunction: async () => {
        throw new Error("nope");
      },
    });
    const driver = new WebNewDriver(page);
    await expect(driver.settingsProfileSelectTab("Security")).rejects.toThrow("never navigated");
  });

  it("reports dialogs from the page evaluate", async () => {
    const open = new WebNewDriver(settingsPage({ evaluate: async () => true }).page);
    expect(await open.settingsProfileDialogOpen()).toBe(true);
    const shut = new WebNewDriver(settingsPage({ evaluate: async () => false }).page);
    expect(await shut.settingsProfileDialogOpen()).toBe(false);
  });

  it("opens home, member profiles, preferences and activity at their markers", async () => {
    const waitFor = vi.fn(async () => undefined);
    const waitForResponse = vi.fn(async () => null);
    const { page, goto } = settingsPage({
      getByTestId: () => locator({ waitFor }),
      getByRole: () => locator({ first: () => locator({ waitFor }) }),
      locator: () => locator({ first: () => locator({ waitFor }) }),
      waitForResponse,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileOpenHome("acme");
    await driver.settingsProfileOpenMemberProfile("acme", "u-1");
    await driver.settingsProfileOpenPreferences();
    await driver.settingsProfileOpenActivity();
    expect(goto).toHaveBeenNthCalledWith(1, "/acme", { timeout: 60_000 });
    expect(goto).toHaveBeenNthCalledWith(2, "/acme/profile/u-1", { timeout: 60_000 });
    expect(goto).toHaveBeenNthCalledWith(3, "/settings/profile/preferences", { timeout: 60_000 });
    expect(goto).toHaveBeenNthCalledWith(4, "/settings/profile/activity", { timeout: 60_000 });
    expect(waitForResponse).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenCalled();
  });
});

describe("settings account reads (NEWFRONT-586, PROF-011..015)", () => {
  function formPage(ids: Record<string, StubLocator>): Page {
    const form = locator({
      locator: (selector: string) => ids[selector] ?? locator({ count: async () => 0 }),
      getByRole: () => locator(),
      getByTestId: (id: string) => ids[`testid:${id}`] ?? locator(),
      getByText: (_text: string) => ids["text"] ?? locator({ count: async () => 0 }),
    });
    return pageOf({ getByTestId: () => form });
  }

  it("reads general field errors with absent twins", async () => {
    const page = formPage({ "#first_name-error": locator({ innerText: async () => "Please enter first name" }) });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileGeneralFieldError("first_name")).toBe("Please enter first name");
    expect(await driver.settingsProfileGeneralFieldError("last_name")).toBeNull();
  });

  it("reads the identity header name and email hooks", async () => {
    const header = locator({
      getByTestId: (id: string) =>
        locator({ innerText: async () => (id === "general-identity-name" ? "Opal Whelan" : "opal@example.com") }),
    });
    const page = pageOf({ getByTestId: () => header });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileGeneralIdentity()).toEqual({ name: "Opal Whelan", email: "opal@example.com" });
  });

  it("normalizes the save label", async () => {
    const page = formPage({});
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileGeneralSaveLabel()).toBe("");
  });

  it("reads avatar rejections with absent twins and reports no cover control", async () => {
    const dialog = locator({
      locator: (selector: string) =>
        selector === "[role='alert']"
          ? locator({ innerText: async () => "Too big." })
          : locator({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(pageOf({ getByRole: () => dialog, getByTestId: () => dialog }));
    expect(await driver.settingsProfileAvatarRejection()).toBe("Too big.");
    const bare = new WebNewDriver(pageOf({ getByRole: () => locator({ count: async () => 0 }) }));
    expect(await bare.settingsProfileAvatarRejection()).toBeNull();
    expect(await driver.settingsProfileCoverControlVisible()).toBe(false);
  });

  it("detects email modal steps", async () => {
    const closed = new WebNewDriver(pageOf({ getByRole: () => locator({ count: async () => 0 }) }));
    expect(await closed.settingsProfileEmailModalStep()).toBe("closed");
    const emailOnly = locator({ getByLabel: () => locator({ count: async () => 0 }) });
    expect(await new WebNewDriver(pageOf({ getByRole: () => emailOnly })).settingsProfileEmailModalStep()).toBe(
      "email"
    );
    const code = locator({ getByLabel: () => locator({ count: async () => 1 }) });
    expect(await new WebNewDriver(pageOf({ getByRole: () => code })).settingsProfileEmailModalStep()).toBe("code");
  });

  it("reads email modal errors and helpers with absent twins", async () => {
    const dialog = locator({
      locator: (selector: string) =>
        selector === "#change-email-code-error"
          ? locator({ innerText: async () => "Invalid verification code" })
          : locator({ count: async () => 0 }),
      getByText: () => locator({ innerText: async () => "Verification code sent to your new email." }),
    });
    const driver = new WebNewDriver(pageOf({ getByRole: () => dialog }));
    expect(await driver.settingsProfileEmailModalError("code")).toBe("Invalid verification code");
    expect(await driver.settingsProfileEmailModalError("email")).toBeNull();
    expect(await driver.settingsProfileEmailModalHelper()).toBe("Verification code sent to your new email.");
    const bareDialog = locator({ count: async () => 0, getByText: () => locator({ count: async () => 0 }) });
    const bare = new WebNewDriver(pageOf({ getByRole: () => bareDialog }));
    expect(await bare.settingsProfileEmailModalHelper()).toBeNull();
  });

  it("reads deactivate titles, descriptions and working flags", async () => {
    const dialog = locator({ getByText: () => locator({ innerText: async () => "Deactivate your account" }) });
    const sections = locator({ filter: () => dialog });
    const driver = new WebNewDriver(pageOf({ getByRole: () => dialog, locator: () => sections }));
    expect(await driver.settingsProfileDeactivateTitle()).toBe("Deactivate your account");
    const absent = locator({ count: async () => 0, filter: () => locator({ count: async () => 0 }) });
    const bare = new WebNewDriver(pageOf({ getByRole: () => absent, locator: () => absent }));
    expect(await bare.settingsProfileDeactivateTitle()).toBeNull();
    expect(await bare.settingsProfileDeactivateDescription()).toBeNull();
    expect(await bare.settingsProfileDeactivateWorking()).toBe(false);
  });

  it("maps security fields to their input ids and reveal hooks", async () => {
    const seen: string[] = [];
    const form = locator({
      locator: (selector: string) => {
        seen.push(selector);
        return locator({
          fill: async () => undefined,
          inputValue: async () => "v",
          getAttribute: async () => "password",
        });
      },
      getByTestId: (id: string) => {
        seen.push(`testid:${id}`);
        return locator();
      },
    });
    const driver = new WebNewDriver(pageOf({ getByTestId: () => form }));
    await driver.settingsProfileSecurityFill("current", "x");
    await driver.settingsProfileSecurityToggleReveal("new");
    await driver.settingsProfileSecurityToggleReveal("confirm");
    expect(await driver.settingsProfileSecurityFieldType("new")).toBe("password");
    expect(await driver.settingsProfileSecurityFieldValue("confirm")).toBe("v");
    expect(seen).toContain("#old_password");
    expect(seen).toContain("#new_password");
    expect(seen).toContain("#confirm_password");
    expect(seen).toContain("testid:password-reveal-next");
    expect(seen).toContain("testid:password-reveal-confirm");
    expect(await driver.settingsProfileSecurityFieldError("new")).toBeNull();
  });

  it("fails profile saves through a route and clears it", async () => {
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, unroute }));
    await driver.settingsProfileFailProfileSaves(500);
    expect(route).toHaveBeenCalledWith("**/api/users/me/profile/", expect.any(Function));
    await driver.settingsProfileClearProfileSaveFailure();
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/profile/");
  });
});

describe("settings preferences (NEWFRONT-586, PROF-016..018)", () => {
  function responseOf(url: string, method: string): unknown {
    return { request: () => ({ method: () => method }), url: () => url };
  }

  it("maps the theme trigger label to its value and nulls unknowns", async () => {
    const driver = new WebNewDriver(
      pageOf({ getByRole: () => locator({ textContent: async () => "Dark high contrast" }) })
    );
    expect(await driver.settingsProfileThemeValue()).toBe("dark-contrast");
    const unknown = new WebNewDriver(pageOf({ getByRole: () => locator({ textContent: async () => "Mystery" }) }));
    expect(await unknown.settingsProfileThemeValue()).toBeNull();
  });

  it("picks a theme through the combobox and settles on the saved pick", async () => {
    let current = "Light";
    const fill = vi.fn(async () => undefined);
    const seen: Array<Record<string, unknown>> = [];
    const waitForResponse = vi.fn(async () => ({ ok: () => true }));
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) => {
        seen.push({ role, ...(options ?? {}) });
        if (role === "combobox") return locator({ fill });
        if (role === "option") {
          return locator({
            click: async () => {
              current = "Dark";
            },
          });
        }
        return locator({ textContent: async () => current });
      },
      waitForResponse,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSetTheme("dark");
    expect(fill).toHaveBeenCalledWith("Dark");
    expect(seen).toContainEqual({ role: "option", name: "Dark", exact: true });
    expect(waitForResponse).toHaveBeenCalledTimes(1);
    const matches = waitForResponse.mock.calls[0]![0] as (response: unknown) => boolean;
    expect(matches(responseOf("https://app.test/api/users/me/profile/", "PATCH"))).toBe(true);
    expect(matches(responseOf("https://app.test/api/users/me/profile/", "GET"))).toBe(false);
    expect(await driver.settingsProfileThemeValue()).toBe("dark");
  });

  it("settles a rejected theme save back on the previous theme", async () => {
    const page = pageOf({
      getByRole: (role: string) => {
        if (role === "combobox") return locator({ fill: async () => undefined });
        if (role === "option") return locator({ click: async () => undefined });
        return locator({ textContent: async () => "Light" });
      },
      waitForResponse: async () => ({ ok: () => false }),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSetTheme("dark");
    expect(await driver.settingsProfileThemeValue()).toBe("light");
  });

  it("reads the applied theme and custom vars from the document", async () => {
    const applied = new WebNewDriver(pageOf({ evaluate: async () => "dark" }));
    expect(await applied.settingsProfileAppliedTheme()).toBe("dark");
    const vars = new WebNewDriver(pageOf({ evaluate: async () => true }));
    expect(await vars.settingsProfileCustomVarsApplied()).toBe(true);
    const bare = new WebNewDriver(pageOf({ evaluate: async () => false }));
    expect(await bare.settingsProfileCustomVarsApplied()).toBe(false);
  });

  it("reads designer visibility and values with absent twins", async () => {
    const shown = new WebNewDriver(
      pageOf({
        locator: (selector: string) =>
          locator({ inputValue: async () => (selector === 'input[name="primary"]' ? "#ff5151" : "#1a2b3c") }),
        getByRole: () => locator({ isChecked: async () => true }),
      })
    );
    expect(await shown.settingsProfileCustomDesignerVisible()).toBe(true);
    expect(await shown.settingsProfileCustomDesignerValues()).toEqual({
      primary: "#ff5151",
      background: "#1a2b3c",
      darkPalette: true,
    });
    const hidden = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    expect(await hidden.settingsProfileCustomDesignerVisible()).toBe(false);
  });

  it("previews a typed color on its swatch", async () => {
    const fill = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      locator: () => locator({ fill }),
      getByLabel: (label: string) => locator({ waitFor, evaluate: async () => `swatch:${label}` }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfilePreviewCustomColor("primary", "#ff5151")).toBe("swatch:Primary preview");
    expect(fill).toHaveBeenCalledWith("#ff5151");
    expect(waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
    expect(await driver.settingsProfilePreviewCustomColor("background", "#1a2b3c")).toBe("swatch:Background preview");
  });

  it("sets the palette and saves the custom theme past the reload", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const waitForResponse = vi.fn(async () => ({ ok: () => true }));
    const page = pageOf({
      getByRole: () => locator({ click }),
      getByTestId: () => locator({ waitFor }),
      waitForResponse,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSetCustomPalette(true);
    expect(click).toHaveBeenCalledTimes(1);
    await driver.settingsProfileSaveCustomTheme();
    expect(waitForResponse).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenNthCalledWith(1, { state: "detached", timeout: 30_000 });
    expect(waitFor).toHaveBeenNthCalledWith(2, { state: "visible", timeout: 60_000 });
  });

  it("reads the locale trigger labels", async () => {
    const labels: Record<string, string> = {
      Language: "English",
      Timezone: "(UTC+14:00) Kiritimati",
      "First day of the week": "Monday",
    };
    const page = pageOf({
      getByRole: (_role: string, options?: Record<string, unknown>) =>
        locator({ textContent: async () => labels[options?.["name"] as string] ?? "" }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileLocaleValues()).toEqual({
      language: "English",
      timezone: "(UTC+14:00) Kiritimati",
      weekStart: "Monday",
    });
  });

  it("picks a timezone by its catalog label and settles on it", async () => {
    let current = "(UTC-11:00) Midway";
    const fill = vi.fn(async () => undefined);
    const seen: Array<Record<string, unknown>> = [];
    const waitForResponse = vi.fn(async () => ({ ok: () => true }));
    const page = pageOf({
      evaluate: async () => [{ value: "Pacific/Kiritimati", label: "(UTC+14:00) Kiritimati" }],
      getByRole: (role: string, options?: Record<string, unknown>) => {
        seen.push({ role, ...(options ?? {}) });
        if (role === "combobox") return locator({ fill });
        if (role === "option") {
          return locator({
            click: async () => {
              current = "(UTC+14:00) Kiritimati";
            },
          });
        }
        return locator({ textContent: async () => current });
      },
      waitForResponse,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSetTimezone("Pacific/Kiritimati");
    expect(fill).toHaveBeenCalledWith("Pacific/Kiritimati");
    expect(seen).toContainEqual({ role: "option", name: "(UTC+14:00) Kiritimati", exact: true });
    const matches = waitForResponse.mock.calls[0]![0] as (response: unknown) => boolean;
    expect(matches(responseOf("https://app.test/api/users/me/", "PATCH"))).toBe(true);
    expect(await driver.settingsProfileLocaleValues()).toMatchObject({ timezone: "(UTC+14:00) Kiritimati" });
  });

  it("picks a language and a week start through their comboboxes", async () => {
    const triggers: Record<string, string> = { Language: "fr", "First day of the week": "Sunday" };
    const fills: string[] = [];
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) => {
        if (role === "combobox") {
          return locator({
            fill: async (value: string) => {
              fills.push(value);
            },
          });
        }
        if (role === "option") {
          return locator({
            click: async () => {
              if (fills[fills.length - 1] === "English") triggers["Language"] = "English";
              else triggers["First day of the week"] = fills[fills.length - 1]!;
            },
          });
        }
        return locator({ textContent: async () => triggers[options?.["name"] as string] ?? "" });
      },
      waitForResponse: async () => ({ ok: () => true }),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileSetLanguage("English");
    await driver.settingsProfileSetWeekStart("Wednesday");
    expect(fills).toEqual(["English", "Wednesday"]);
    expect(await driver.settingsProfileLocaleValues()).toEqual({
      language: "English",
      timezone: "",
      weekStart: "Wednesday",
    });
  });

  it("pads the greeting clock and reads the sidebar clock with absent twins", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        locator({
          textContent: async () =>
            selector === "#home-date-line" ? "Thursday, October 23 3:45 PM" : " 15:04 Pacific/Kiritimati ",
        }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileGreetingClockText()).toBe("03:45");
    expect(await driver.settingsProfileSidebarClockText()).toBe("15:04 Pacific/Kiritimati");
    const bare = new WebNewDriver(pageOf({ locator: () => locator({ textContent: async () => "" }) }));
    expect(await bare.settingsProfileGreetingClockText()).toBeNull();
    expect(await bare.settingsProfileSidebarClockText()).toBeNull();
  });

  it("reads the calendar header order and drops blanks", async () => {
    const order = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const seen: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        seen.push(selector);
        return locator({
          count: async () => order.length,
          nth: (index: number) => locator({ textContent: async () => ` ${order[index]!} ` }),
        });
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileCalendarDayOrder()).toEqual(order);
    expect(seen).toEqual(['[data-testid="issues-calendar"] .sticky > div > div']);
  });

  it("switches project layouts through data-layout and detects the calendar", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const seen: string[] = [];
    const page = pageOf({
      getByRole: () => locator({ count: async () => 0 }),
      locator: (selector: string) => {
        seen.push(selector);
        return locator({
          filter: () => locator({ first: () => locator({ click }) }),
          first: () => locator({ waitFor }),
        });
      },
      getByTestId: () => locator({ waitFor, isVisible: async () => true }),
    });
    const driver = new WebNewDriver(page);
    await driver.layoutsSwitchTo("calendar");
    expect(click).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    expect(await driver.layoutsCalendarVisible()).toBe(true);
    await driver.layoutsSwitchTo("list");
    // The list settle matches any labelled box: rows, empty and error
    // twins use different elements.
    expect(seen).toContain('[aria-label="Issue list"], [aria-label="Issues"]');
    const bare = new WebNewDriver(pageOf({ getByTestId: () => locator({ count: async () => 0 }) }));
    expect(await bare.layoutsCalendarVisible()).toBe(false);
  });

  it("keeps the profile-tab layout path when its trigger is present", async () => {
    const clicks: string[] = [];
    const page = pageOf({
      getByRole: () => locator({ filter: () => locator({ first: () => locator({ click: async () => undefined }) }) }),
      locator: (selector: string) =>
        locator({
          click: async () => {
            clicks.push(selector);
          },
        }),
      getByTestId: () => locator({ waitFor: async () => undefined }),
    });
    const driver = new WebNewDriver(page);
    await driver.layoutsSwitchTo("kanban");
    expect(clicks).toEqual(['[data-profile-layout-choice="kanban"]']);
  });

  it("opens the cycle page on the shared list section", async () => {
    const goto = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      goto,
      waitForLoadState: async () => undefined,
      locator: () => locator({ first: () => locator({ waitFor }) }),
    });
    const driver = new WebNewDriver(page);
    await driver.openCyclePage("acme", "p-1", "c-1");
    expect(goto).toHaveBeenCalledWith("/acme/projects/p-1/cycles/c-1", { timeout: 60_000 });
    expect(waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
  });

  it("leaves the cycle timezone dates to the area that renders them", async () => {
    const driver = new WebNewDriver(pageOf({}));
    await expect(driver.settingsProfileCycleUserTimezoneDates()).rejects.toThrow(
      "drivers/web_new has no settingsProfileCycleUserTimezoneDates yet"
    );
  });
});

describe("settings own activity (NEWFRONT-586, PROF-019)", () => {
  it("counts entries and detects the load-more control with absent twins", async () => {
    const page = pageOf({
      getByTestId: () => locator({ count: async () => 100 }),
      getByRole: () => locator({ count: async () => 1, isVisible: async () => true }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileOwnActivityEntryCount()).toBe(100);
    expect(await driver.settingsProfileOwnActivityLoadMoreVisible()).toBe(true);
    const bare = new WebNewDriver(
      pageOf({
        getByTestId: () => locator({ count: async () => 0 }),
        getByRole: () => locator({ count: async () => 0 }),
      })
    );
    expect(await bare.settingsProfileOwnActivityEntryCount()).toBe(0);
    expect(await bare.settingsProfileOwnActivityLoadMoreVisible()).toBe(false);
  });

  it("loads more until the row count grows", async () => {
    let rows = 100;
    const click = vi.fn(async () => {
      rows = 105;
    });
    const page = pageOf({
      getByTestId: () => locator({ count: async () => rows }),
      getByRole: () => locator({ click }),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileOwnActivityLoadMore();
    expect(click).toHaveBeenCalledTimes(1);
    expect(await driver.settingsProfileOwnActivityEntryCount()).toBe(105);
  });

  it("reads the empty illustration art with absent twins", async () => {
    const seen: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        seen.push(selector);
        return locator({ getAttribute: async () => "/empty-state/activity/activity-dark.svg" });
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileOwnActivityEmptyIllustration()).toBe("/empty-state/activity/activity-dark.svg");
    expect(seen).toEqual(['section[aria-labelledby="own-activity-heading"] img']);
    const bare = new WebNewDriver(pageOf({ locator: () => locator({ count: async () => 0 }) }));
    expect(await bare.settingsProfileOwnActivityEmptyIllustration()).toBeNull();
  });
});

describe("settings assistant (NEWFRONT-586, PROF-021..023)", () => {
  function sectionPage(section: StubLocator): Page {
    return pageOf({ getByTestId: () => section });
  }

  it("reads the newest toast as title plus body with absent twins", async () => {
    const cards = locator({
      last: () => locator({ innerText: async () => " Saved \n AI provider configuration updated. \n" }),
    });
    const driver = new WebNewDriver(pageOf({ getByTestId: () => cards }));
    expect(await driver.settingsProfileLastToast()).toEqual({
      title: "Saved",
      message: "AI provider configuration updated.",
    });
    const bare = new WebNewDriver(pageOf({ getByTestId: () => locator({ count: async () => 0 }) }));
    expect(await bare.settingsProfileLastToast()).toBeNull();
    const blank = new WebNewDriver(
      pageOf({ getByTestId: () => locator({ last: () => locator({ innerText: async () => "  " }) }) })
    );
    expect(await blank.settingsProfileLastToast()).toBeNull();
  });

  it("reads the assistant facts and drives its controls", async () => {
    const clicks: string[] = [];
    const fills: Array<[string, string]> = [];
    const selected: string[] = [];
    const section = locator({
      locator: (selector: string) => {
        if (selector === "#assistant-llm-base-url") return locator({ inputValue: async () => "http://dead:9" });
        if (selector === "#assistant-llm-provider") {
          return locator({
            inputValue: async () => "openai_compatible",
            selectOption: async (value: string) => {
              selected.push(value);
            },
          });
        }
        if (selector === "#assistant-llm-model")
          return locator({
            inputValue: async () => "probe-model",
            fill: async (value: string) => {
              fills.push([selector, value]);
            },
          });
        return locator({
          inputValue: async () => "",
          fill: async (value: string) => {
            fills.push([selector, value]);
          },
          getAttribute: async () => "Your provider API key",
        });
      },
      getByRole: (_role: string, options?: Record<string, unknown>) => {
        const name = options?.["name"] as string;
        return locator({
          isDisabled: async () => name === "Save",
          click: async () => {
            clicks.push(name);
          },
        });
      },
      getByText: () => locator({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(sectionPage(section));
    expect(await driver.settingsProfileAssistantFacts()).toEqual({
      provider: "openai_compatible",
      baseUrlVisible: true,
      baseUrl: "http://dead:9",
      model: "probe-model",
      keyValue: "",
      keyPlaceholder: "Your provider API key",
      saveDisabled: true,
      testDisabled: false,
      removeVisible: true,
      lastVerified: null,
    });
    await driver.settingsProfileAssistantSetProvider("anthropic");
    await driver.settingsProfileAssistantFill("model", "m");
    await driver.settingsProfileAssistantSave();
    await driver.settingsProfileAssistantTest();
    await driver.settingsProfileAssistantRemove();
    expect(selected).toEqual(["anthropic"]);
    expect(fills).toEqual([["#assistant-llm-model", "m"]]);
    expect(clicks).toEqual(["Save", "Test connection", "Remove"]);
  });

  it("blanks the base URL when the endpoint hides", async () => {
    const section = locator({
      locator: (selector: string) =>
        selector === "#assistant-llm-base-url"
          ? locator({ count: async () => 0 })
          : locator({ inputValue: async () => "", getAttribute: async () => "" }),
      getByRole: () => locator({ isDisabled: async () => false }),
      getByText: () => locator({ innerText: async () => "Last verified: whenever" }),
    });
    const driver = new WebNewDriver(sectionPage(section));
    expect(await driver.settingsProfileAssistantFacts()).toMatchObject({
      baseUrlVisible: false,
      baseUrl: "",
      lastVerified: "Last verified: whenever",
    });
  });

  it("fails assistant writes through routes and clears them", async () => {
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, unroute }));
    await driver.settingsProfileAssistantFailWrites();
    expect(route).toHaveBeenCalledWith("**/api/users/me/ai-assistant/**", expect.any(Function));
    const handler = route.mock.calls[0]![1] as (route: unknown) => void;
    const fulfilled: unknown[] = [];
    const continued: number[] = [];
    const put = {
      request: () => ({ method: () => "PUT", url: () => "https://app.test/api/users/me/ai-assistant/config/" }),
      fulfill: async (body: unknown) => {
        fulfilled.push(body);
      },
      continue: async () => {
        continued.push(1);
      },
    };
    handler(put);
    expect(fulfilled).toEqual([{ status: 500, contentType: "application/json", body: '{"error":"probe_boom"}' }]);
    handler({
      request: () => ({ method: () => "GET", url: () => "https://app.test/api/users/me/ai-assistant/config/" }),
      fulfill: async () => undefined,
      continue: async () => {
        continued.push(1);
      },
    });
    expect(continued).toHaveLength(1);
    await driver.settingsProfileAssistantClearRoutes();
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/ai-assistant/**");
  });

  it("reads dictation facts with a hidden twin and drives its controls", async () => {
    const section = locator({
      locator: (selector: string) => {
        if (selector === "#assistant-voice-base-url") return locator({ inputValue: async () => "http://dead:9" });
        if (selector === "#assistant-voice-model") return locator({ inputValue: async () => "whisper-1" });
        return locator({ inputValue: async () => "", getAttribute: async () => "Your transcription API key" });
      },
      getByRole: () => locator({ isDisabled: async () => true, count: async () => 0 }),
      getByText: () => locator({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(sectionPage(section));
    expect(await driver.settingsProfileDictationFacts()).toEqual({
      sectionVisible: true,
      baseUrl: "http://dead:9",
      model: "whisper-1",
      keyValue: "",
      keyPlaceholder: "Your transcription API key",
      saveDisabled: true,
      testDisabled: true,
      removeVisible: false,
      lastVerified: null,
    });
    const hidden = new WebNewDriver(pageOf({ getByTestId: () => locator({ count: async () => 0 }) }));
    expect(await hidden.settingsProfileDictationFacts()).toMatchObject({ sectionVisible: false });
    const fills: string[] = [];
    const clicks: string[] = [];
    const controls = locator({
      locator: () =>
        locator({
          fill: async (value: string) => {
            fills.push(value);
          },
        }),
      getByRole: (_role: string, options?: Record<string, unknown>) =>
        locator({
          click: async () => {
            clicks.push(options?.["name"] as string);
          },
        }),
    });
    const writer = new WebNewDriver(sectionPage(controls));
    await writer.settingsProfileDictationFill("key", "k");
    await writer.settingsProfileDictationSave();
    await writer.settingsProfileDictationTest();
    await writer.settingsProfileDictationRemove();
    expect(fills).toEqual(["k"]);
    expect(clicks).toEqual(["Save", "Test connection", "Remove"]);
  });

  it("reads tool server rows with disabled, locked and prefix states", async () => {
    const rows = [
      {
        name: "probe-mcp-a",
        url: "https://mcp-a.example.com/sse",
        disabled: false,
        locked: false,
        prefix: "probe_mcp_a_*",
      },
      { name: "probe-mcp-b", url: "https://mcp-b.example.com/mcp", disabled: true, locked: true, prefix: null },
    ];
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id !== "tool-server-row") return locator({ isDisabled: async () => true });
        return locator({
          count: async () => rows.length,
          nth: (index: number) => {
            const row = rows[index]!;
            return locator({
              getAttribute: async () => row.name,
              getByTestId: () => locator({ innerText: async () => row.url }),
              getByRole: (_role: string, options?: Record<string, unknown>) =>
                locator({
                  count: async () =>
                    (options?.["name"] as string) === `Enable ${row.name}`
                      ? row.disabled
                        ? 1
                        : 0
                      : row.locked
                        ? 1
                        : 0,
                }),
              locator: () =>
                locator({
                  count: async () => (row.prefix === null ? 0 : 1),
                  first: () => locator({ innerText: async () => row.prefix ?? "" }),
                }),
            });
          },
        });
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileToolServerRows()).toEqual(rows);
    expect(await driver.settingsProfileToolServerAddDisabled()).toBe(true);
  });

  it("adds, toggles and removes tool servers through the section", async () => {
    const fills: Array<[string, string]> = [];
    const clicks: string[] = [];
    const section = locator({
      getByLabel: (label: string) =>
        locator({
          fill: async (value: string) => {
            fills.push([label, value]);
          },
        }),
      getByRole: (_role: string, options?: Record<string, unknown>) => {
        const name = options?.["name"] as string;
        return locator({
          count: async () => (name === "Disable probe-mcp-a" ? 1 : 1),
          first: () =>
            locator({
              click: async () => {
                clicks.push(name);
              },
            }),
          click: async () => {
            clicks.push(name);
          },
        });
      },
    });
    const page = pageOf({
      getByTestId: () => section,
      locator: () => section,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileToolServerAdd("probe-mcp-b", "https://mcp-b.example.com/mcp", "Bearer x");
    expect(fills).toEqual([
      ["Name", "probe-mcp-b"],
      ["Server URL", "https://mcp-b.example.com/mcp"],
      ["Authorization header (optional)", "Bearer x"],
    ]);
    await driver.settingsProfileToolServerAdd("probe-mcp-a", "https://mcp-a.example.com/sse");
    expect(fills).toHaveLength(5);
    await driver.settingsProfileToolServerToggle("probe-mcp-a");
    await driver.settingsProfileToolServerRemove("probe-mcp-b");
    expect(clicks).toContain("Add server");
    expect(clicks).toContain("Disable probe-mcp-a");
    expect(clicks).toContain("Remove");
  });
});

describe("settings upkeep and integrations (NEWFRONT-586, PROF-024..025)", () => {
  function upkeepPage(
    jobs: { name: string; interval: string; description: string; enabled: boolean; disabled: boolean }[]
  ): {
    page: Page;
    clicks: string[];
  } {
    const clicks: string[] = [];
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "upkeep-job") {
          return locator({
            count: async () => jobs.length,
            nth: (index: number) => {
              const job = jobs[index]!;
              return locator({
                locator: () =>
                  locator({
                    nth: (span: number) =>
                      locator({ innerText: async () => [job.name, job.interval, job.description][span]! }),
                  }),
                getByRole: () =>
                  locator({
                    first: () =>
                      locator({
                        getAttribute: async () => (job.enabled ? "true" : "false"),
                        isDisabled: async () => job.disabled,
                        click: async () => {
                          clicks.push(`job:${job.name}`);
                        },
                      }),
                  }),
              });
            },
          });
        }
        return locator({
          getAttribute: async () => "false",
          click: async () => {
            clicks.push("master");
          },
        });
      },
      getByText: () => locator({ count: async () => 0 }),
    });
    return { page, clicks };
  }

  it("reads upkeep facts with an empty twin", async () => {
    const { page } = upkeepPage([
      {
        name: "Close issues when their PR merges",
        interval: "daily",
        description: "marks an issue Done",
        enabled: true,
        disabled: false,
      },
    ]);
    const driver = new WebNewDriver(page);
    expect(await driver.settingsProfileUpkeepFacts()).toEqual({
      emptyExplanation: null,
      masterPaused: false,
      jobs: [
        {
          name: "Close issues when their PR merges",
          interval: "daily",
          description: "marks an issue Done",
          enabled: true,
          toggleDisabled: false,
        },
      ],
    });
    const empty = new WebNewDriver(
      pageOf({
        getByTestId: () => locator(),
        getByText: () => locator({ innerText: async () => "Nothing is scheduled on this instance yet." }),
      })
    );
    expect(await empty.settingsProfileUpkeepFacts()).toEqual({
      emptyExplanation: "Nothing is scheduled on this instance yet.",
      masterPaused: null,
      jobs: [],
    });
  });

  it("toggles the master and jobs past their write settles", async () => {
    const waitForResponse = vi.fn(async () => ({ ok: () => true }));
    const { page, clicks } = upkeepPage([
      {
        name: "Close issues when their PR merges",
        interval: "daily",
        description: "d",
        enabled: true,
        disabled: false,
      },
    ]);
    const driver = new WebNewDriver(pageOf({ ...(page as unknown as Record<string, unknown>), waitForResponse }));
    await driver.settingsProfileUpkeepToggleMaster(true);
    await driver.settingsProfileUpkeepToggleJob("Close issues when their PR merges", false);
    expect(clicks).toEqual(["master", "job:Close issues when their PR merges"]);
    expect(waitForResponse).toHaveBeenCalledTimes(2);
    const masterMatches = waitForResponse.mock.calls[0]![0] as (response: unknown) => boolean;
    const jobMatches = waitForResponse.mock.calls[1]![0] as (response: unknown) => boolean;
    const response = (url: string, method: string): unknown => ({
      request: () => ({ method: () => method }),
      url: () => url,
    });
    expect(masterMatches(response("https://app.test/api/users/me/auto-pm/", "PATCH"))).toBe(true);
    expect(masterMatches(response("https://app.test/api/users/me/auto-pm/jobs/x/", "PATCH"))).toBe(false);
    expect(jobMatches(response("https://app.test/api/users/me/auto-pm/jobs/x/", "PATCH"))).toBe(true);
    await expect(driver.settingsProfileUpkeepToggleJob("missing", true)).rejects.toThrow("not found");
  });

  it("fails and delays upkeep writes through routes", async () => {
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route, unroute }));
    await driver.settingsProfileUpkeepFailWrites();
    expect(route).toHaveBeenCalledWith("**/api/users/me/auto-pm/**", expect.any(Function));
    const fail = route.mock.calls[0]![1] as (route: unknown) => void;
    const fulfilled: unknown[] = [];
    fail({
      request: () => ({ method: () => "PATCH", url: () => "https://app.test/api/users/me/auto-pm/" }),
      fulfill: async (body: unknown) => {
        fulfilled.push(body);
      },
      continue: async () => undefined,
    });
    expect(fulfilled).toEqual([{ status: 500, contentType: "application/json", body: '{"error":"probe_boom"}' }]);
    await driver.settingsProfileUpkeepDelayWrites(3000);
    expect(route).toHaveBeenCalledTimes(2);
    await driver.settingsProfileUpkeepClearRoutes();
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/auto-pm/**");
  });

  it("reads integrations facts across the settled twins", async () => {
    const panel = locator({
      getByRole: (role: string, options?: Record<string, unknown>) => {
        const name = options?.["name"] as string;
        if (role === "button" && name === "Reconnect")
          return locator({
            count: async () => 1,
            first: () => locator({ innerText: async () => "Reconnect", isDisabled: async () => true }),
          });
        if (role === "button" && name === "Refresh")
          return locator({ count: async () => 1, isVisible: async () => true, first: () => locator({}) });
        return locator({ count: async () => 0 });
      },
      getByTestId: () => locator({ first: () => locator({ evaluate: async () => [] }) }),
      getByText: (text: string) =>
        locator({
          count: async () => (String(text).includes("No GitHub App installation") ? 1 : 0),
          first: () => locator({ innerText: async () => "No GitHub App installation is connected yet." }),
        }),
      locator: (selector: string) =>
        selector === "h3"
          ? locator({ first: () => locator({ innerText: async () => "GitHub App" }) })
          : locator({ first: () => locator({ innerText: async () => "Install the Pi Dash GitHub App for Acme." }) }),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "#integrations-workspace") {
          return locator({
            first: () =>
              locator({
                evaluate: async () => [{ slug: "acme", name: "Acme" }],
                inputValue: async () => "acme",
              }),
          });
        }
        return locator();
      },
      getByTestId: (id: string) => {
        if (id === "integrations-settings-loading") return locator({ count: async () => 0 });
        if (id === "integrations-no-admin") return locator({ count: async () => 0 });
        return panel;
      },
    });
    const driver = new WebNewDriver(page);
    const facts = await driver.settingsProfileIntegrationsFacts();
    expect(facts).toMatchObject({
      loading: false,
      noAdmin: false,
      connected: true,
      installLabel: "Reconnect",
      refreshVisible: true,
      panelHeading: "GitHub App",
      selectedSlug: "acme",
    });
    expect(facts.workspaceOptions).toEqual([{ slug: "acme", name: "Acme" }]);
    expect(facts.unconnectedCopy).toBe("No GitHub App installation is connected yet.");
  });

  it("reports the loading and no-admin twins", async () => {
    const loading = new WebNewDriver(
      pageOf({
        getByTestId: (id: string) =>
          id === "integrations-settings-loading"
            ? locator({ isVisible: async () => true })
            : locator({ count: async () => 0 }),
      })
    );
    expect(await loading.settingsProfileIntegrationsFacts()).toMatchObject({ loading: true });
    const noAdmin = new WebNewDriver(
      pageOf({
        locator: () => locator({ count: async () => 0 }),
        getByTestId: (id: string) => {
          if (id === "integrations-settings-loading") return locator({ count: async () => 0 });
          if (id === "integrations-no-admin") return locator({});
          return locator({ count: async () => 0 });
        },
      })
    );
    expect(await noAdmin.settingsProfileIntegrationsFacts()).toMatchObject({
      loading: false,
      noAdmin: true,
      panelHeading: null,
    });
  });

  it("selects workspaces, refreshes and fails the refresh through routes", async () => {
    const selected: string[] = [];
    const clicks: string[] = [];
    const route = vi.fn(async () => undefined);
    const unroute = vi.fn(async () => undefined);
    const page = pageOf({
      locator: () =>
        locator({
          evaluate: async () => "Acme",
          selectOption: async (slug: string) => {
            selected.push(slug);
          },
        }),
      getByTestId: () =>
        locator({
          getByRole: () =>
            locator({
              click: async () => {
                clicks.push("refresh");
              },
            }),
        }),
      waitForFunction: async () => undefined,
      waitForResponse: async () => ({ ok: () => true }),
      route,
      unroute,
    });
    const driver = new WebNewDriver(page);
    await driver.settingsProfileIntegrationsSelect("acme");
    expect(selected).toEqual(["acme"]);
    await driver.settingsProfileIntegrationsRefresh();
    expect(clicks).toEqual(["refresh"]);
    await driver.settingsProfileIntegrationsFailRefresh();
    await driver.settingsProfileIntegrationsClearRoutes();
    expect(route).toHaveBeenCalledWith("**/api/users/me/integrations/github/app/refresh/", expect.any(Function));
    expect(unroute).toHaveBeenCalledWith("**/api/users/me/integrations/github/app/refresh/");
    const missing = new WebNewDriver(
      pageOf({ locator: () => locator({ evaluate: async () => "" }), waitForFunction: async () => undefined })
    );
    await expect(missing.settingsProfileIntegrationsSelect("nope")).rejects.toThrow("has no option");
  });
});
