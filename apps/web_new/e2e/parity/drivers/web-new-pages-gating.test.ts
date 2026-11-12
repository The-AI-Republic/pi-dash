// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the pages gating / list-state / URL / detail-shell driver
// family (NEWFRONT-314, PAGE-018-025,079-080). A Page double answers the
// data-testid and accessible-name probes, and the armed route families are
// invoked directly so both the hold/fail and the fallback branches run
// without a browser or the parity stack.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

interface LocatorOptions {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: unknown) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  inputValue?: (options?: unknown) => Promise<string>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  evaluate?: (fn: (node: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  all?: () => Promise<Locator[]>;
  nth?: (index: number) => Locator;
  last?: () => Locator;
  locator?: (selector: string) => Locator;
  role?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator;
  testId?: (id: string) => Locator;
  text?: (text: string) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
}

/** A locator double; children of a node fall back to the node itself. */
function loc(overrides: LocatorOptions = {}): Locator {
  const node: Record<string, unknown> = {};
  const self = node as unknown as Locator;
  node["count"] = overrides.count ?? (async () => 1);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["allInnerTexts"] = overrides.allInnerTexts ?? (async () => []);
  node["getAttribute"] = overrides.getAttribute ?? (async () => null);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["fill"] = overrides.fill ?? (async () => undefined);
  node["press"] = overrides.press ?? (async () => undefined);
  node["inputValue"] = overrides.inputValue ?? (async () => "");
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["isDisabled"] = overrides.isDisabled ?? (async () => false);
  node["boundingBox"] = overrides.boundingBox ?? (async () => null);
  node["evaluate"] = overrides.evaluate ?? (async () => null);
  node["all"] = overrides.all ?? (async () => []);
  node["first"] = () => overrides.first?.() ?? self;
  node["nth"] = (index: number) => overrides.nth?.(index) ?? self;
  node["last"] = () => overrides.last?.() ?? self;
  node["locator"] = (selector: string) => overrides.locator?.(selector) ?? self;
  node["getByRole"] = (role: string, options?: { name?: string | RegExp; exact?: boolean }) =>
    overrides.role?.(role, options) ?? self;
  node["getByTestId"] = (id: string) => overrides.testId?.(id) ?? self;
  node["getByText"] = (text: string) => overrides.text?.(text) ?? self;
  node["filter"] = (options: unknown) => overrides.filter?.(options) ?? self;
  return self;
}

/** A node that is not in the page at all. */
function absent(): Locator {
  return loc({ count: async () => 0, isVisible: async () => false });
}

interface RouteCall {
  matcher: unknown;
  handler: (route: Route) => Promise<void>;
}

interface PageCalls {
  gotos: string[];
  goBacks: number;
  reloads: number;
  timeouts: number[];
  presses: string[];
  permissions: string[][];
  cookiesCleared: number;
  routes: RouteCall[];
  requestListeners: Array<(request: { url(): string; method(): string }) => void>;
}

interface PageDoubleConfig {
  url?: string;
  title?: string;
  testIds?: (id: string) => Locator | undefined;
  roles?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator | undefined;
  texts?: (text: string) => Locator | undefined;
  selectors?: (selector: string) => Locator | undefined;
  evaluate?: (fn: (node: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  onWaitForURL?: (pattern: RegExp) => void;
  page?: Record<string, unknown>;
}

function pageDouble(config: PageDoubleConfig = {}): {
  page: Page;
  calls: PageCalls;
  state: { url: string };
} {
  const calls: PageCalls = {
    gotos: [],
    goBacks: 0,
    reloads: 0,
    timeouts: [],
    presses: [],
    permissions: [],
    cookiesCleared: 0,
    routes: [],
    requestListeners: [],
  };
  const state = { url: config.url ?? "http://new.test/acme/projects/project-1/pages" };
  const page = {
    url: () => state.url,
    title: async () => config.title ?? "",
    goto: async (target: string) => {
      calls.gotos.push(target);
      state.url = new URL(target, "http://new.test").href;
    },
    goBack: async () => {
      calls.goBacks += 1;
    },
    reload: async () => {
      calls.reloads += 1;
    },
    waitForURL: async (pattern: RegExp) => config.onWaitForURL?.(pattern),
    waitForTimeout: async (ms: number) => {
      calls.timeouts.push(ms);
    },
    keyboard: {
      press: async (key: string) => {
        calls.presses.push(key);
      },
    },
    getByTestId: (id: string) => config.testIds?.(id) ?? absent(),
    getByRole: (role: string, options?: { name?: string | RegExp; exact?: boolean }) =>
      config.roles?.(role, options) ?? absent(),
    getByText: (text: string) => config.texts?.(text) ?? absent(),
    locator: (selector: string) => config.selectors?.(selector) ?? absent(),
    route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
      calls.routes.push({ matcher, handler });
    },
    on: (event: string, handler: (request: { url(): string; method(): string }) => void) => {
      if (event === "request") calls.requestListeners.push(handler);
    },
    context: () => ({
      clearCookies: async () => {
        calls.cookiesCleared += 1;
      },
      grantPermissions: async (permissions: string[]) => {
        calls.permissions.push(permissions);
      },
    }),
    evaluate: (fn: (node: unknown) => unknown, arg?: unknown) =>
      config.evaluate !== undefined ? config.evaluate(fn, arg) : Promise.resolve(fn(arg)),
    ...config.page,
  };
  return { page: page as unknown as Page, calls, state };
}

interface RouteLog {
  fallbacks: number;
  fulfilled: Array<Record<string, unknown>>;
}

function testRoute(method: string, url: string, log: RouteLog): Route {
  return {
    request: () => ({ method: () => method, url: () => url }),
    fallback: async () => {
      log.fallbacks += 1;
    },
    fulfill: async (options: Record<string, unknown>) => {
      log.fulfilled.push(options);
    },
  } as unknown as Route;
}

/** Let a handler run up to the point where it suspends on its gate. */
function flush(): Promise<void> {
  return new Promise<void>((resolve) => setImmediate(resolve));
}

function logOf(): RouteLog {
  return { fallbacks: 0, fulfilled: [] };
}

function matches(call: RouteCall, raw: string): boolean {
  return (call.matcher as (url: URL) => boolean)(new URL(raw));
}

/**
 * Dispatch one request the way Playwright does: a URL the family never
 * matched falls back to the network untouched, and only a matched URL
 * reaches the armed handler.
 */
async function dispatch(call: RouteCall, method: string, url: string, log: RouteLog): Promise<void> {
  if (!matches(call, url)) {
    log.fallbacks += 1;
    return;
  }
  await call.handler(testRoute(method, url, log));
}

const LIST_URL = "http://new.test/api/workspaces/acme/projects/project-1/pages/";
const DETAIL_URL = `${LIST_URL}page-1/`;
const OTHER_DETAIL_URL = `${LIST_URL}page-2/`;
const VERSION_URL = `${DETAIL_URL}versions/version-1/`;

/** The list screen with one visible row link per title. */
function listPage(
  titles: Array<{ title: string | null; visible?: boolean }> = [],
  extra: PageDoubleConfig = {}
): ReturnType<typeof pageDouble> {
  const links = loc({
    all: async () =>
      titles.map((entry) =>
        loc({
          isVisible: async () => entry.visible ?? true,
          getAttribute: async (name: string) => (name === "aria-label" ? entry.title : null),
        })
      ),
  });
  const rows = loc({ locator: (selector: string) => (selector === "a[aria-label]" ? links : absent()) });
  return pageDouble({
    ...extra,
    testIds: (id) => {
      const given = extra.testIds?.(id);
      if (given !== undefined) return given;
      return id === "pages-list-screen" || id === "page-row" ? rows : absent();
    },
  });
}

/** One row that answers the title/star/menu probes of `pagesGatingRow`. */
function rowNode(config: {
  title: string;
  href?: string | null;
  star?: boolean;
  trigger?: Locator;
  linkClick?: () => void;
  visible?: boolean;
}): { node: Locator; link: Locator } {
  const link = loc({
    click: async () => config.linkClick?.(),
    getAttribute: async (name: string) => (name === "href" ? (config.href ?? null) : null),
  });
  const node = loc({
    isVisible: async () => config.visible ?? true,
    role: (role, options) => {
      if (role === "link") return link;
      if (role === "button" && options?.name === `Page actions for ${config.title}`) {
        return config.trigger ?? absent();
      }
      return absent();
    },
    testId: (id) => (id === "page-row-star" && config.star === true ? loc() : absent()),
  });
  return { node, link };
}

function rowsWith(row: { node: Locator }): Locator {
  return loc({ filter: () => row.node, first: () => row.node });
}

/** A detail header double with an optional sync badge and control cluster. */
function headerNode(config: {
  crumb?: string | null;
  crumbAbsent?: boolean;
  backHref?: string | null;
  star?: boolean;
  copyLink?: boolean;
  menu?: boolean;
  sync?: "Syncing..." | "Connection lost" | null;
  optionsTrigger?: Locator;
  backLink?: Locator;
  crumbNode?: Locator;
}): Locator {
  return loc({
    text: (text: string) => (config.sync === text ? loc() : absent()),
    testId: (id: string) => (id === "page-detail-star" && config.star === true ? loc() : absent()),
    role: (role, options) => {
      if (role === "button" && options?.name === undefined) {
        return config.crumbAbsent === true
          ? absent()
          : (config.crumbNode ?? loc({ innerText: async () => config.crumb ?? "" }));
      }
      if (role === "button" && options.name === "Copy link") return config.copyLink === true ? loc() : absent();
      if (role === "button" && options.name === "Copied link") return loc();
      if (role === "button" && options.name === "Page options") {
        return config.menu === true ? (config.optionsTrigger ?? loc()) : absent();
      }
      if (role === "link" && options.name === "Pages") {
        return config.backLink ?? loc({ getAttribute: async () => config.backHref ?? null });
      }
      return absent();
    },
  });
}

/** A menu popup that answers one trigger click with the given entry labels. */
function menuPopup(entries: Array<string | null>): Locator {
  const items = loc({
    allInnerTexts: async () => entries.map((entry) => entry ?? ""),
    count: async () => entries.length,
  });
  return loc({
    waitFor: async () => undefined,
    role: (role) => (role === "menuitem" ? items : absent()),
  });
}

describe("list navigation and addresses", () => {
  it("opens the list with and without a query and settles the rows", async () => {
    const stub = listPage([{ title: "Alpha" }]);
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingOpenList("acme", "project-1");
    await driver.pagesGatingOpenList("acme", "project-1", "?type=private");
    await driver.pagesGatingOpenList("acme", "project-1", undefined, false);

    expect(stub.calls.gotos).toEqual([
      "/acme/projects/project-1/pages",
      "/acme/projects/project-1/pages?type=private",
      "/acme/projects/project-1/pages",
    ]);
    await expect(driver.pagesGatingRowTitles()).resolves.toEqual(["Alpha"]);
  });

  it("opens a detail with and without a query without waiting for a shell", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingOpenDetail("acme", "project-1", "page-1");
    await driver.pagesGatingOpenDetail("acme", "project-1", "page-1", "?paneTab=info&version=version-1");

    expect(stub.calls.gotos).toEqual([
      "/acme/projects/project-1/pages/page-1",
      "/acme/projects/project-1/pages/page-1?paneTab=info&version=version-1",
    ]);
  });

  it("reads the address, its query keys and the document title", async () => {
    const stub = pageDouble({
      url: "http://new.test/acme/projects/project-1/pages/page-1?paneTab=info&version=version-1",
      title: "Alpha · Pages",
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingCurrentUrl()).resolves.toContain("/pages/page-1");
    await expect(driver.pagesGatingQueryParam("paneTab")).resolves.toBe("info");
    await expect(driver.pagesGatingQueryParam("paneTab")).resolves.toBe("info");
    await expect(driver.pagesGatingDocTitle()).resolves.toBe("Alpha · Pages");
  });

  it("reads a missing query key as null", async () => {
    const driver = new WebNewDriver(pageDouble({ url: "http://new.test/acme/projects/project-1/pages" }).page);
    await expect(driver.pagesGatingQueryParam("paneTab")).resolves.toBeNull();
  });

  it("reads row titles in order, skipping hidden and unlabeled links", async () => {
    const stub = listPage([
      { title: "Alpha" },
      { title: "  Beta  " },
      { title: null },
      { title: "Gamma", visible: false },
    ]);
    const driver = new WebNewDriver(stub.page);

    // Labels are trimmed; a blank label is dropped and a hidden row skipped.
    await expect(driver.pagesGatingRowTitles()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("reads a row href, or null when the row is gone", async () => {
    const row = rowNode({ title: "Alpha", href: "/acme/projects/project-1/pages/page-1" });
    const present = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-row" ? rowsWith(row) : absent()) }).page
    );
    await expect(present.pagesGatingRowHref("Alpha")).resolves.toBe("/acme/projects/project-1/pages/page-1");

    const missing = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-row" ? loc({ filter: () => absent(), first: () => absent() }) : absent()),
      }).page
    );
    await expect(missing.pagesGatingRowHref("Alpha")).resolves.toBeNull();
  });

  it("derives the active tab from the type key, defaulting to public", async () => {
    const cases: Array<[string, string]> = [
      ["http://new.test/pages", "public"],
      ["http://new.test/pages?type=public", "public"],
      ["http://new.test/pages?type=private", "private"],
      ["http://new.test/pages?type=archived", "archived"],
      ["http://new.test/pages?type=other", "public"],
    ];
    for (const [url, tab] of cases) {
      const driver = new WebNewDriver(pageDouble({ url }).page);
      await expect(driver.pagesGatingActiveTab()).resolves.toBe(tab);
    }
  });

  it("clicks a tab and waits for the address to name it", async () => {
    const stub = listPage();
    const click = vi.fn(async () => {
      stub.state.url = "http://new.test/pages?type=archived";
    });
    const link = loc({ click });
    const tabs = loc({ role: (role, options) => (role === "link" && options?.name === "Archived" ? link : absent()) });
    stub.page = {
      ...(stub.page as unknown as Record<string, unknown>),
      getByTestId: (id: string) => (id === "pages-list-tabs" ? tabs : absent()),
    } as unknown as Page;
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingClickTab("archived");

    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesGatingActiveTab()).resolves.toBe("archived");
  });

  it("names each tab label it clicks through", async () => {
    const seen: string[] = [];
    const link = (label: string) =>
      loc({
        click: async () => {
          seen.push(label);
          stub.state.url = `http://new.test/pages?type=${label === "Public" ? "public" : label.toLowerCase()}`;
        },
      });
    const tabs = loc({ role: (_role, options) => link(String(options?.name)) });
    const stub = listPage([], { testIds: (id) => (id === "pages-list-tabs" ? tabs : undefined) });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingClickTab("public");
    await driver.pagesGatingClickTab("private");
    await driver.pagesGatingClickTab("archived");

    expect(seen).toEqual(["Public", "Private", "Archived"]);
  });

  it("goes back to the list and settles it", async () => {
    const stub = listPage([{ title: "Alpha" }]);
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingGoBack();

    expect(stub.calls.goBacks).toBe(1);
    await expect(driver.pagesGatingRowTitles()).resolves.toEqual(["Alpha"]);
  });

  it("waits out a loading variant before it settles the list", async () => {
    const driver = new WebNewDriver(pageDouble().page);
    const variants = ["loading", "empty", "empty"];
    const read = vi.fn(async () => variants.shift() ?? "empty");
    (driver as unknown as { pagesGatingListVariant: () => Promise<string> }).pagesGatingListVariant = read;

    await driver.pagesGatingOpenList("acme", "project-1");

    // The first read is still mid-load, so the settle has to read again.
    expect(read.mock.calls.length).toBeGreaterThan(1);
  });
});

