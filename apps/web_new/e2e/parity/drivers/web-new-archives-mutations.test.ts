// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archived work-item mutations driver family
// (NEWFRONT-700, ARCH-004/008-013). A stub Page replays row menus,
// mutation shaping, the archive dialog and the archived detail here
// instead of a browser; route handlers are captured and invoked
// directly so every shipped line and branch runs.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

function loc(
  overrides: {
    count?: () => Promise<number>;
    innerText?: () => Promise<string>;
    isVisible?: () => Promise<boolean>;
    waitFor?: (options?: unknown) => Promise<void>;
    click?: (options?: unknown) => Promise<void>;
    scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
    nested?: (selector: string) => Locator;
    byTestId?: (id: string) => Locator;
    byRole?: (role: string, options?: unknown) => Locator;
    filtered?: (options?: unknown) => Locator;
    nth?: (index: number) => Locator;
    last?: () => Locator;
  } = {}
): Locator {
  const node: Record<string, unknown> = {};
  node["count"] = overrides.count ?? (async () => 1);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["scrollIntoViewIfNeeded"] = overrides.scrollIntoViewIfNeeded ?? (async () => undefined);
  const self = node as unknown as Locator;
  node["first"] = () => self;
  node["nth"] = (index: number) => overrides.nth?.(index) ?? self;
  node["last"] = () => overrides.last?.() ?? self;
  node["locator"] = (selector: string) => overrides.nested?.(selector) ?? self;
  node["getByTestId"] = (id: string) => overrides.byTestId?.(id) ?? self;
  node["getByRole"] = (role: string, options?: unknown) => overrides.byRole?.(role, options) ?? self;
  node["filter"] = (options?: unknown) => overrides.filtered?.(options) ?? self;
  return self;
}

interface RouteCall {
  matcher: unknown;
  handler: (route: Route) => Promise<void>;
}

interface PageCalls {
  goto: string[];
  timeouts: number[];
  routes: RouteCall[];
  unroutes: number;
  waitForURLs: string[];
  keyPresses: string[];
  clicks: number;
}

function stubPage(config: {
  url?: string | (() => string);
  locatorFor?: (selector: string) => Locator;
  roleFor?: (role: string, options?: unknown) => Locator;
  testIdFor?: (id: string) => Locator;
  onTimeout?: (calls: PageCalls) => void;
  unrouteImpl?: () => Promise<void>;
}): { page: Page; calls: PageCalls } {
  const calls: PageCalls = {
    goto: [],
    timeouts: [],
    routes: [],
    unroutes: 0,
    waitForURLs: [],
    keyPresses: [],
    clicks: [],
  };
  let now = 1_000_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  const page = {
    goto: async (url: string) => {
      calls.goto.push(url);
    },
    waitForTimeout: async (ms: number) => {
      calls.timeouts.push(ms);
      now += ms;
      config.onTimeout?.(calls);
    },
    url: () => (typeof config.url === "function" ? config.url() : (config.url ?? "http://new.test/")),
    locator: (selector: string) => config.locatorFor?.(selector) ?? loc(),
    getByRole: (role: string, options?: unknown) => config.roleFor?.(role, options) ?? loc(),
    getByTestId: (id: string) => config.testIdFor?.(id) ?? loc(),
    waitForURL: async (pattern: string) => {
      calls.waitForURLs.push(pattern);
    },
    route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
      calls.routes.push({ matcher, handler });
    },
    unroute: async () => {
      calls.unroutes += 1;
      await config.unrouteImpl?.();
    },
    keyboard: {
      press: async (key: string) => {
        calls.keyPresses.push(key);
      },
    },
  };
  return { page: page as unknown as Page, calls };
}

