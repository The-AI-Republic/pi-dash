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
