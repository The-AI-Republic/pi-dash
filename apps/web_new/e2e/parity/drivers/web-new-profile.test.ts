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
  it("installs one route, holds the export request, then releases it", async () => {
    const held: Route = { continue: vi.fn(async () => undefined) } as unknown as Route;
    let installed: ((route: Route) => Promise<void>) | null = null;
    const route = vi.fn(async (_pattern: string, handler: (route: Route) => Promise<void>) => {
      installed = handler;
    });
    const unroute = vi.fn(async () => undefined);
    // The stall uses `expect.poll`; give it a passing target by resolving now.
    const page = pageOf({ route, unroute });
    const driver = new WebNewDriver(page);
    await driver.profileActivityStallExport();
    await driver.profileActivityStallExport();
    expect(route).toHaveBeenCalledTimes(1);
    expect(route.mock.calls[0][0]).toBe("**/api/workspaces/*/user-activity/*/export/**");
    await installed!(held);
    await driver.profileActivityReleaseExport();
    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/user-activity/*/export/**", expect.anything());
    expect(held.continue).toHaveBeenCalled();
  });

  it("releases nothing when no stall was installed", async () => {
    const unroute = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ unroute }));
    await driver.profileActivityReleaseExport();
    expect(unroute).not.toHaveBeenCalled();
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