function fakeRoute(
  method: string,
  url: string,
  calls: { continued: number; aborted: number; fulfilled: number }
): Route {
  return {
    request: () => ({ method: () => method, url: () => url }),
    continue: async () => {
      calls.continued += 1;
    },
    abort: async () => {
      calls.aborted += 1;
    },
    fulfill: async () => {
      calls.fulfilled += 1;
    },
  } as unknown as Route;
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("archives archive dialog drivers", () => {
  it("reports no dialog when none is open", async () => {
    const { page } = stubPage({ locatorFor: () => loc({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesArchiveDialog()).toBeNull();
  });

  it("reads the open dialog's title, body and confirm label", async () => {
    const dialog = loc({
      byRole: (role) =>
        role === "heading" ? loc({ innerText: async () => "Archive Work item Web 42" }) : loc({ count: async () => 1 }),
      nested: () => loc({ innerText: async () => "Archived work items can be restored later." }),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    const found = await driver.archivesArchiveDialog();
    expect(found?.title).toBe("Archive Work item Web 42");
    expect(found?.body).toContain("restored");
    expect(found?.confirmLabel).toBe("");
  });

  it("reports no dialog when the open one has no heading", async () => {
    const dialog = loc({ byRole: () => loc({ innerText: async () => "  " }) });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesArchiveDialog()).toBeNull();
  });

  it("reads the confirm label from the Archive button", async () => {
    const dialog = loc({
      byRole: (role, options) => {
        if (role === "heading") return loc({ innerText: async () => "Archive Work item Web 42" });
        const name = (options as { name?: unknown }).name;
        if (name instanceof RegExp) return loc({ count: async () => 1, innerText: async () => "Archiving…" });
        return loc({ count: async () => 0 });
      },
      nested: () => loc({ innerText: async () => "body" }),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    expect((await driver.archivesArchiveDialog())?.confirmLabel).toBe("Archiving…");
  });

  it("falls back to the trailing action when no Archive button names itself", async () => {
    const dialog = loc({
      byRole: (role, options) => {
        if (role === "heading") return loc({ innerText: async () => "Archive Work item Web 42" });
        const name = (options as { name?: unknown } | undefined)?.name;
        if (name instanceof RegExp) return loc({ count: async () => 0 });
        return loc({ count: async () => 2, last: () => loc({ innerText: async () => "Confirm" }) });
      },
      nested: () => loc({ innerText: async () => "body" }),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    expect((await driver.archivesArchiveDialog())?.confirmLabel).toBe("Confirm");
  });

  it("tolerates a dialog torn down mid-read", async () => {
    const dialog = loc({
      byRole: () => loc({ innerText: async () => "Archive Work item Web 42" }),
      nested: () => loc({ innerText: async () => Promise.reject(new Error("detached")) }),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    const found = await driver.archivesArchiveDialog();
    expect(found?.title).toBe("Archive Work item Web 42");
    expect(found?.body).toBe("");
  });

  it("confirms the dialog and settles when it closes", async () => {
    let open = true;
    const { page } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[role="dialog"]') {
          return loc({
            count: async () => (open ? 1 : 0),
            byRole: () => loc({ count: async () => 1, click: async () => void (open = false) }),
          });
        }
        return loc({ count: async () => 0 });
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesConfirmArchiveDialog();

    expect(open).toBe(false);
  });

  it("confirms the dialog and settles on a new confirmation", async () => {
    let toasts = 0;
    const { page } = stubPage({
      locatorFor: (selector) => {
        if (selector === '[data-testid="toast-notification"]') return loc({ count: async () => toasts });
        if (selector === '[role="dialog"]') {
          return loc({ byRole: () => loc({ count: async () => 1, click: async () => void (toasts += 1) }) });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesConfirmArchiveDialog();

    expect(toasts).toBe(1);
  });

  it("confirms the dialog and still returns when nothing settles", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesConfirmArchiveDialog();

    expect(calls.timeouts.length).toBeGreaterThan(0);
  });

  it("cancels the dialog through its cancel control", async () => {
    let detached = false;
    const dialog = loc({
      byRole: () => loc({ click: async () => void (detached = true) }),
      waitFor: async () => {
        if (!detached) throw new Error("still open");
      },
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    await driver.archivesCancelArchiveDialog();

    expect(detached).toBe(true);
  });

  it("cancels the dialog even when the detach wait never lands", async () => {
    const dialog = loc({
      byRole: () => loc(),
      waitFor: async () => Promise.reject(new Error("still open")),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    await driver.archivesCancelArchiveDialog();
  });
});

describe("archives row menu drivers", () => {
  it("opens the row menu of the named archived issue", async () => {
    const clicked: string[] = [];
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues",
      locatorFor: (selector) => {
        if (selector === '[data-testid="archives-content"]') {
          return loc({
            nested: () =>
              loc({
                byTestId: (id) => {
                  clicked.push(id);
                  return loc();
                },
              }),
          });
        }
        return loc();
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesOpenRowMenu("Old login form");

    expect(clicked).toEqual(["row-menu-trigger"]);
  });

  it("opens the row menu on the cycles and modules tabs", async () => {
    for (const tab of ["cycles", "modules"] as const) {
      const { page } = stubPage({ url: `http://new.test/acme/projects/p1/archives/${tab}` });
      const driver = new WebNewDriver(page);

      await driver.archivesRowMenuOpenFirst(tab === "cycles" ? "Sprint 9" : "Auth");
    }
  });

  it("tolerates a trigger that needs no scrolling", async () => {
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues",
      locatorFor: () =>
        loc({
          nested: () =>
            loc({ byTestId: () => loc({ scrollIntoViewIfNeeded: async () => Promise.reject(new Error("gone")) }) }),
        }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesRowMenuOpenFirst("Old login form");
  });

  it("reads first-line titles and skips blank or torn-down items", async () => {
    const texts = ["Restore", "Open in new tab\nsecond line", "", "Copy link"];
    const { page } = stubPage({
      roleFor: (role) =>
        role === "menuitem"
          ? loc({
              count: async () => texts.length + 1,
              nth: (index) =>
                index >= texts.length
                  ? loc({ innerText: async () => Promise.reject(new Error("detached")) })
                  : loc({ innerText: async () => texts[index] as string }),
            })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowMenuEntryTitles()).toEqual(["Restore", "Open in new tab", "Copy link"]);
    expect(await driver.archivesRowMenuEntries()).toEqual(["Restore", "Open in new tab", "Copy link"]);
  });

  it("reads no titles when no menu is open", async () => {
    const { page } = stubPage({ roleFor: () => loc({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowMenuEntryTitles()).toEqual([]);
  });

  it("clicks the open row menu entry whose label contains the text", async () => {
    let clicked = 0;
    const { page } = stubPage({
      roleFor: () => loc({ filtered: () => loc({ click: async () => void (clicked += 1) }) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesRowMenuClick("Copy link");

    expect(clicked).toBe(1);
  });

  it("dismisses open menus with Escape", async () => {
    const { page, calls } = stubPage({
      roleFor: () => loc({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesCloseMenus();

    expect(calls.keyPresses).toEqual(["Escape"]);
  });

  it("dismisses menus even when the key and the detach wait refuse", async () => {
    const { page } = stubPage({
      roleFor: () => loc({ waitFor: async () => Promise.reject(new Error("stuck")) }),
    });
    const keyboard = page.keyboard as unknown as { press: () => Promise<void> };
    keyboard.press = async () => Promise.reject(new Error("no keyboard"));
    const driver = new WebNewDriver(page);

    await driver.archivesCloseMenus();
  });
});

describe("archives mutation shaping drivers", () => {
  it("fails the next matching mutation once and then clears itself", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesFailNextMutation("DELETE", "/issues/i1/archive/");

    expect(calls.routes).toHaveLength(1);
    const [armed] = calls.routes;
    if (armed === undefined) throw new Error("expected an armed route");
    expect((armed.matcher as (url: URL) => boolean)(new URL("http://new.test/api/issues/i1/archive/"))).toBe(true);
    expect((armed.matcher as (url: URL) => boolean)(new URL("http://new.test/api/projects/"))).toBe(false);

    const shaped = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(fakeRoute("DELETE", "http://new.test/api/issues/i1/archive/", shaped));
    expect(shaped.fulfilled).toBe(1);
    expect(calls.unroutes).toBe(1);

    const other = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(fakeRoute("GET", "http://new.test/api/issues/i1/archive/", other));
    expect(other.continued).toBe(1);
    expect(other.fulfilled).toBe(0);
  });

  it("clearing with nothing armed unroutes nothing", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesClearMutationFailure();

    expect(calls.unroutes).toBe(0);
  });

  it("clearing tolerates an unroute refusal", async () => {
    const { page, calls } = stubPage({ unrouteImpl: async () => Promise.reject(new Error("gone")) });
    const driver = new WebNewDriver(page);

    await driver.archivesFailNextMutation("POST", "/issues/i1/archive/");
    await driver.archivesClearMutationFailure();

    expect(calls.unroutes).toBe(1);
  });

  it("fails the next item archive write once across issues, cycles and modules", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesItemFailNextWrite();

    expect(calls.routes).toHaveLength(1);
    const [armed] = calls.routes;
    if (armed === undefined) throw new Error("expected an armed route");
    expect(armed.matcher).toBe("**/archive/");

    const first = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(
      fakeRoute("DELETE", "http://new.test/api/workspaces/acme/projects/p1/issues/i1/archive/", first)
    );
    expect(first.aborted).toBe(1);

    const second = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(
      fakeRoute("POST", "http://new.test/api/workspaces/acme/projects/p1/cycles/c1/archive/", second)
    );
    expect(second.continued).toBe(1);
    expect(second.aborted).toBe(0);

    const read = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(fakeRoute("GET", "http://new.test/api/workspaces/acme/projects/p1/modules/m1/archive/", read));
    expect(read.continued).toBe(1);

    const other = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(fakeRoute("DELETE", "http://new.test/api/workspaces/acme/projects/p1/issues/i1/", other));
    expect(other.continued).toBe(1);
  });
});

describe("archives mutation list drivers", () => {
  it("names the rendered archived issues", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ nested: () => loc({ count: async () => 0 }) }) : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesMutationVisibleIssueNames()).toEqual([]);
  });
});

describe("archives archive modal drivers", () => {
  function dialogPage(heading: string | null, body: string | null, tornDown: "title" | "body" | null = null) {
    return stubPage({
      locatorFor: (selector) => {
        if (selector !== '[role="dialog"]') return loc();
        if (heading === null && body === null) return loc({ count: async () => 0 });
        return loc({
          byRole: () =>
            loc({
              innerText: () =>
                tornDown === "title" ? Promise.reject(new Error("detached")) : Promise.resolve(heading ?? ""),
            }),
          nested: () =>
            loc({
              innerText: () =>
                tornDown === "body" ? Promise.reject(new Error("detached")) : Promise.resolve(body ?? ""),
            }),
        });
      },
    });
  }

  it("reads the open archive modal's title and body", async () => {
    const { page } = dialogPage("Archive Work item WEB 42", "Archived work items can be restored later.");
    const driver = new WebNewDriver(page);

    expect(await driver.archivesArchiveModalTitle()).toBe("Archive Work item WEB 42");
    expect(await driver.archivesArchiveModalBody()).toContain("restored");
  });

  it("reports no modal text when no dialog shows", async () => {
    const { page } = dialogPage(null, null);
    const driver = new WebNewDriver(page);

    expect(await driver.archivesArchiveModalTitle()).toBeNull();
    expect(await driver.archivesArchiveModalBody()).toBeNull();
  });

  it("reports blank modal text as absent", async () => {
    const { page } = dialogPage("  ", "  ");
    const driver = new WebNewDriver(page);

    expect(await driver.archivesArchiveModalTitle()).toBeNull();
    expect(await driver.archivesArchiveModalBody()).toBeNull();
  });

  it("tolerates a modal torn down mid-read", async () => {
    const tornTitle = dialogPage("Archive Work item WEB 42", "body", "title");
    expect(await new WebNewDriver(tornTitle.page).archivesArchiveModalTitle()).toBeNull();
    const tornBody = dialogPage("Archive Work item WEB 42", "body", "body");
    expect(await new WebNewDriver(tornBody.page).archivesArchiveModalBody()).toBeNull();
  });

  it("dismisses the modal through its cancel control", async () => {
    let detached = false;
    const dialog = loc({
      byRole: () => loc({ click: async () => void (detached = true) }),
      waitFor: async () => {
        if (!detached) throw new Error("still open");
      },
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    await driver.archivesArchiveModalCancel();

    expect(detached).toBe(true);
  });

  it("dismisses the modal even when the detach wait never lands", async () => {
    const dialog = loc({
      byRole: () => loc(),
      waitFor: async () => Promise.reject(new Error("still open")),
    });
    const { page } = stubPage({ locatorFor: () => dialog });
    const driver = new WebNewDriver(page);

    await driver.archivesArchiveModalCancel();
  });
});

describe("archives detail drivers", () => {
  it("opens the archived detail address raw", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await driver.archivesOpenDetailRaw("acme", "p1", "i1");

    expect(calls.goto).toEqual(["/acme/projects/p1/archives/issues/i1"]);
  });

  it("picks an entry of the detail header menu", async () => {
    let opened = 0;
    let chosen = 0;
    const { page } = stubPage({
      testIdFor: () => loc({ click: async () => void (opened += 1) }),
      roleFor: () => loc({ filtered: () => loc({ click: async () => void (chosen += 1) }) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesDetailMenuChoose("Restore");

    expect(opened).toBe(1);
    expect(chosen).toBe(1);
  });

  it("reads the breadcrumb trail as one normalized line", async () => {
    const { page } = stubPage({
      roleFor: () => loc({ innerText: async () => "Website\n  Archives\nWEB-7" }),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDetailBreadcrumbText()).toBe("Website Archives WEB-7");
  });

  it("reads a torn-down breadcrumb as blank", async () => {
    const { page } = stubPage({
      roleFor: () => loc({ innerText: async () => Promise.reject(new Error("detached")) }),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDetailBreadcrumbText()).toBe("");
  });

  it("reads the detail banner from both banner methods", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byTestId: () => loc({ innerText: async () => "This work item is archived." }) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDetailBannerText()).toContain("archived");
    expect(await driver.archivesMutationDetailBannerText()).toContain("archived");
  });

  it("reports no banner when it is absent or blank", async () => {
    const absent = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byTestId: () => loc({ count: async () => 0 }) })
          : loc(),
    });
    expect(await new WebNewDriver(absent.page).archivesDetailBannerText()).toBeNull();
    const blank = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byTestId: () => loc({ innerText: async () => "  " }) })
          : loc(),
    });
    expect(await new WebNewDriver(blank.page).archivesMutationDetailBannerText()).toBeNull();
  });

  it("tolerates a banner torn down mid-read", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byTestId: () => loc({ innerText: async () => Promise.reject(new Error("detached")) }) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDetailBannerText()).toBeNull();
  });

  it("follows the banner back to the archived list", async () => {
    let followed = 0;
    const { page, calls } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byTestId: () => loc({ byRole: () => loc({ click: async () => void (followed += 1) }) }) })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesDetailBannerBack();

    expect(followed).toBe(1);
    expect(calls.waitForURLs).toEqual(["**/archives/issues**"]);
  });

  it("spots the not-found state only when it shows", async () => {
    const shown = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc() }) : loc(),
    });
    expect(await new WebNewDriver(shown.page).archivesDetailNotFoundVisible()).toBe(true);

    const missing = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc({ count: async () => 0 }) }) : loc(),
    });
    expect(await new WebNewDriver(missing.page).archivesDetailNotFoundVisible()).toBe(false);

    const hidden = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byRole: () => loc({ isVisible: async () => false }) })
          : loc(),
    });
    expect(await new WebNewDriver(hidden.page).archivesDetailNotFoundVisible()).toBe(false);

    const torn = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byRole: () => loc({ isVisible: async () => Promise.reject(new Error("detached")) }) })
          : loc(),
    });
    expect(await new WebNewDriver(torn.page).archivesDetailNotFoundVisible()).toBe(false);
  });

  it("spots the activity composer only when it shows", async () => {
    const shown = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc() }) : loc(),
    });
    expect(await new WebNewDriver(shown.page).archivesDetailComposerVisible()).toBe(true);

    const missing = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc({ count: async () => 0 }) }) : loc(),
    });
    expect(await new WebNewDriver(missing.page).archivesDetailComposerVisible()).toBe(false);

    const torn = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ byRole: () => loc({ isVisible: async () => Promise.reject(new Error("detached")) }) })
          : loc(),
    });
    expect(await new WebNewDriver(torn.page).archivesDetailComposerVisible()).toBe(false);
  });

  it("reads the reaction control as enabled only with an undisabled button", async () => {
    const enabled = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ nested: () => loc({ nested: () => loc({ count: async () => 1 }) }) })
          : loc(),
    });
    expect(await new WebNewDriver(enabled.page).archivesDetailReactionControlEnabled()).toBe(true);

    const frozen = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]'
          ? loc({ nested: () => loc({ nested: () => loc({ count: async () => 0 }) }) })
          : loc(),
    });
    expect(await new WebNewDriver(frozen.page).archivesDetailReactionControlEnabled()).toBe(false);

    const absent = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ nested: () => loc({ count: async () => 0 }) }) : loc(),
    });
    expect(await new WebNewDriver(absent.page).archivesDetailReactionControlEnabled()).toBe(false);
  });

  it("samples the pending detail with its record read held, then settles", async () => {
    let holdFired = false;
    const { page, calls } = stubPage({
      onTimeout: (seen) => {
        if (holdFired || seen.routes.length === 0) return;
        holdFired = true;
        const [armed] = seen.routes;
        if (armed === undefined) throw new Error("expected an armed route");
        void armed.handler(
          fakeRoute("GET", "http://new.test/api/workspaces/acme/projects/p1/issues/i1", {
            continued: 0,
            aborted: 0,
            fulfilled: 0,
          })
        );
      },
    });
    const driver = new WebNewDriver(page);

    const out = await driver.archivesDetailPendingStateOnHeldFetch("acme", "p1", "i1");

    expect(out.heldRequests).toBe(1);
    expect(out.settled).toBe(true);
    expect(calls.goto).toEqual(["/acme/projects/p1/archives/issues/i1"]);
    expect(calls.unroutes).toBe(1);

    // After release the hold passes reads through, whatever the method.
    const [armed] = calls.routes;
    if (armed === undefined) throw new Error("expected an armed route");
    const held = armed.matcher as (url: URL) => boolean;
    expect(held(new URL("http://new.test/api/workspaces/acme/projects/p1/issues/i1"))).toBe(true);
    expect(held(new URL("http://new.test/api/workspaces/acme/projects/p1/issues/i2"))).toBe(false);
    expect(held(new URL("http://other.test/projects/p1/issues/i1"))).toBe(false);
    const after = { continued: 0, aborted: 0, fulfilled: 0 };
    await armed.handler(fakeRoute("POST", "http://new.test/api/workspaces/acme/projects/p1/issues/i1", after));
    await armed.handler(fakeRoute("GET", "http://new.test/api/workspaces/acme/projects/p1/issues/i1", after));
    expect(after.continued).toBe(2);
  });

  it("throws when the detail never issues its record read", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);

    await expect(driver.archivesDetailPendingStateOnHeldFetch("acme", "p1", "i1")).rejects.toThrow(
      "never issued its record read"
    );
    expect(calls.unroutes).toBe(1);
  });
});