describe("gated list controls", () => {
  it("reports the create action by presence", async () => {
    const offered = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "pages-quick-create" ? loc() : undefined) }).page
    );
    await expect(offered.pagesGatingCreateOffered()).resolves.toBe(true);
    await expect(new WebNewDriver(pageDouble().page).pagesGatingCreateOffered()).resolves.toBe(false);
  });

  it("reports the row star only for a rendered row that offers it", async () => {
    const starred = rowNode({ title: "Alpha", star: true });
    const plain = rowNode({ title: "Beta" });
    const starredDriver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-row" ? rowsWith(starred) : absent()) }).page
    );
    const plainDriver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-row" ? rowsWith(plain) : absent()) }).page
    );
    const missingDriver = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-row" ? loc({ filter: () => absent(), first: () => absent() }) : absent()),
      }).page
    );

    await expect(starredDriver.pagesGatingRowStarOffered("Alpha")).resolves.toBe(true);
    await expect(plainDriver.pagesGatingRowStarOffered("Beta")).resolves.toBe(false);
    await expect(missingDriver.pagesGatingRowStarOffered("Gamma")).resolves.toBe(false);
  });

  it("reads one row's overflow entries, trimming blanks", async () => {
    const click = vi.fn(async () => undefined);
    const trigger = loc({ click });
    const row = rowNode({ title: "Alpha", trigger });
    const popup = menuPopup(["Open in new tab", "  Copy   link ", null, ""]);
    const stub = pageDouble({
      testIds: (id) => (id === "page-row" ? rowsWith(row) : absent()),
      roles: (role) => (role === "menu" ? popup : absent()),
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingRowMenuItems("Alpha")).resolves.toEqual(["Open in new tab", "Copy link"]);
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.calls.presses).toEqual(["Escape", "Escape"]);
  });

  it("reads the detail control cluster for each sync state", async () => {
    const full = new WebNewDriver(
      pageDouble({
        testIds: (id) =>
          id === "page-detail-header" ? headerNode({ star: true, copyLink: true, menu: true, sync: null }) : absent(),
      }).page
    );
    await expect(full.pagesGatingDetailCluster()).resolves.toEqual({
      star: true,
      copyLink: true,
      menu: true,
      syncBadge: null,
    });

    const syncing = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-detail-header" ? headerNode({ sync: "Syncing..." }) : absent()),
      }).page
    );
    await expect(syncing.pagesGatingDetailCluster()).resolves.toEqual({
      star: false,
      copyLink: false,
      menu: false,
      syncBadge: "Syncing...",
    });

    const lost = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-detail-header" ? headerNode({ sync: "Connection lost" }) : absent()),
      }).page
    );
    await expect(lost.pagesGatingDetailCluster()).resolves.toEqual({
      star: false,
      copyLink: false,
      menu: false,
      syncBadge: "Connection lost",
    });
  });

  it("reads the detail options menu entries", async () => {
    const optionsClick = vi.fn(async () => undefined);
    const optionsTrigger = loc({ click: optionsClick });
    const popup = menuPopup(["Copy link", "Make a copy", "Lock", "Version history", null]);
    const stub = pageDouble({
      testIds: (id) => (id === "page-detail-header" ? headerNode({ menu: true, optionsTrigger }) : absent()),
      roles: (role) => (role === "menu" ? popup : absent()),
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingDetailMenuItems()).resolves.toEqual([
      "Copy link",
      "Make a copy",
      "Lock",
      "Version history",
    ]);
    expect(optionsClick).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("clicks a named detail options item after dismissing a stale popup", async () => {
    const itemClick = vi.fn(async () => undefined);
    const optionsClick = vi.fn(async () => undefined);
    const item = loc({ click: itemClick });
    const optionsTrigger = loc({ click: optionsClick });
    const stub = pageDouble({
      testIds: (id) => (id === "page-detail-header" ? headerNode({ menu: true, optionsTrigger }) : absent()),
      roles: (role, options) => (role === "menuitem" && options?.name === "Lock" ? item : absent()),
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingClickOptionsItem("Lock");

    expect(optionsClick).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(itemClick).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.calls.presses).toEqual(["Escape"]);
  });

  it("counts editable regions", async () => {
    const editable = loc({ count: async () => 3 });
    const stub = pageDouble({
      selectors: (selector) => (selector === '[contenteditable="true"]' ? editable : absent()),
    });
    await expect(new WebNewDriver(stub.page).pagesGatingEditableCount()).resolves.toBe(3);
    await expect(new WebNewDriver(pageDouble().page).pagesGatingEditableCount()).resolves.toBe(0);
  });
});

describe("detail error panel", () => {
  it("reports panel visibility only for a rendered, visible panel", async () => {
    const visible = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-panel" ? loc() : absent()) }).page
    );
    const hidden = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-panel" ? loc({ isVisible: async () => false }) : absent()) })
        .page
    );
    const missing = new WebNewDriver(pageDouble().page);

    await expect(visible.pagesGatingErrorPanelVisible()).resolves.toBe(true);
    await expect(hidden.pagesGatingErrorPanelVisible()).resolves.toBe(false);
    await expect(missing.pagesGatingErrorPanelVisible()).resolves.toBe(false);
  });

  it("reads the panel heading, body and recovery link", async () => {
    const panel = loc({
      role: (role, options) => {
        if (role === "heading" && (options as { level?: number }).level === 2) {
          return loc({ innerText: async () => "  Page not found  " });
        }
        if (role === "link" && options?.name === "View other Pages") {
          return loc({ getAttribute: async () => "/acme/projects/project-1/pages" });
        }
        return absent();
      },
      locator: (selector) =>
        selector === "p" ? loc({ innerText: async () => "This page was deleted.\nAsk an admin." }) : absent(),
    });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-panel" ? panel : absent()) }).page
    );

    await expect(driver.pagesGatingErrorPanel()).resolves.toEqual({
      heading: "Page not found",
      body: "This page was deleted. Ask an admin.",
      backHref: "/acme/projects/project-1/pages",
    });
  });

  it("reads nulls when the panel copy is absent or detached", async () => {
    const empty = loc({
      role: (role) => {
        if (role === "heading") {
          return loc({
            innerText: async () => {
              throw new Error("detached");
            },
          });
        }
        return loc({ getAttribute: async () => null });
      },
      locator: () => loc({ innerText: async () => null as unknown as string }),
    });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-panel" ? empty : absent()) }).page
    );

    await expect(driver.pagesGatingErrorPanel()).resolves.toEqual({
      heading: null,
      body: null,
      backHref: null,
    });
  });
});

