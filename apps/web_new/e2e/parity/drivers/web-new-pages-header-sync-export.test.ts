// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver pages detail header/sync/export
// probes (NEWFRONT-923, PAGE-055-064, 074). A Page double proves the
// addresses, selectors, route interceptions, request spies and download
// reads; no browser or parity stack is involved. The three DOM-walker
// reads (oversized strip, sync badge text, toast text) run against real
// markup in web-new-pages-header-sync-export-dom.test.ts.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Download, Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface LocatorOptions {
  count?: number;
  visible?: boolean;
  innerText?: () => Promise<string | null>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: { state?: string; timeout?: number }) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  selectOption?: (selection: unknown, options?: unknown) => Promise<void>;
  locator?: (selector: string) => Locator;
  getByRole?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator;
  getByText?: (text: string | RegExp, options?: { exact?: boolean }) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
  last?: () => Locator;
  evaluate?: (fn: (node: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  isVisible?: () => Promise<boolean>;
}

class FakeLocator {
  readonly waitFor: ReturnType<typeof vi.fn>;
  readonly click: ReturnType<typeof vi.fn>;
  readonly selectOption: ReturnType<typeof vi.fn>;
  readonly evaluate: ReturnType<typeof vi.fn>;
  readonly isVisible: ReturnType<typeof vi.fn>;

  constructor(private readonly options: LocatorOptions = {}) {
    this.waitFor = vi.fn(this.options.waitFor ?? (async () => undefined));
    this.click = vi.fn(this.options.click ?? (async () => undefined));
    this.selectOption = vi.fn(this.options.selectOption ?? (async () => undefined));
    this.evaluate = vi.fn(this.options.evaluate ?? (async () => undefined));
    this.isVisible = vi.fn(
      this.options.isVisible ?? (async () => this.options.visible ?? (this.options.count ?? 0) > 0)
    );
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.count ?? 0);
  }

  innerText(): Promise<string | null> {
    return (this.options.innerText ?? (async () => ""))();
  }

  getAttribute(name: string): Promise<string | null> {
    return (this.options.getAttribute ?? (async () => null))(name);
  }

  locator(selector: string): Locator {
    return this.options.locator?.(selector) ?? empty();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): Locator {
    return this.options.getByRole?.(role, options) ?? empty();
  }

  getByText(text: string | RegExp, options?: { exact?: boolean }): Locator {
    return this.options.getByText?.(text, options) ?? empty();
  }

  filter(options: unknown): Locator {
    return this.options.filter?.(options) ?? this.asLocator();
  }

  first(): Locator {
    return this.options.first?.() ?? this.asLocator();
  }

  nth(index: number): Locator {
    return this.options.nth?.(index) ?? empty();
  }

  last(): Locator {
    return this.options.last?.() ?? this.asLocator();
  }

  private asLocator(): Locator {
    return this as unknown as Locator;
  }
}

function empty(): Locator {
  return new FakeLocator({ count: 0 }) as unknown as Locator;
}

function at(options: LocatorOptions = {}): Locator {
  return new FakeLocator({ count: 1, ...options }) as unknown as Locator;
}

function menuOf(labels: Array<string | null>): Locator {
  return at({
    count: labels.length,
    nth: (index) =>
      at({
        innerText:
          labels[index] === null
            ? async () => {
                throw new Error("menu item detached");
              }
            : async () => labels[index] ?? "",
      }),
  });
}