describe("list variants and placeholders", () => {
  it("classifies an unmounted screen as unknown", async () => {
    await expect(new WebNewDriver(pageDouble().page).pagesGatingListVariant()).resolves.toBe("unknown");
  });

  it("classifies the gate and no-results variants before the empty state", async () => {
    const gate = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "pages-list-screen" ? loc() : id === "pages-feature-gate" ? loc() : absent()),
      }).page
    );
    await expect(gate.pagesGatingListVariant()).resolves.toBe("gate");

    const noResults = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "pages-list-screen" ? loc() : id === "pages-no-results" ? loc() : absent()),
      }).page
    );
    await expect(noResults.pagesGatingListVariant()).resolves.toBe("no-results");
  });

  it("separates the archived empty state from the docs empty state", async () => {
    const archived = new WebNewDriver(
      pageDouble({
        testIds: (id) =>
          id === "pages-list-screen"
            ? loc()
            : id === "pages-empty-state"
              ? loc({ innerText: async () => "No archived pages yet" })
              : absent(),
      }).page
    );
    await expect(archived.pagesGatingListVariant()).resolves.toBe("archived-empty");

    const docs = new WebNewDriver(
      pageDouble({
        testIds: (id) =>
          id === "pages-list-screen"
            ? loc()
            : id === "pages-empty-state"
              ? loc({ innerText: async () => "No pages in this project yet" })
              : absent(),
      }).page
    );
    await expect(docs.pagesGatingListVariant()).resolves.toBe("docs-empty");
  });

  it("reads a hidden empty state as no variant and reports rows before loading", async () => {
    const hiddenEmpty = new WebNewDriver(
      pageDouble({
        testIds: (id) =>
          id === "pages-list-screen"
            ? loc()
            : id === "pages-empty-state"
              ? loc({ isVisible: async () => false })
              : absent(),
      }).page
    );
    await expect(hiddenEmpty.pagesGatingListVariant()).resolves.toBe("unknown");

    const rows = new WebNewDriver(listPage([{ title: "Alpha" }]).page);
    await expect(rows.pagesGatingListVariant()).resolves.toBe("rows");
  });

  it("reports loading from skeleton blocks and unknown when empty with no skeleton", async () => {
    const skeletonBlocks = loc({ count: async () => 5 });
    const skeleton = loc({
      locator: (selector: string) => (selector === "span.animate-shimmer" ? skeletonBlocks : absent()),
    });
    const loading = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "pages-list-screen" || id === "pages-list-skeleton" ? skeleton : absent()),
      }).page
    );
    await expect(loading.pagesGatingListVariant()).resolves.toBe("loading");
    await expect(loading.pagesGatingSkeletonCount()).resolves.toBe(5);

    const noSkeleton = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "pages-list-screen" ? loc() : absent()) }).page
    );
    await expect(noSkeleton.pagesGatingSkeletonCount()).resolves.toBe(0);
    await expect(noSkeleton.pagesGatingListVariant()).resolves.toBe("unknown");
  });

  it("reads the empty-create action as absent, enabled or disabled", async () => {
    const state = async (button: Locator | undefined, emptyPresent = true) =>
      new WebNewDriver(
        pageDouble({
          testIds: (id) =>
            id === "pages-empty-state" ? (emptyPresent ? loc({ role: () => button ?? absent() }) : absent()) : absent(),
        }).page
      ).pagesGatingEmptyCreateState();

    await expect(state(undefined)).resolves.toBe("absent");
    await expect(state(undefined, false)).resolves.toBe("absent");
    await expect(state(loc({ isDisabled: async () => true }))).resolves.toBe("disabled");
    await expect(state(loc({ isDisabled: async () => false }))).resolves.toBe("enabled");
  });
});

describe("held and failed route families", () => {
  it("holds a collection read until released, then lets it through", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStallListFetch();
    expect(stub.calls.routes).toHaveLength(1);
    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the list family to arm");

    expect(matches(call, `${LIST_URL}?type=private`)).toBe(true);
    expect(matches(call, DETAIL_URL)).toBe(false);
    expect(matches(call, "http://new.test/acme/projects/project-1/pages")).toBe(false);

    const held = logOf();
    const pending = dispatch(call, "GET", `${LIST_URL}?type=private`, held);
    await flush();
    expect(held.fallbacks).toBe(0);

    await driver.pagesGatingReleaseListFetch();
    await pending;
    expect(held.fallbacks).toBe(1);
  });

  it("keeps a create out of a stalled list read and a list read out of a failed create", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStallListFetch();
    const list = stub.calls.routes[0];
    if (list === undefined) throw new Error("expected the list family to arm");
    const viaPost = logOf();
    await dispatch(list, "POST", LIST_URL, viaPost);
    expect(viaPost.fallbacks).toBe(1);

    await driver.pagesGatingFailListCreates();
    const create = stub.calls.routes[1];
    if (create === undefined) throw new Error("expected the create family to arm");
    const viaGet = logOf();
    await dispatch(create, "GET", LIST_URL, viaGet);
    expect(viaGet.fallbacks).toBe(1);
    expect(viaGet.fulfilled).toEqual([]);

    const viaPostFail = logOf();
    await dispatch(create, "POST", LIST_URL, viaPostFail);
    expect(viaPostFail.fulfilled).toHaveLength(1);
    expect(viaPostFail.fulfilled[0]?.["status"]).toBe(500);
    expect(viaPostFail.fulfilled[0]?.["contentType"]).toBe("application/json");
    expect(JSON.parse(String(viaPostFail.fulfilled[0]?.["body"]))).toEqual({
      detail: "parity-forced-pages-failure",
    });
    await driver.pagesGatingUnfailListFetch();
    await driver.pagesGatingUnfailListCreates();
  });

  it("fails a collection read with a 500 and passes other methods", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingFailListFetch();
    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the list family to arm");
    const log = logOf();
    await dispatch(call, "GET", LIST_URL, log);
    await dispatch(call, "DELETE", LIST_URL, log);

    expect(log.fulfilled).toHaveLength(1);
    expect(log.fallbacks).toBe(1);
    // Another area's collection is not this family's to touch.
    expect(matches(call, "http://new.test/api/workspaces/acme/projects/project-1/issues/")).toBe(false);
    await driver.pagesGatingUnfailListFetch();
  });

  it("flips an armed family between stalling and failing instead of re-arming", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStallListFetch();
    await driver.pagesGatingFailListFetch();
    expect(stub.calls.routes).toHaveLength(1);

    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the list family to arm");
    const log = logOf();
    await dispatch(call, "GET", LIST_URL, log);
    expect(log.fulfilled).toHaveLength(1);
    await driver.pagesGatingUnfailListFetch();
  });

  it("arms each family once and disarms an unknown family quietly", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStallVersionDetail();
    await driver.pagesGatingReleaseVersionDetail();
    await driver.pagesGatingReleaseVersionDetail();
    await driver.pagesGatingReleaseDetailFetch();
    expect(stub.calls.routes).toHaveLength(1);

    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the version family to arm");
    expect(matches(call, VERSION_URL)).toBe(true);
    expect(matches(call, LIST_URL)).toBe(false);
    const log = logOf();
    await dispatch(call, "GET", VERSION_URL, log);
    expect(log.fallbacks).toBe(1);
  });

  it("holds only the stalled page's record read, not its neighbours", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStallDetailFetch("page-1");
    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the detail family to arm");
    expect(matches(call, DETAIL_URL)).toBe(true);
    expect(matches(call, OTHER_DETAIL_URL)).toBe(false);

    const neighbour = logOf();
    await dispatch(call, "GET", OTHER_DETAIL_URL, neighbour);
    expect(neighbour.fallbacks).toBe(1);

    const held = logOf();
    const pending = dispatch(call, "GET", DETAIL_URL, held);
    await flush();
    expect(held.fallbacks).toBe(0);

    await driver.pagesGatingReleaseDetailFetch();
    await pending;
    expect(held.fallbacks).toBe(1);
    expect(matches(call, DETAIL_URL)).toBe(false);

    // Arming again retargets the single installed family at the next page.
    await driver.pagesGatingStallDetailFetch("page-2");
    expect(stub.calls.routes).toHaveLength(1);
    expect(matches(call, OTHER_DETAIL_URL)).toBe(true);
    const second = logOf();
    const secondPending = dispatch(call, "GET", OTHER_DETAIL_URL, second);
    await flush();
    expect(second.fallbacks).toBe(0);
    await driver.pagesGatingReleaseDetailFetch();
    await secondPending;
    expect(second.fallbacks).toBe(1);
  });
});

describe("request spy and toasts", () => {
  function spyPage(): ReturnType<typeof pageDouble> {
    return pageDouble();
  }

  it("logs page, favorite and shelved requests and ignores everything else", async () => {
    const stub = spyPage();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStartRequestSpy();
    expect(stub.calls.requestListeners).toHaveLength(1);
    const listen = stub.calls.requestListeners[0];
    if (listen === undefined) throw new Error("expected the spy to listen");

    listen({ url: () => "http://new.test/other/pages/", method: () => "GET" });
    listen({ url: () => "http://new.test/api/workspaces/acme/projects/project-1/boards/", method: () => "GET" });
    listen({ url: () => `${LIST_URL}?type=private`, method: () => "GET" });
    listen({
      url: () => "http://new.test/api/workspaces/acme/projects/project-1/favorite-pages/",
      method: () => "GET",
    });
    listen({
      url: () => "http://new.test/api/workspaces/acme/projects/project-1/archived-pages/",
      method: () => "POST",
    });

    await expect(driver.pagesGatingLoggedRequests()).resolves.toEqual([
      { method: "GET", path: LIST_URL, query: "type=private" },
      {
        method: "GET",
        path: "http://new.test/api/workspaces/acme/projects/project-1/favorite-pages/",
        query: "",
      },
      {
        method: "POST",
        path: "http://new.test/api/workspaces/acme/projects/project-1/archived-pages/",
        query: "",
      },
    ]);
  });

  it("resets the log, returns copies, and reuses one listener across starts", async () => {
    const stub = spyPage();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingStartRequestSpy();
    const listen = stub.calls.requestListeners[0];
    if (listen === undefined) throw new Error("expected the spy to listen");
    listen({ url: () => LIST_URL, method: () => "GET" });

    const first = await driver.pagesGatingLoggedRequests();
    first.push({ method: "GET", path: "injected", query: "" });
    await expect(driver.pagesGatingLoggedRequests()).resolves.toHaveLength(1);

    await driver.pagesGatingResetRequestSpy();
    await expect(driver.pagesGatingLoggedRequests()).resolves.toEqual([]);

    listen({ url: () => LIST_URL, method: () => "GET" });
    await driver.pagesGatingStartRequestSpy();
    expect(stub.calls.requestListeners).toHaveLength(1);
    await expect(driver.pagesGatingLoggedRequests()).resolves.toEqual([]);
  });

  it("reads toast texts, skipping an absent stack and blank toasts", async () => {
    const toasts = loc({ allInnerTexts: async () => ["  Could not create   page ", "", "Copied link"] });
    const stack = loc({ testId: (id) => (id === "toast-notification" ? toasts : absent()) });
    const driver = new WebNewDriver(pageDouble({ testIds: (id) => (id === "toast-stack" ? stack : absent()) }).page);
    await expect(driver.pagesGatingToastTexts()).resolves.toEqual(["Could not create page", "Copied link"]);

    await expect(new WebNewDriver(pageDouble().page).pagesGatingToastTexts()).resolves.toEqual([]);
  });
});