interface PageOptions {
  testIds?: Record<string, Locator>;
  roles?: Record<string, Locator>;
  selectors?: Record<string, Locator>;
  evaluate?: (fn: (arg: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  url?: string;
  waitForEvent?: (event: string, options?: unknown) => Promise<unknown>;
}

interface PageDouble {
  page: Page;
  getByTestId: ReturnType<typeof vi.fn>;
  getByRole: ReturnType<typeof vi.fn>;
  locator: ReturnType<typeof vi.fn>;
  route: ReturnType<typeof vi.fn>;
  on: ReturnType<typeof vi.fn>;
  grantPermissions: ReturnType<typeof vi.fn>;
  keyboardPress: ReturnType<typeof vi.fn>;
  waitForTimeout: ReturnType<typeof vi.fn>;
  listeners: Record<string, (payload: unknown) => void>;
  routes: Array<{ pattern: string | RegExp; handler: (route: Route) => Promise<void> }>;
}

function pageDouble(options: PageOptions = {}): PageDouble {
  const listeners: Record<string, (payload: unknown) => void> = {};
  const routes: PageDouble["routes"] = [];
  const grantPermissions = vi.fn(async () => undefined);
  const keyboardPress = vi.fn(async () => undefined);
  const waitForTimeout = vi.fn(async () => undefined);
  const getByTestId = vi.fn((id: string) => options.testIds?.[id] ?? empty());
  const getByRole = vi.fn((role: string) => options.roles?.[role] ?? empty());
  const locator = vi.fn((selector: string) => options.selectors?.[selector] ?? empty());
  const route = vi.fn(async (pattern: string | RegExp, handler: (route: Route) => Promise<void>) => {
    routes.push({ pattern, handler });
  });
  const on = vi.fn((event: string, handler: (payload: unknown) => void) => {
    listeners[event] = handler;
  });
  const page = {
    context: () => ({ grantPermissions }),
    keyboard: { press: keyboardPress },
    getByTestId,
    getByRole,
    locator,
    route,
    on,
    waitForTimeout,
    evaluate: vi.fn(options.evaluate ?? (async () => undefined)),
    url: () => options.url ?? "https://app.test/",
    waitForEvent: vi.fn(
      options.waitForEvent ??
        (async () => {
          throw new Error("no event");
        })
    ),
  } as unknown as Page;
  return {
    page,
    getByTestId,
    getByRole,
    locator,
    route,
    on,
    grantPermissions,
    keyboardPress,
    waitForTimeout,
    listeners,
    routes,
  };
}

function routeDouble(method: string): {
  fulfill: ReturnType<typeof vi.fn>;
  cont: ReturnType<typeof vi.fn>;
  route: Route;
} {
  const fulfill = vi.fn(async () => undefined);
  const cont = vi.fn(async () => undefined);
  const route = { request: () => ({ method: () => method }), fulfill, continue: cont } as unknown as Route;
  return { fulfill, cont, route };
}

describe("copy link", () => {
  function headerWith(copyPresent: boolean, copiedPresent: boolean): Locator {
    return at({
      getByRole: (_role, options) => {
        const name = typeof options?.name === "string" ? options.name : "";
        if (name === "Copied link") return copiedPresent ? at() : empty();
        if (name === "Copy link") return copyPresent ? at() : empty();
        return empty();
      },
    });
  }

  it("prefers the copied confirmation while both labels resolve", async () => {
    const stub = pageDouble({ testIds: { "page-detail-header": headerWith(true, true) } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderCopyLinkLabel()).resolves.toBe("Copied link");
  });

  it("reads the resting label and null when the control is absent", async () => {
    const resting = new WebNewDriver(pageDouble({ testIds: { "page-detail-header": headerWith(true, false) } }).page);
    await expect(resting.pagesHeaderCopyLinkLabel()).resolves.toBe("Copy link");

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderCopyLinkLabel()).resolves.toBeNull();
  });

  it("grants clipboard permissions and clicks either copy label", async () => {
    const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
    const buttonFake = new FakeLocator({ count: 1 });
    const header = at({
      getByRole: (role, options) => {
        seen.push({ role, options });
        return buttonFake as unknown as Locator;
      },
    });
    const stub = pageDouble({ testIds: { "page-detail-header": header } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderClickCopyLink();

    expect(stub.grantPermissions).toHaveBeenCalledWith(["clipboard-read", "clipboard-write"]);
    expect(seen).toHaveLength(1);
    expect(seen[0]?.role).toBe("button");
    expect(String(seen[0]?.options?.name)).toBe("/^(Copy link|Copied link)$/");
    expect(buttonFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the clipboard through an evaluated reader", async () => {
    const stub = pageDouble({ evaluate: async () => "https://app.test/acme/projects/p1/pages/pg-1" });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderReadClipboard()).resolves.toBe("https://app.test/acme/projects/p1/pages/pg-1");
    expect(stub.grantPermissions).toHaveBeenCalledWith(["clipboard-read", "clipboard-write"]);
  });
});

describe("favorite star", () => {
  function starWith(label: string | null, pressed: string | null): Locator {
    if (label === null && pressed === null) return empty();
    return at({
      getAttribute: async (name) => {
        if (name === "aria-label") return label;
        if (name === "aria-pressed") return pressed;
        return null;
      },
    });
  }

  it("reads the star label and null when withheld or unrecognized", async () => {
    const added = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-star": starWith("Add to favorites", "false") } }).page
    );
    await expect(added.pagesHeaderFavoriteLabel()).resolves.toBe("Add to favorites");

    const removed = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-star": starWith("Remove favorite", "true") } }).page
    );
    await expect(removed.pagesHeaderFavoriteLabel()).resolves.toBe("Remove favorite");

    const other = new WebNewDriver(pageDouble({ testIds: { "page-detail-star": starWith("Star", "false") } }).page);
    await expect(other.pagesHeaderFavoriteLabel()).resolves.toBeNull();

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderFavoriteLabel()).resolves.toBeNull();
  });

  it("reads the filled state from aria-pressed", async () => {
    const filled = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-star": starWith("Remove favorite", "true") } }).page
    );
    await expect(filled.pagesHeaderFavoriteFilled()).resolves.toBe(true);

    const resting = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-star": starWith("Add to favorites", "false") } }).page
    );
    await expect(resting.pagesHeaderFavoriteFilled()).resolves.toBe(false);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderFavoriteFilled()).resolves.toBe(false);
  });

  it("toggles through the star hook", async () => {
    const starFake = new FakeLocator({ count: 1 });
    const stub = pageDouble({ testIds: { "page-detail-star": starFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderToggleFavorite();

    expect(stub.getByTestId).toHaveBeenCalledWith("page-detail-star");
    expect(starFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the favorites panel expansion and false when withheld", async () => {
    const expanded = new WebNewDriver(
      pageDouble({ testIds: { "favorites-section-toggle": at({ getAttribute: async () => "true" }) } }).page
    );
    await expect(expanded.pagesHeaderFavoritesExpanded()).resolves.toBe(true);

    const collapsed = new WebNewDriver(
      pageDouble({ testIds: { "favorites-section-toggle": at({ getAttribute: async () => "false" }) } }).page
    );
    await expect(collapsed.pagesHeaderFavoritesExpanded()).resolves.toBe(false);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderFavoritesExpanded()).resolves.toBe(false);
  });

  it("lists favorite names and skips blank entries", async () => {
    const names: Array<string | null> = ["Alpha", "", null, "Beta"];
    const entries = at({
      count: names.length,
      nth: (index) => at({ getAttribute: async () => names[index] ?? null }),
    });
    const driver = new WebNewDriver(pageDouble({ testIds: { "favorite-entry": entries } }).page);

    await expect(driver.pagesHeaderFavoritesNames()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("fails and unfails favorite writes through one route", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderFailFavoriteWrites();
    await driver.pagesHeaderFailFavoriteWrites();

    expect(stub.route).toHaveBeenCalledTimes(1);
    expect(stub.routes).toHaveLength(1);
    const registration = stub.routes[0] as PageDouble["routes"][number];
    expect(registration.pattern).toBeInstanceOf(RegExp);
    const pattern = registration.pattern as RegExp;
    expect(pattern.test("https://api.test/api/workspaces/acme/projects/p1/favorite-pages/pg-1/")).toBe(true);
    expect(pattern.test("https://api.test/api/workspaces/acme/projects/p1/pages/pg-1/")).toBe(false);

    for (const method of ["POST", "DELETE"]) {
      const refused = routeDouble(method);
      await registration.handler(refused.route);
      expect(refused.fulfill).toHaveBeenCalledWith({ status: 500, contentType: "application/json", body: "{}" });
      expect(refused.cont).not.toHaveBeenCalled();
    }

    const read = routeDouble("GET");
    await registration.handler(read.route);
    expect(read.cont).toHaveBeenCalledTimes(1);
    expect(read.fulfill).not.toHaveBeenCalled();

    await driver.pagesHeaderUnfailFavoriteWrites();
    const passed = routeDouble("POST");
    await registration.handler(passed.route);
    expect(passed.cont).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });
});

describe("options menu", () => {
  it("opens through the header trigger and waits for the first entry", async () => {
    const triggerFake = new FakeLocator({ count: 1 });
    const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
    const header = at({
      getByRole: (role, options) => {
        seen.push({ role, options });
        return triggerFake as unknown as Locator;
      },
    });
    const menuFake = new FakeLocator({ count: 3 });
    const stub = pageDouble({
      testIds: { "page-detail-header": header },
      roles: { menuitem: menuFake as unknown as Locator },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderOpenOptions();

    expect(seen).toEqual([{ role: "button", options: { name: "Page options", exact: true } }]);
    expect(triggerFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.getByRole).toHaveBeenCalledWith("menuitem");
    expect(menuFake.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("lists menu labels normalized and skips empty or detached entries", async () => {
    const driver = new WebNewDriver(pageDouble({ roles: { menuitem: menuOf(["  Full   width  ", "", null]) } }).page);

    await expect(driver.pagesHeaderMenuItems()).resolves.toEqual(["Full width"]);
  });

  it("treats a null menu read as empty", async () => {
    const driver = new WebNewDriver(
      pageDouble({ roles: { menuitem: at({ count: 1, nth: () => at({ innerText: async () => null }) }) } }).page
    );

    await expect(driver.pagesHeaderMenuItems()).resolves.toEqual([]);
  });

  it("chooses a menu item by an escaped exact-text match", async () => {
    let captured: unknown;
    const menuFake = new FakeLocator({
      count: 8,
      filter: (options) => {
        captured = options;
        return menuFake as unknown as Locator;
      },
    });
    const stub = pageDouble({ roles: { menuitem: menuFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderChooseMenuItem("Make private (a+b)");

    const expression = (captured as { hasText: RegExp }).hasText;
    expect(expression).toBeInstanceOf(RegExp);
    expect(expression.source).toBe("^Make private \\(a\\+b\\)$");
    expect(expression.test("Make private (a+b)")).toBe(true);
    expect(expression.test("Make private (a+b) plus more")).toBe(false);
    expect(menuFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("lock", () => {
  it("reads the lock label and null when withheld or unrecognized", async () => {
    for (const label of ["Lock", "Locked", "Unlocked"]) {
      const driver = new WebNewDriver(
        pageDouble({ testIds: { "page-detail-lock": at({ getAttribute: async () => label }) } }).page
      );
      await expect(driver.pagesHeaderLockLabel()).resolves.toBe(label);
    }

    const other = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-lock": at({ getAttribute: async () => "Pin" }) } }).page
    );
    await expect(other.pagesHeaderLockLabel()).resolves.toBeNull();

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderLockLabel()).resolves.toBeNull();
  });

  it("toggles through the lock hook", async () => {
    const lockFake = new FakeLocator({ count: 1 });
    const stub = pageDouble({ testIds: { "page-detail-lock": lockFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderToggleLock();

    expect(stub.getByTestId).toHaveBeenCalledWith("page-detail-lock");
    expect(lockFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reports the editor editable only while the live body resolves", async () => {
    const selector = '[aria-label="Page body"][contenteditable="true"]';
    const stub = pageDouble({ selectors: { [selector]: at() } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderEditorEditable()).resolves.toBe(true);
    expect(stub.locator).toHaveBeenCalledWith(selector);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderEditorEditable()).resolves.toBe(false);
  });

  it("fails and unfails lock writes through one route", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderFailLockWrites();
    await driver.pagesHeaderFailLockWrites();

    expect(stub.route).toHaveBeenCalledTimes(1);
    expect(stub.routes).toHaveLength(1);
    const registration = stub.routes[0] as PageDouble["routes"][number];
    const pattern = registration.pattern as RegExp;
    expect(pattern.test("https://api.test/api/workspaces/acme/projects/p1/pages/pg-1/lock/")).toBe(true);
    expect(pattern.test("https://api.test/api/workspaces/acme/projects/p1/favorite-pages/pg-1/")).toBe(false);

    const refused = routeDouble("POST");
    await registration.handler(refused.route);
    expect(refused.fulfill).toHaveBeenCalledWith({ status: 500, contentType: "application/json", body: "{}" });
    expect(refused.cont).not.toHaveBeenCalled();

    const read = routeDouble("GET");
    await registration.handler(read.route);
    expect(read.cont).toHaveBeenCalledTimes(1);

    await driver.pagesHeaderUnfailLockWrites();
    const passed = routeDouble("DELETE");
    await registration.handler(passed.route);
    expect(passed.cont).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });
});

describe("archived marker and delete dialog", () => {
  it("reads the archived marker normalized and null when absent or detached", async () => {
    const marked = new WebNewDriver(
      pageDouble({ testIds: { "page-archived-marker": at({ innerText: async () => "  Archived at\nOct 1, 2026 " }) } })
        .page
    );
    await expect(marked.pagesHeaderArchivedMarker()).resolves.toBe("Archived at Oct 1, 2026");

    const detached = new WebNewDriver(
      pageDouble({
        testIds: {
          "page-archived-marker": at({
            innerText: async () => {
              throw new Error("detached");
            },
          }),
        },
      }).page
    );
    await expect(detached.pagesHeaderArchivedMarker()).resolves.toBeNull();

    const blank = new WebNewDriver(
      pageDouble({ testIds: { "page-archived-marker": at({ innerText: async () => null }) } }).page
    );
    await expect(blank.pagesHeaderArchivedMarker()).resolves.toBeNull();

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderArchivedMarker()).resolves.toBeNull();
  });

  function deleteDialog(): { dialog: Locator; fake: FakeLocator; seen: unknown[] } {
    const seen: unknown[] = [];
    const fake = new FakeLocator({
      count: 1,
      filter: (options) => {
        seen.push(options);
        return fake as unknown as Locator;
      },
      innerText: async () => "  Delete   page\nPermanent ",
    });
    return { dialog: fake as unknown as Locator, fake, seen };
  }

  it("scopes the delete dialog to the naming dialog", async () => {
    const { dialog, seen } = deleteDialog();
    const driver = new WebNewDriver(pageDouble({ roles: { dialog } }).page);

    await expect(driver.pagesHeaderDeleteDialogVisible()).resolves.toBe(true);
    expect(seen).toEqual([{ hasText: "Delete page" }]);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderDeleteDialogVisible()).resolves.toBe(false);
  });

  it("waits attached and reads the delete dialog text", async () => {
    const { dialog, fake } = deleteDialog();
    const driver = new WebNewDriver(pageDouble({ roles: { dialog } }).page);

    await expect(driver.pagesHeaderDeleteDialogText()).resolves.toBe("Delete page Permanent");
    expect(fake.waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
  });

  it("reads a blank delete dialog as empty", async () => {
    const dialogFake = new FakeLocator({ count: 1, innerText: async () => null });
    const driver = new WebNewDriver(pageDouble({ roles: { dialog: dialogFake as unknown as Locator } }).page);

    await expect(driver.pagesHeaderDeleteDialogText()).resolves.toBe("");
  });

  it("confirms the delete and waits for the dialog to detach", async () => {
    const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
    const confirmFake = new FakeLocator({ count: 1 });
    const dialogFake = new FakeLocator({
      count: 1,
      getByRole: (role, options) => {
        seen.push({ role, options });
        return confirmFake as unknown as Locator;
      },
    });
    const stub = pageDouble({ roles: { dialog: dialogFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderConfirmDelete();

    expect(seen).toEqual([{ role: "button", options: { name: "Delete", exact: true } }]);
    expect(confirmFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(dialogFake.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 30_000 });
  });

  it("cancels the delete through the cancel button", async () => {
    const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
    const cancelFake = new FakeLocator({ count: 1 });
    const dialogFake = new FakeLocator({
      count: 1,
      getByRole: (role, options) => {
        seen.push({ role, options });
        return cancelFake as unknown as Locator;
      },
    });
    const stub = pageDouble({ roles: { dialog: dialogFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderCancelDelete();

    expect(seen).toEqual([{ role: "button", options: { name: "Cancel", exact: true } }]);
    expect(cancelFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("canvas, toolbar and pane", () => {
  it("settles the canvas max width on two agreeing reads", async () => {
    const reads = ["720px", "900px", "1152px", "1152px"];
    const canvasFake = new FakeLocator({ count: 1, evaluate: async () => reads.shift() ?? "1152px" });
    const stub = pageDouble({ selectors: { ".frame-renderer": canvasFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderCanvasMaxWidth()).resolves.toBe("1152px");
    expect(stub.locator).toHaveBeenCalledWith(".frame-renderer");
    expect(canvasFake.evaluate).toHaveBeenCalledTimes(4);
    expect(stub.waitForTimeout).toHaveBeenCalledTimes(3);
    expect(stub.waitForTimeout).toHaveBeenCalledWith(100);
  });

  it("returns the latest canvas read when the width never settles", async () => {
    let tick = 0;
    const canvasFake = new FakeLocator({ count: 1, evaluate: async () => `${720 + (tick += 1)}px` });
    const stub = pageDouble({ selectors: { ".frame-renderer": canvasFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderCanvasMaxWidth()).resolves.toBe("746px");
    expect(canvasFake.evaluate).toHaveBeenCalledTimes(26);
    expect(stub.waitForTimeout).toHaveBeenCalledTimes(25);
  });

  it("reports the toolbar visible only while the bar shows", async () => {
    const shown = new WebNewDriver(pageDouble({ testIds: { "page-formatting-bar": at({ visible: true }) } }).page);
    await expect(shown.pagesHeaderToolbarVisible()).resolves.toBe(true);

    const hidden = new WebNewDriver(pageDouble({ testIds: { "page-formatting-bar": at({ visible: false }) } }).page);
    await expect(hidden.pagesHeaderToolbarVisible()).resolves.toBe(false);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderToolbarVisible()).resolves.toBe(false);
  });

  it("reads the pane tab from the location", async () => {
    const tabbed = new WebNewDriver(
      pageDouble({ url: "https://app.test/acme/projects/p1/pages/pg-1?paneTab=info" }).page
    );
    await expect(tabbed.pagesHeaderPaneTabParam()).resolves.toBe("info");

    const plain = new WebNewDriver(pageDouble({ url: "https://app.test/acme/projects/p1/pages/pg-1" }).page);
    await expect(plain.pagesHeaderPaneTabParam()).resolves.toBeNull();
  });

  it("reads the pane open flag and false when withheld", async () => {
    const open = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": at({ getAttribute: async () => "true" }) } }).page
    );
    await expect(open.pagesHeaderPaneOpen()).resolves.toBe(true);

    const closed = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": at({ getAttribute: async () => "false" }) } }).page
    );
    await expect(closed.pagesHeaderPaneOpen()).resolves.toBe(false);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderPaneOpen()).resolves.toBe(false);
  });
});

describe("sync badge tooltip and offline pill", () => {
  function headerWithBadges(lost: Locator | null, syncing: Locator | null): Locator {
    return at({
      getByText: (text) => {
        if (text === "Connection lost") return lost ?? empty();
        if (text === "Syncing...") return syncing ?? empty();
        return empty();
      },
    });
  }

  it("reads the connection-lost tooltip first", async () => {
    const stub = pageDouble({
      testIds: {
        "page-detail-header": headerWithBadges(at({ getAttribute: async () => "Connection lost. Retrying." }), at()),
      },
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderSyncBadgeTooltip()).resolves.toBe("Connection lost. Retrying.");
  });

  it("falls back to the syncing tooltip and null when no badge shows", async () => {
    const syncing = new WebNewDriver(
      pageDouble({
        testIds: { "page-detail-header": headerWithBadges(null, at({ getAttribute: async () => "Syncing." })) },
      }).page
    );
    await expect(syncing.pagesHeaderSyncBadgeTooltip()).resolves.toBe("Syncing.");

    const bare = new WebNewDriver(pageDouble({ testIds: { "page-detail-header": headerWithBadges(null, null) } }).page);
    await expect(bare.pagesHeaderSyncBadgeTooltip()).resolves.toBeNull();
  });

  it("reads null for a badge without a title", async () => {
    const driver = new WebNewDriver(
      pageDouble({ testIds: { "page-detail-header": headerWithBadges(at({ getAttribute: async () => null }), null) } })
        .page
    );

    await expect(driver.pagesHeaderSyncBadgeTooltip()).resolves.toBeNull();
  });

  it("reads null for a syncing badge without a title", async () => {
    const driver = new WebNewDriver(
      pageDouble({
        testIds: { "page-detail-header": headerWithBadges(null, at({ getAttribute: async () => null })) },
      }).page
    );

    await expect(driver.pagesHeaderSyncBadgeTooltip()).resolves.toBeNull();
  });

  it("reports the offline pill only while it shows", async () => {
    const offline = new WebNewDriver(pageDouble({ testIds: { "page-offline-pill": at({ visible: true }) } }).page);
    await expect(offline.pagesHeaderOfflinePill()).resolves.toBe("Offline");

    const hidden = new WebNewDriver(pageDouble({ testIds: { "page-offline-pill": at({ visible: false }) } }).page);
    await expect(hidden.pagesHeaderOfflinePill()).resolves.toBeNull();

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderOfflinePill()).resolves.toBeNull();
  });
});

describe("description and page write spies", () => {
  function descriptionRequest(method: string, url: string, body: Record<string, unknown> | null, throws = false) {
    return {
      method: () => method,
      url: () => url,
      postDataJSON: () => {
        if (throws) throw new Error("no body");
        return body;
      },
    };
  }

  const descriptionUrl = "https://api.test/api/workspaces/w1/projects/p1/pages/pg-1/description/";

  it("counts description patches and keeps the most recent keys", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderSpyDescriptionWrites();
    await driver.pagesHeaderSpyDescriptionWrites();

    expect(stub.on).toHaveBeenCalledTimes(2);
    const request = stub.listeners.request as (payload: unknown) => void;
    const response = stub.listeners.response as (payload: unknown) => void;

    request(descriptionRequest("PATCH", descriptionUrl, { description_binary: "x", description_html: "y" }));
    request(descriptionRequest("PATCH", descriptionUrl, null, true));
    request(descriptionRequest("GET", descriptionUrl, null));
    request(descriptionRequest("PATCH", "https://api.test/api/workspaces/w1/projects/p1/pages/pg-1/", {}));
    request(descriptionRequest("PATCH", descriptionUrl, { description_json: "z" }));

    await expect(driver.pagesHeaderDescriptionWriteCount()).resolves.toBe(3);
    await expect(driver.pagesHeaderDescriptionWriteKeys()).resolves.toEqual(["description_json"]);
    const times = await driver.pagesHeaderDescriptionWriteTimes();
    expect(times).toHaveLength(3);
    expect(times.every((stamp) => typeof stamp === "number")).toBe(true);

    response({
      request: () => descriptionRequest("PATCH", descriptionUrl, null),
      status: () => 200,
    });
    response({
      request: () => descriptionRequest("PATCH", descriptionUrl, null),
      status: () => 500,
    });
    response({
      request: () => descriptionRequest("GET", descriptionUrl, null),
      status: () => 200,
    });
    await expect(driver.pagesHeaderDescriptionWriteStatuses()).resolves.toEqual([200, 500]);
  });

  it("fails only description patches and leaves reads and record writes alone", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderFailDescriptionWrites();
    await driver.pagesHeaderFailDescriptionWrites();

    expect(stub.route).toHaveBeenCalledTimes(1);
    const registration = stub.routes[0] as PageDouble["routes"][number];
    expect(registration.pattern).toBeInstanceOf(RegExp);
    const pattern = registration.pattern as RegExp;
    expect(pattern.test(descriptionUrl)).toBe(true);
    expect(pattern.test("https://api.test/api/workspaces/w1/projects/p1/pages/pg-1/")).toBe(false);

    const refused = routeDouble("PATCH");
    await registration.handler(refused.route);
    expect(refused.fulfill).toHaveBeenCalledWith({ status: 500, contentType: "application/json", body: "{}" });
    expect(refused.cont).not.toHaveBeenCalled();

    const read = routeDouble("GET");
    await registration.handler(read.route);
    expect(read.cont).toHaveBeenCalledTimes(1);
    expect(read.fulfill).not.toHaveBeenCalled();

    await driver.pagesHeaderUnfailDescriptionWrites();
    const passed = routeDouble("PATCH");
    await registration.handler(passed.route);
    expect(passed.cont).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });

  it("treats a description patch without a body as keyless", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderSpyDescriptionWrites();

    const request = stub.listeners.request as (payload: unknown) => void;
    request(descriptionRequest("PATCH", descriptionUrl, null));

    await expect(driver.pagesHeaderDescriptionWriteCount()).resolves.toBe(1);
    await expect(driver.pagesHeaderDescriptionWriteKeys()).resolves.toEqual([]);
  });

  it("counts record patches and ignores description and read traffic", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderSpyPageWrites();
    await driver.pagesHeaderSpyPageWrites();

    expect(stub.on).toHaveBeenCalledTimes(1);
    const request = stub.listeners.request as (payload: unknown) => void;
    request({ method: () => "PATCH", url: () => "https://api.test/api/workspaces/w1/projects/p1/pages/pg-1/" });
    request({ method: () => "PATCH", url: () => descriptionUrl });
    request({ method: () => "GET", url: () => "https://api.test/api/workspaces/w1/projects/p1/pages/pg-1/" });

    await expect(driver.pagesHeaderPageWriteCount()).resolves.toBe(1);
  });

  it("presses the save gesture", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderPressSave();

    expect(stub.keyboardPress).toHaveBeenCalledWith("ControlOrMeta+S");
  });
});

describe("export dialog", () => {
  function exportDialog(): { dialog: Locator; fake: FakeLocator; seen: unknown[] } {
    const seen: unknown[] = [];
    const fake = new FakeLocator({
      count: 1,
      filter: (options) => {
        seen.push(options);
        return fake as unknown as Locator;
      },
    });
    return { dialog: fake as unknown as Locator, fake, seen };
  }

  it("scopes the export dialog to the naming dialog", async () => {
    const { dialog, seen } = exportDialog();
    const driver = new WebNewDriver(pageDouble({ roles: { dialog } }).page);

    await expect(driver.pagesHeaderExportDialogVisible()).resolves.toBe(true);
    expect(seen).toEqual([{ hasText: "Export page" }]);

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesHeaderExportDialogVisible()).resolves.toBe(false);
  });

  function selectWith(checked: string | null): Locator {
    if (checked === null) return empty();
    return at({
      locator: (selector) =>
        selector === "option:checked" ? at({ innerText: async () => `  ${checked}  ` }) : empty(),
    });
  }

  it("reads the export form and the page-size visibility", async () => {
    const selects: Record<string, Locator> = {
      "#page-export-format": selectWith("PDF"),
      "#page-export-scope": selectWith("Everything"),
      "#page-export-size": selectWith("A4"),
    };
    const dialogFake = new FakeLocator({ count: 1, locator: (selector) => selects[selector] ?? empty() });
    const driver = new WebNewDriver(pageDouble({ roles: { dialog: dialogFake as unknown as Locator } }).page);

    await expect(driver.pagesHeaderExportForm()).resolves.toEqual({
      format: "PDF",
      scope: "Everything",
      pageFormat: "A4",
      pageFormatVisible: true,
    });
    expect(dialogFake.waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
  });

  it("blanks missing selects and hides the page size", async () => {
    const dialogFake = new FakeLocator({ count: 1 });
    const driver = new WebNewDriver(pageDouble({ roles: { dialog: dialogFake as unknown as Locator } }).page);

    await expect(driver.pagesHeaderExportForm()).resolves.toEqual({
      format: "",
      scope: "",
      pageFormat: "",
      pageFormatVisible: false,
    });
  });

  it("blanks a detached selected option", async () => {
    const dialogFake = new FakeLocator({
      count: 1,
      locator: (selector) =>
        selector === "#page-export-format"
          ? at({
              locator: () =>
                at({
                  innerText: async () => {
                    throw new Error("detached");
                  },
                }),
            })
          : empty(),
    });
    const driver = new WebNewDriver(pageDouble({ roles: { dialog: dialogFake as unknown as Locator } }).page);

    const form = await driver.pagesHeaderExportForm();
    expect(form.format).toBe("");
  });

  it("sets the export selects by label", async () => {
    const cases = [
      {
        run: (driver: WebNewDriver) => driver.pagesHeaderExportSetFormat("Markdown"),
        id: "#page-export-format",
        label: "Markdown",
      },
      {
        run: (driver: WebNewDriver) => driver.pagesHeaderExportSetScope("No images"),
        id: "#page-export-scope",
        label: "No images",
      },
      {
        run: (driver: WebNewDriver) => driver.pagesHeaderExportSetPageFormat("Letter"),
        id: "#page-export-size",
        label: "Letter",
      },
    ] as const;
    for (const { run, id, label } of cases) {
      const seenSelectors: string[] = [];
      const selectFake = new FakeLocator({ count: 1 });
      const dialogFake = new FakeLocator({
        count: 1,
        locator: (selector) => {
          seenSelectors.push(selector);
          return selectFake as unknown as Locator;
        },
      });
      const driver = new WebNewDriver(pageDouble({ roles: { dialog: dialogFake as unknown as Locator } }).page);

      await run(driver);

      expect(seenSelectors).toEqual([id]);
      expect(selectFake.selectOption).toHaveBeenCalledWith({ label }, { timeout: 30_000 });
    }
  });

  it("confirms the export and reads the downloaded file", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pages-export-"));
    try {
      const body = `# Plan\n${"x".repeat(500)}\n`;
      const file = join(dir, "plan.md");
      writeFileSync(file, body);
      const download = { path: async () => file, suggestedFilename: () => "plan.md" } as unknown as Download;
      const waitForEvent = vi.fn(async () => download);
      const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
      const exportFake = new FakeLocator({ count: 1 });
      const dialogFake = new FakeLocator({
        count: 1,
        getByRole: (role, options) => {
          seen.push({ role, options });
          return exportFake as unknown as Locator;
        },
      });
      const stub = pageDouble({ roles: { dialog: dialogFake as unknown as Locator }, waitForEvent });
      const driver = new WebNewDriver(stub.page);

      const result = await driver.pagesHeaderExportConfirm();

      expect(waitForEvent).toHaveBeenCalledWith("download", { timeout: 30_000 });
      expect(seen).toHaveLength(1);
      expect(seen[0]?.role).toBe("button");
      expect(String(seen[0]?.options?.name)).toBe("/^(Export|Exporting)$/");
      expect(exportFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
      expect(result.fileName).toBe("plan.md");
      expect(result.byteLength).toBe(body.length);
      expect(result.headText).toBe(body.slice(0, 400));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("refuses an export with no local file", async () => {
    const download = { path: async () => null, suggestedFilename: () => "plan.md" } as unknown as Download;
    const stub = pageDouble({
      roles: { dialog: at() },
      waitForEvent: async () => download,
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesHeaderExportConfirm()).rejects.toThrow("[parity] page export produced no local file.");
  });

  it("cancels the export through the cancel button", async () => {
    const seen: Array<{ role: string; options?: { name?: string | RegExp; exact?: boolean } }> = [];
    const cancelFake = new FakeLocator({ count: 1 });
    const dialogFake = new FakeLocator({
      count: 1,
      getByRole: (role, options) => {
        seen.push({ role, options });
        return cancelFake as unknown as Locator;
      },
    });
    const stub = pageDouble({ roles: { dialog: dialogFake as unknown as Locator } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesHeaderExportCancel();

    expect(seen).toEqual([{ role: "button", options: { name: "Cancel", exact: true } }]);
    expect(cancelFake.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("wait synced", () => {
  const BODY_SELECTOR = '[aria-label="Page body"][contenteditable="true"]';
  const READONLY_SELECTOR = '[data-testid="page-detail-title"][data-readonly="true"]';

  function syncedDriver(mounts: { body: boolean; readonly: boolean }): {
    driver: WebNewDriver;
    bodyFake: FakeLocator;
    readonlyFake: FakeLocator;
  } {
    const header = at({ getByText: () => empty() });
    const fail = async (): Promise<void> => {
      throw new Error("Timeout 30000ms exceeded.");
    };
    const bodyFake = new FakeLocator({ count: 1, waitFor: mounts.body ? async () => undefined : fail });
    const readonlyFake = new FakeLocator({ count: 1, waitFor: mounts.readonly ? async () => undefined : fail });
    const stub = pageDouble({
      testIds: { "page-detail-header": header },
      selectors: {
        [BODY_SELECTOR]: bodyFake as unknown as Locator,
        [READONLY_SELECTOR]: readonlyFake as unknown as Locator,
      },
    });
    return { driver: new WebNewDriver(stub.page), bodyFake, readonlyFake };
  }

  it("settles on the live body editor when it mounts", async () => {
    const { driver, bodyFake } = syncedDriver({ body: true, readonly: false });

    await expect(driver.pagesLiveWaitSynced()).resolves.toBeUndefined();
    expect(bodyFake.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("settles on the read-only face when no live editor mounts (NEWFRONT-940)", async () => {
    const { driver, readonlyFake } = syncedDriver({ body: false, readonly: true });

    await expect(driver.pagesLiveWaitSynced()).resolves.toBeUndefined();
    expect(readonlyFake.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("fails loudly when neither face mounts", async () => {
    const { driver } = syncedDriver({ body: false, readonly: false });

    await expect(driver.pagesLiveWaitSynced()).rejects.toThrow(
      "pagesLiveWaitSynced: neither the live body editor nor the read-only face mounted within 30000ms"
    );
  });
});