describe("list search, sort and filters", () => {
  it("expands a collapsed search before typing, then reads the value back", async () => {
    let expanded = false;
    const fill = vi.fn(async () => undefined);
    const field = loc({
      count: async () => (expanded ? 1 : 0),
      fill,
      inputValue: async () => "alpha",
    });
    const expand = vi.fn(async () => {
      expanded = true;
    });
    const search = loc({
      role: (role) => (role === "textbox" ? field : role === "button" ? loc({ click: expand }) : absent()),
    });
    const stub = listPage([], { testIds: (id) => (id === "pages-list-search" ? search : undefined) });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingSearchValue()).resolves.toBeNull();
    await driver.pagesGatingSearchType("alpha");

    expect(expand).toHaveBeenCalledTimes(1);
    expect(fill).toHaveBeenCalledWith("alpha", { timeout: 30_000 });
    await expect(driver.pagesGatingSearchValue()).resolves.toBe("alpha");
  });

  it("types into an already expanded search without pressing its trigger", async () => {
    const fill = vi.fn(async () => undefined);
    const field = loc({ fill, inputValue: async () => "beta" });
    const press = vi.fn(async () => undefined);
    const search = loc({ role: (role) => (role === "textbox" ? field : loc({ click: press })) });
    const stub = listPage([], { testIds: (id) => (id === "pages-list-search" ? search : undefined) });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingSearchType("beta");

    expect(press).not.toHaveBeenCalled();
    expect(fill).toHaveBeenCalledWith("beta", { timeout: 30_000 });
  });

  it("reads a detached search value as null", async () => {
    const field = loc({
      inputValue: async () => {
        throw new Error("detached");
      },
    });
    const search = loc({ role: () => field });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "pages-list-search" ? search : absent()) }).page
    );
    await expect(driver.pagesGatingSearchValue()).resolves.toBeNull();
  });

  it("reads the current sort key and throws when no known key shows", async () => {
    const named = (match: string) => loc({ isVisible: async () => match === "Date modified" });
    const stub = pageDouble({
      roles: (role, options) => (role === "button" ? named(String(options?.name)) : absent()),
    });
    await expect(new WebNewDriver(stub.page).pagesGatingSortLabel()).resolves.toBe("Date modified");

    const none = pageDouble({
      roles: (role, options) =>
        role === "button" ? loc({ isVisible: async () => options?.name === "Filters" }) : absent(),
    });
    await expect(new WebNewDriver(none.page).pagesGatingSortLabel()).rejects.toThrow("sort control shows no known key");
  });

  it("switches the sort to the next key through the popup", async () => {
    let current = "Name";
    const keyClick = vi.fn(async () => undefined);
    const itemClick = vi.fn(async () => {
      current = "Date created";
    });
    const keyButton = loc({ isVisible: async () => true, click: keyClick });
    const nextItem = loc({ click: itemClick });
    const popup = loc({ role: (role) => (role === "menuitem" ? nextItem : absent()) });
    const stub = pageDouble({
      roles: (role, options) => {
        if (role === "menu") return popup;
        if (role !== "button") return absent();
        return options?.name === current ? keyButton : absent();
      },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingSortCycle();

    expect(keyClick).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(itemClick).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesGatingSortLabel()).resolves.toBe("Date created");
  });

  it("opens the filter panel only when shut, then toggles favorites", async () => {
    let open = false;
    const favorites = vi.fn(async () => undefined);
    const trigger = vi.fn(async () => {
      open = true;
    });
    const panel = loc({
      count: async () => (open ? 1 : 0),
      role: (role, options) =>
        role === "button" && options?.name === "Favorites" ? loc({ click: favorites }) : absent(),
    });
    const stub = listPage([], {
      testIds: (id) => (id === "pages-filter-panel" ? panel : undefined),
      roles: (role, options) => (role === "button" && options?.name === "Filters" ? loc({ click: trigger }) : absent()),
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingFilterToggleFavorites();
    await driver.pagesGatingFilterToggleFavorites();

    expect(trigger).toHaveBeenCalledTimes(1);
    expect(favorites).toHaveBeenCalledTimes(2);
  });

  it("reads filter chips, trimming blanks and tolerating an absent strip", async () => {
    const chips = loc({ allInnerTexts: async () => [" Favorites ", "", "Owner   is me"] });
    const strip = loc({
      locator: (selector) => (selector === '[data-testid^="pages-filter-chip-"]' ? chips : absent()),
    });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "pages-filter-chips" ? strip : absent()) }).page
    );
    await expect(driver.pagesGatingChips()).resolves.toEqual(["Favorites", "Owner is me"]);

    await expect(new WebNewDriver(pageDouble().page).pagesGatingChips()).resolves.toEqual([]);
  });
});

describe("detail shell and switcher", () => {
  function detailPage(config: {
    header?: Locator;
    switcher?: Locator;
    pane?: Locator;
    overlay?: Locator;
    title?: Locator;
    body?: Locator;
    panel?: Locator;
    spinner?: Locator;
    liveLoader?: Locator;
    skeleton?: Locator;
    optionsEntries?: Array<string | null>;
    optionsTrigger?: Locator;
    listbox?: Locator;
    searchbox?: Locator;
    option?: Locator;
    copiedTrigger?: Locator;
    quickCreate?: Locator;
    extra?: PageDoubleConfig;
  }): ReturnType<typeof pageDouble> {
    return pageDouble({
      ...config.extra,
      testIds: (id) => {
        const given = config.extra?.testIds?.(id);
        if (given !== undefined) return given;
        switch (id) {
          case "page-detail-header":
            return config.header ?? absent();
          case "page-navigation-pane":
            return config.pane ?? absent();
          case "page-version-overlay":
            return config.overlay ?? absent();
          case "page-detail-title":
            return config.title ?? absent();
          case "page-canvas-stage":
            return config.body ?? absent();
          case "page-detail-panel":
            return config.panel ?? absent();
          case "page-detail-spinner":
            return config.spinner ?? absent();
          case "page-live-loader":
            return config.liveLoader ?? absent();
          case "page-body-skeleton":
            return config.skeleton ?? absent();
          case "pages-quick-create":
            return config.quickCreate ?? absent();
          default:
            return absent();
        }
      },
      roles: (role, options) => {
        const given = config.extra?.roles?.(role, options);
        if (given !== undefined) return given;
        if (role === "menu") {
          return config.optionsEntries !== undefined ? menuPopup(config.optionsEntries) : absent();
        }
        if (role === "listbox") return config.listbox ?? absent();
        if (role === "searchbox") return config.searchbox ?? absent();
        if (role === "option") return config.option ?? absent();
        return absent();
      },
    });
  }

  it("reads the project crumb and its empty and missing forms", async () => {
    const crumb = async (value: string | null) => {
      const stub = detailPage({ header: headerNode({ crumb: value }) });
      return new WebNewDriver(stub.page).pagesGatingBreadcrumbProject();
    };
    await expect(crumb("Acme Project")).resolves.toBe("Acme Project");
    await expect(crumb("   ")).resolves.toBeNull();

    const detached = detailPage({
      header: headerNode({
        crumbNode: loc({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
      }),
    });
    await expect(new WebNewDriver(detached.page).pagesGatingBreadcrumbProject()).resolves.toBeNull();

    const missing = detailPage({ header: headerNode({ crumbAbsent: true }) });
    await expect(new WebNewDriver(missing.page).pagesGatingBreadcrumbProject()).resolves.toBeNull();
  });

  it("reads the back-to-list href", async () => {
    const href = async (value: string | null) => {
      const stub = detailPage({ header: headerNode({ backHref: value }) });
      return new WebNewDriver(stub.page).pagesGatingBackLinkHref();
    };
    await expect(href("/acme/projects/project-1/pages")).resolves.toBe("/acme/projects/project-1/pages");
    await expect(href(null)).resolves.toBeNull();

    const detached = detailPage({
      header: headerNode({
        backLink: loc({
          getAttribute: async () => {
            throw new Error("detached");
          },
        }),
      }),
    });
    await expect(new WebNewDriver(detached.page).pagesGatingBackLinkHref()).resolves.toBeNull();
  });

  it("opens the switcher past a stale popup and reads its option names", async () => {
    const triggerClick = vi.fn(async () => undefined);
    const header = loc({
      role: (role, options) =>
        role === "button" && typeof options?.name !== "string" ? loc({ click: triggerClick }) : absent(),
    });
    const names = loc({ allInnerTexts: async () => [" Alpha ", "", "Beta   Two"] });
    const listbox = loc({ testId: (id) => (id === "page-switcher-option-name" ? names : absent()) });
    const stub = detailPage({ header, listbox });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingSwitcherOptions()).resolves.toEqual(["Alpha", "Beta Two"]);

    expect(stub.calls.presses).toEqual(["Escape", "Escape"]);
    expect(stub.calls.timeouts).toEqual([300]);
    expect(triggerClick).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("narrows switcher options to the query and waits for foreign names to drop", async () => {
    let reads = 0;
    const names = loc({
      allInnerTexts: async () => {
        reads += 1;
        return reads === 1 ? ["Alpha", "Beta"] : ["Alpha"];
      },
    });
    const listbox = loc({ testId: (id) => (id === "page-switcher-option-name" ? names : absent()) });
    const fill = vi.fn(async () => undefined);
    const searchbox = loc({ fill });
    const stub = detailPage({
      header: loc({
        role: (role, options) => (role === "button" && typeof options?.name !== "string" ? loc() : absent()),
      }),
      listbox,
      searchbox,
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingSwitcherSearch("Al")).resolves.toEqual(["Alpha"]);
    expect(fill).toHaveBeenCalledWith("Al", { timeout: 30_000 });
  });

  it("selects a switcher sibling and waits for the detail to land", async () => {
    const optionClick = vi.fn(async () => {
      stub.state.url = "http://new.test/acme/projects/project-1/pages/page-2";
    });
    const option = loc({ click: optionClick });
    const body = loc({
      locator: () => loc({ waitFor: async () => undefined, innerText: async () => "Body" }),
    });
    const stub = detailPage({
      header: loc({
        role: (role, options) => (role === "button" && typeof options?.name !== "string" ? loc() : absent()),
      }),
      listbox: loc(),
      option,
      body,
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingSwitcherSelect("Beta")).resolves.toBeUndefined();
    expect(optionClick).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesGatingWaitDetailSettled()).resolves.toBe("editor");
  });

  it("reads title facts for a filled and an empty title", async () => {
    const filled = detailPage({
      title: loc({
        innerText: async () => "Alpha",
        boundingBox: async () => ({ x: 0, y: 0, width: 400, height: 32 }),
      }),
    });
    await expect(new WebNewDriver(filled.page).pagesGatingTitleFacts("page-1")).resolves.toEqual({
      text: "Alpha",
      placeholder: null,
      height: 32,
    });

    const empty = detailPage({
      title: loc({
        innerText: async () => "Untitled",
        boundingBox: async () => null,
        locator: (selector) => {
          if (selector === '[data-empty="true"]') return loc();
          if (selector === "[data-placeholder]") {
            return loc({ getAttribute: async () => "Untitled" });
          }
          return absent();
        },
      }),
    });
    await expect(new WebNewDriver(empty.page).pagesGatingTitleFacts("page-1")).resolves.toEqual({
      text: "Untitled",
      placeholder: "Untitled",
      height: 0,
    });
  });

  it("reads an empty title with a detached placeholder and body as nulls", async () => {
    const stub = detailPage({
      title: loc({
        innerText: async () => {
          throw new Error("detached");
        },
        locator: (selector) =>
          selector === '[data-empty="true"]'
            ? loc()
            : loc({
                getAttribute: async () => {
                  throw new Error("detached");
                },
              }),
      }),
    });
    await expect(new WebNewDriver(stub.page).pagesGatingTitleFacts("page-1")).resolves.toEqual({
      text: "",
      placeholder: null,
      height: 0,
    });
  });

  it("reads an empty title whose placeholder heading has not mounted yet", async () => {
    // The static empty marker is present while the title chunk loads and the
    // placeholder heading arrives with it, so emptiness alone owes no text.
    const stub = detailPage({
      title: loc({
        innerText: async () => "Untitled",
        locator: (selector) => (selector === '[data-empty="true"]' ? loc() : absent()),
      }),
    });
    await expect(new WebNewDriver(stub.page).pagesGatingTitleFacts("page-1")).resolves.toEqual({
      text: "Untitled",
      placeholder: null,
      height: 0,
    });
  });

  it("reports the body skeleton from either cold-load surface", async () => {
    const skeletonPage = detailPage({ skeleton: loc() });
    await expect(new WebNewDriver(skeletonPage.page).pagesGatingBodySkeletonVisible()).resolves.toBe(true);

    const livePage = detailPage({ liveLoader: loc() });
    await expect(new WebNewDriver(livePage.page).pagesGatingBodySkeletonVisible()).resolves.toBe(true);

    await expect(new WebNewDriver(pageDouble().page).pagesGatingBodySkeletonVisible()).resolves.toBe(false);
  });

  it("reads the selected pane tab only while the pane key is set", async () => {
    const tabs = loc({
      count: async () => 3,
      nth: (index) =>
        loc({
          getAttribute: async () => (index === 1 ? "true" : "false"),
          innerText: async () => ["Outline", " Info ", "Assets"][index] ?? "",
        }),
    });
    const pane = loc({ role: (role) => (role === "tab" ? tabs : absent()) });
    const withKey = new WebNewDriver(
      detailPage({
        pane,
        extra: { url: "http://new.test/pages/page-1?paneTab=info" },
      }).page
    );
    await expect(withKey.pagesGatingPaneActiveTab()).resolves.toBe("Info");

    const withoutKey = new WebNewDriver(detailPage({ pane }).page);
    await expect(withoutKey.pagesGatingPaneActiveTab()).resolves.toBeNull();

    const withoutPane = new WebNewDriver(
      detailPage({ extra: { url: "http://new.test/pages/page-1?paneTab=info" } }).page
    );
    await expect(withoutPane.pagesGatingPaneActiveTab()).resolves.toBeNull();

    const nothingSelected = new WebNewDriver(
      detailPage({
        pane: loc({ role: (role) => (role === "tab" ? loc({ getAttribute: async () => "false" }) : absent()) }),
        extra: { url: "http://new.test/pages/page-1?paneTab=info" },
      }).page
    );
    await expect(nothingSelected.pagesGatingPaneActiveTab()).resolves.toBeNull();

    const blankLabel = new WebNewDriver(
      detailPage({
        pane: loc({
          role: (role) =>
            role === "tab" ? loc({ getAttribute: async () => "true", innerText: async () => " " }) : absent(),
        }),
        extra: { url: "http://new.test/pages/page-1?paneTab=info" },
      }).page
    );
    await expect(blankLabel.pagesGatingPaneActiveTab()).resolves.toBeNull();

    const detached = new WebNewDriver(
      detailPage({
        pane: loc({
          role: (role) =>
            role === "tab"
              ? loc({
                  getAttribute: async () => "true",
                  innerText: async () => {
                    throw new Error("detached");
                  },
                })
              : absent(),
        }),
        extra: { url: "http://new.test/pages/page-1?paneTab=info" },
      }).page
    );
    await expect(detached.pagesGatingPaneActiveTab()).resolves.toBeNull();
  });

  it("clicks a version link and waits for the overlay", async () => {
    let open = false;
    const linkClick = vi.fn(async () => {
      open = true;
    });
    const link = loc({ click: linkClick });
    const pane = loc({ locator: (selector) => (selector.includes("data-version-id") ? link : absent()) });
    const overlay = loc({ isVisible: async () => open });
    const stub = detailPage({ pane, overlay });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingClickVersionLink("version-1");

    expect(linkClick).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesGatingOverlayOpen()).resolves.toBe(true);
  });

  it("reads the overlay title and text, returning empties when closed", async () => {
    const overlay = loc({
      innerText: async () => "Version from Oct 20\nAlpha body",
      locator: (selector) => (selector === "h6" ? loc({ innerText: async () => " Version from Oct 20 " }) : absent()),
    });
    const openDriver = new WebNewDriver(detailPage({ overlay }).page);
    await expect(openDriver.pagesGatingOverlayTitle()).resolves.toBe("Version from Oct 20");
    await expect(openDriver.pagesGatingOverlayText()).resolves.toBe("Version from Oct 20\nAlpha body");

    const closed = new WebNewDriver(pageDouble().page);
    await expect(closed.pagesGatingOverlayTitle()).resolves.toBe("");
    await expect(closed.pagesGatingOverlayText()).resolves.toBe("");
    await expect(closed.pagesGatingOverlaySkeletonVisible()).resolves.toBe(false);
    await expect(closed.pagesGatingOverlayOpen()).resolves.toBe(false);
  });

  it("reads a detached overlay as blank", async () => {
    const overlay = loc({
      innerText: async () => {
        throw new Error("detached");
      },
      locator: () => loc({ innerText: async () => null as unknown as string }),
    });
    const driver = new WebNewDriver(detailPage({ overlay }).page);
    await expect(driver.pagesGatingOverlayTitle()).resolves.toBe("");
    await expect(driver.pagesGatingOverlayText()).resolves.toBe("");

    const detachedHeading = loc({
      innerText: async () => "Alpha body",
      locator: (selector: string) =>
        selector === "h6"
          ? loc({
              innerText: async () => {
                throw new Error("detached");
              },
            })
          : absent(),
    });
    await expect(
      new WebNewDriver(detailPage({ overlay: detachedHeading }).page).pagesGatingOverlayTitle()
    ).resolves.toBe("");
  });

  it("reads overlay skeleton visibility inside an open overlay", async () => {
    const skeleton = loc();
    const overlay = loc({ testId: (id) => (id === "page-version-skeleton" ? skeleton : absent()) });
    const driver = new WebNewDriver(detailPage({ overlay }).page);
    await expect(driver.pagesGatingOverlaySkeletonVisible()).resolves.toBe(true);

    const idle = new WebNewDriver(
      detailPage({
        overlay: loc({
          testId: (id) => (id === "page-version-skeleton" ? loc({ isVisible: async () => false }) : absent()),
        }),
      }).page
    );
    await expect(idle.pagesGatingOverlaySkeletonVisible()).resolves.toBe(false);
  });
});

describe("detail wait, spinner and creation", () => {
  it("reports the editor once the canvas body paints", async () => {
    const body = loc({ locator: () => loc({ waitFor: async () => undefined, innerText: async () => "" }) });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-canvas-stage" ? body : absent()) }).page
    );
    await expect(driver.pagesGatingWaitDetailSettled()).resolves.toBe("editor");
  });

  it("reports the panel when the record read resolves to it", async () => {
    const body = loc({
      locator: () =>
        loc({
          waitFor: async () => {
            throw new Error("no body");
          },
        }),
    });
    const panel = loc();
    const driver = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-canvas-stage" ? body : id === "page-detail-panel" ? panel : absent()),
      }).page
    );
    await expect(driver.pagesGatingWaitDetailSettled()).resolves.toBe("panel");
  });

  it("reports the panel when a visible panel wins the race", async () => {
    const body = loc({
      locator: () =>
        loc({
          waitFor: async () => {
            throw new Error("no body");
          },
        }),
    });
    let panelReady = false;
    const panel = loc({ isVisible: async () => panelReady });
    const driver = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-canvas-stage" ? body : id === "page-detail-panel" ? panel : absent()),
      }).page
    );
    // The error-panel check runs after the race; a visible panel short-circuits.
    panelReady = true;
    await expect(driver.pagesGatingWaitDetailSettled()).resolves.toBe("panel");
  });

  it("reports a record spinner only outside the collaboration loader", async () => {
    const loaderSpinner = loc({
      isVisible: async () => true,
      locator: (selector: string) => (selector.startsWith("xpath=ancestor") ? loc() : absent()),
    });
    const inner = loc({
      count: async () => 1,
      nth: () => loaderSpinner,
    });
    const loaderOnly = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-spinner" ? inner : absent()) }).page
    );
    await expect(loaderOnly.pagesGatingDetailSpinnerVisible()).resolves.toBe(false);

    const hidden = loc({
      count: async () => 1,
      nth: () => loc({ isVisible: async () => false }),
    });
    await expect(
      new WebNewDriver(
        pageDouble({ testIds: (id) => (id === "page-detail-spinner" ? hidden : absent()) }).page
      ).pagesGatingDetailSpinnerVisible()
    ).resolves.toBe(false);

    const own = loc({
      count: async () => 2,
      nth: (index: number) =>
        index === 0
          ? loc({ isVisible: async () => true, locator: () => loc() })
          : loc({ isVisible: async () => true, locator: () => absent() }),
    });
    await expect(
      new WebNewDriver(
        pageDouble({ testIds: (id) => (id === "page-detail-spinner" ? own : absent()) }).page
      ).pagesGatingDetailSpinnerVisible()
    ).resolves.toBe(true);

    await expect(new WebNewDriver(pageDouble().page).pagesGatingDetailSpinnerVisible()).resolves.toBe(false);
  });

  it("creates a page and returns its id once the detail lands", async () => {
    const createClick = vi.fn(async () => undefined);
    const quickCreate = loc({ click: createClick });
    const body = loc({ locator: () => loc({ waitFor: async () => undefined, innerText: async () => "" }) });
    const stub = pageDouble({
      testIds: (id) => (id === "pages-quick-create" ? quickCreate : id === "page-canvas-stage" ? body : absent()),
      onWaitForURL: () => {
        stub.state.url = "http://new.test/acme/projects/project-1/pages/abc12345-0000";
      },
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingQuickCreate()).resolves.toBe("abc12345-0000");
    expect(createClick).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("rejects a create that never lands on a page id", async () => {
    const stub = pageDouble({
      url: "http://new.test/acme/projects/project-1/pages",
      testIds: (id) => (id === "pages-quick-create" ? loc({ click: async () => undefined }) : absent()),
      onWaitForURL: () => undefined,
    });
    await expect(new WebNewDriver(stub.page).pagesGatingQuickCreate()).rejects.toThrow("never landed on a page id");
  });

  it("clicks create without waiting for navigation", async () => {
    const click = vi.fn(async () => undefined);
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "pages-quick-create" ? loc({ click }) : absent()) }).page
    );
    await driver.pagesGatingClickCreate();
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("opens a row through its link and settles the detail", async () => {
    const body = loc({ locator: () => loc({ waitFor: async () => undefined, innerText: async () => "" }) });
    const stub = pageDouble({ testIds: (id) => (id === "page-canvas-stage" ? body : absent()) });
    const row = rowNode({
      title: "Alpha",
      linkClick: () => {
        stub.state.url = "http://new.test/acme/projects/project-1/pages/page-1";
      },
    });
    stub.page = {
      ...(stub.page as unknown as Record<string, unknown>),
      getByTestId: (id: string) => (id === "page-row" ? rowsWith(row) : id === "page-canvas-stage" ? body : absent()),
    } as unknown as Page;
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingOpenRow("Alpha");

    await expect(driver.pagesGatingCurrentUrl()).resolves.toContain("/pages/page-1");
  });

  it("copies the page link through the header control", async () => {
    const copyClick = vi.fn(async () => undefined);
    const header = loc({
      role: (role, options) => {
        if (role !== "button") return absent();
        if (options?.name === "Copy link") return loc({ click: copyClick });
        if (options?.name === "Copied link") return loc({ waitFor: async () => undefined });
        return absent();
      },
    });
    vi.stubGlobal("navigator", {
      clipboard: { readText: async () => "https://new.test/acme/projects/project-1/pages/page-1" },
    });
    const stub = pageDouble({
      testIds: (id) => (id === "page-detail-header" ? header : absent()),
      evaluate: (fn) => Promise.resolve(fn(undefined)),
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesGatingCopyLinkText()).resolves.toBe(
      "https://new.test/acme/projects/project-1/pages/page-1"
    );
    expect(stub.calls.permissions).toEqual([["clipboard-read", "clipboard-write"]]);
    expect(copyClick).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("signs out by clearing cookies and waiting for the sign-in card", async () => {
    const waitFor = vi.fn(async () => undefined);
    const email = loc({ waitFor });
    const stub = pageDouble({
      url: "http://new.test/",
      page: {
        getByPlaceholder: (placeholder: string) => (placeholder === "name@company.com" ? email : absent()),
      },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesGatingSignOut();

    expect(stub.calls.cookiesCleared).toBe(1);
    expect(stub.calls.gotos).toEqual(["/"]);
    expect(waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
  });
});

describe("edge branches", () => {
  it("classifies an unparsable address by its bare path", async () => {
    const stub = pageDouble();
    // A route family matcher sees any string a browser hands it.
    const driver = new WebNewDriver(stub.page);
    await driver.pagesGatingFailListFetch();
    const call = stub.calls.routes[0];
    if (call === undefined) throw new Error("expected the list family to arm");
    const plain = call.matcher as (url: URL) => boolean;
    // The matcher reads the href off a URL, so pass a URL that never parses
    // through the driver's own fallback path instead.
    const raw = { href: "::::" } as unknown as URL;
    expect(plain(raw)).toBe(false);
    await driver.pagesGatingUnfailListFetch();
  });

  it("gives up on a row set that never settles", async () => {
    // Every read names a new row, so the two-read agreement never lands and
    // the deadline is the only exit.
    let read = 0;
    const links = loc({
      all: async () => [loc({ isVisible: async () => true, getAttribute: async () => `Row ${read++}` })],
    });
    const rows = loc({ locator: () => links });
    const field = loc({ fill: async () => undefined });
    const search = loc({ role: () => field });
    const driver = new WebNewDriver(
      pageDouble({
        testIds: (id) => (id === "page-row" ? rows : id === "pages-list-search" ? search : absent()),
      }).page
    );
    // The search type reads rows until two reads agree; with a fresh label
    // every read, the deadline is the only exit.
    const now = vi.spyOn(Date, "now");
    now.mockReturnValueOnce(0).mockReturnValueOnce(0).mockReturnValue(60_000);
    await expect(driver.pagesGatingSearchType("alpha")).resolves.toBeUndefined();
    expect(read).toBeGreaterThan(1);
    now.mockRestore();
  });

  it("treats a detached error-panel body, link and empty copy as blank", async () => {
    const panel = loc({
      role: (role) => {
        if (role === "heading") return loc({ innerText: async () => "Page unavailable" });
        return loc({
          getAttribute: async () => {
            throw new Error("detached");
          },
        });
      },
      locator: () =>
        loc({
          first: () =>
            loc({
              innerText: async () => {
                throw new Error("detached");
              },
            }),
        }),
    });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-detail-panel" ? panel : absent()) }).page
    );
    await expect(driver.pagesGatingErrorPanel()).resolves.toEqual({
      heading: "Page unavailable",
      body: null,
      backHref: null,
    });

    const empty = new WebNewDriver(
      pageDouble({
        testIds: (id) =>
          id === "pages-list-screen"
            ? loc()
            : id === "pages-empty-state"
              ? loc({
                  isVisible: async () => true,
                  first: () =>
                    loc({
                      innerText: async () => {
                        throw new Error("detached");
                      },
                    }),
                })
              : absent(),
      }).page
    );
    await expect(empty.pagesGatingListVariant()).resolves.toBe("docs-empty");
  });

  it("settles even when the row read throws mid-remount", async () => {
    // The variant read sees rows, the settle's first read sees them too, and
    // the second read detaches as a remount swaps the row list.
    let reads = 0;
    const links = loc({
      all: async () => {
        reads += 1;
        if (reads > 2) throw new Error("detached");
        return [loc({ isVisible: async () => true, getAttribute: async () => "Alpha" })];
      },
    });
    const rows = loc({ locator: () => links });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-row" ? rows : id === "pages-list-screen" ? loc() : absent()) }).page
    );
    await expect(driver.pagesGatingOpenList("acme", "project-1")).resolves.toBeUndefined();
    expect(reads).toBeGreaterThan(2);
  });

  it("waits in bounded steps for a column that has not painted a body yet", async () => {
    let reads = 0;
    const body = loc({
      locator: () =>
        loc({
          waitFor: async () => undefined,
          innerText: async () => {
            reads += 1;
            if (reads < 3) throw new Error("not painted");
            return "Body";
          },
        }),
    });
    const driver = new WebNewDriver(
      pageDouble({ testIds: (id) => (id === "page-canvas-stage" ? body : absent()) }).page
    );
    await expect(driver.pagesGatingWaitDetailSettled()).resolves.toBe("editor");
    expect(reads).toBe(3);
  });

  it("gives up waiting for a column that never paints a body", async () => {
    const body = loc({
      locator: () =>
        loc({
          waitFor: async () => undefined,
          innerText: async () => {
            throw new Error("never painted");
          },
        }),
    });
    const panel = loc({
      waitFor: async () => {
        throw new Error("no panel");
      },
      isVisible: async () => false,
    });
    const stub = pageDouble({
      testIds: (id) => (id === "page-canvas-stage" ? body : id === "page-detail-panel" ? panel : absent()),
    });
    // The wait spends its whole step budget, then reports the editor anyway
    // rather than failing the scenario.
    await expect(new WebNewDriver(stub.page).pagesGatingWaitDetailSettled()).resolves.toBe("editor");
    expect(stub.calls.timeouts).toHaveLength(120);
    expect(stub.calls.timeouts.every((ms) => ms === 250)).toBe(true);
  });

  it("reports the editor when neither body nor panel ever answers", async () => {
    const body = loc({
      locator: () =>
        loc({
          waitFor: async () => {
            throw new Error("no body");
          },
        }),
    });
    const panel = loc({
      waitFor: async () => {
        throw new Error("no panel");
      },
      isVisible: async () => false,
    });
    const stub = pageDouble({
      testIds: (id) => (id === "page-canvas-stage" ? body : id === "page-detail-panel" ? panel : absent()),
    });
    // Neither race arm settles, so the shell read is null and the call
    // answers the editor without a further wait.
    await expect(new WebNewDriver(stub.page).pagesGatingWaitDetailSettled()).resolves.toBe("editor");
    expect(stub.calls.timeouts).toEqual([]);
  });

  it("reads a title height of zero when the box read detaches", async () => {
    const stub = detailPageTitle(
      loc({
        innerText: async () => "Alpha",
        boundingBox: async () => {
          throw new Error("detached");
        },
      })
    );
    await expect(new WebNewDriver(stub.page).pagesGatingTitleFacts("page-1")).resolves.toEqual({
      text: "Alpha",
      placeholder: null,
      height: 0,
    });
  });
});

/** A detail page whose only interesting node is the title shell. */
function detailPageTitle(title: Locator): ReturnType<typeof pageDouble> {
  return pageDouble({ testIds: (id) => (id === "page-detail-title" ? title : absent()) });
}
