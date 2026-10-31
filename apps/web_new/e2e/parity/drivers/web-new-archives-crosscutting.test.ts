// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archives cross-cutting driver family (NEWFRONT-703,
// ARCH-026/031/032). A stub Page replays the project dialog confirm,
// the project write shaping, and the drag/export/key absence probes
// here instead of a browser; route handlers are captured and invoked
// directly so every shipped line and branch runs.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

type EvaluateFn = (node: unknown) => unknown;

function loc(
  overrides: {
    count?: () => Promise<number>;
    innerText?: () => Promise<string>;
    isVisible?: () => Promise<boolean>;
    isEnabled?: () => Promise<boolean>;
    waitFor?: (options?: unknown) => Promise<void>;
    click?: (options?: unknown) => Promise<void>;
    press?: (key: string, options?: unknown) => Promise<void>;
    scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
    dragTo?: (target: Locator, options?: unknown) => Promise<void>;
    boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
    evaluate?: (pageFunction: EvaluateFn) => Promise<unknown>;
    nested?: (selector: string) => Locator;
    byTestId?: (id: string) => Locator;
    byRole?: (role: string, options?: unknown) => Locator;
    byText?: (text: string) => Locator;
    filtered?: (options?: unknown) => Locator;
    nth?: (index: number) => Locator;
    last?: () => Locator;
  } = {}
): Locator {
  const node: Record<string, unknown> = {};
  node["count"] = overrides.count ?? (async () => 1);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["isEnabled"] = overrides.isEnabled ?? (async () => true);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["press"] = overrides.press ?? (async () => undefined);
  node["scrollIntoViewIfNeeded"] = overrides.scrollIntoViewIfNeeded ?? (async () => undefined);
  node["dragTo"] = overrides.dragTo ?? (async () => undefined);
  node["boundingBox"] = overrides.boundingBox ?? (async () => null);
  node["evaluate"] = overrides.evaluate ?? (async () => null);
  const self = node as unknown as Locator;
  node["first"] = () => self;
  node["nth"] = (index: number) => overrides.nth?.(index) ?? self;
  node["last"] = () => overrides.last?.() ?? self;
  node["locator"] = (selector: string) => overrides.nested?.(selector) ?? self;
  node["getByTestId"] = (id: string) => overrides.byTestId?.(id) ?? self;
  node["getByRole"] = (role: string, options?: unknown) => overrides.byRole?.(role, options) ?? self;
  node["getByText"] = (text: string) => overrides.byText?.(text) ?? self;
  node["filter"] = (options?: unknown) => overrides.filtered?.(options) ?? self;
  return self;
}

interface RouteCall {
  matcher: unknown;
  handler: (route: Route) => Promise<void>;
}

interface PageCalls {
  routes: RouteCall[];
  timeouts: number[];
}

function stubPage(config: {
  locatorFor?: (selector: string) => Locator;
  roleFor?: (role: string, options?: unknown) => Locator;
}): { page: Page; calls: PageCalls } {
  const calls: PageCalls = { routes: [], timeouts: [] };
  const page = {
    url: () => "http://new.test/",
    locator: (selector: string) => config.locatorFor?.(selector) ?? loc(),
    getByRole: (role: string, options?: unknown) => config.roleFor?.(role, options) ?? loc(),
    route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
      calls.routes.push({ matcher, handler });
    },
    waitForTimeout: async (ms: number) => {
      calls.timeouts.push(ms);
    },
  };
  return { page: page as unknown as Page, calls };
}

interface RouteOutcome {
  continued: number;
  fulfilled: Array<Record<string, unknown>>;
}

function fakeRoute(method: string, url: string, outcome: RouteOutcome): Route {
  return {
    request: () => ({ method: () => method, url: () => url }),
    continue: async () => {
      outcome.continued += 1;
    },
    fulfill: async (options: Record<string, unknown>) => {
      outcome.fulfilled.push(options);
    },
  } as unknown as Route;
}

function dialogPage(confirm: Locator): { page: Page; calls: PageCalls } {
  return stubPage({
    locatorFor: (selector) =>
      selector === '[role="dialog"]' ? loc({ byRole: () => loc({ last: () => confirm }) }) : loc(),
  });
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("archives project confirm drivers", () => {
  it("reads the dialog confirm label trimmed", async () => {
    const { page } = dialogPage(loc({ innerText: async () => "  Restore  " }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmLabel()).toBe("Restore");
  });

  it("reads an empty label when the confirm carries no text", async () => {
    const { page } = dialogPage(loc({ innerText: async () => null as unknown as string }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmLabel()).toBe("");
  });

  it("reports idle when no confirm shows", async () => {
    const { page } = dialogPage(loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmBusy()).toBe(false);
  });

  it("reports busy while the confirm relabels mid-write", async () => {
    const { page } = dialogPage(loc({ innerText: async () => "Archiving…" }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmBusy()).toBe(true);
  });

  it("reports idle for an enabled confirm with its idle label", async () => {
    const { page } = dialogPage(loc({ innerText: async () => "Archive", isEnabled: async () => true }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmBusy()).toBe(false);
  });

  it("reads a textless enabled confirm as idle", async () => {
    const { page } = dialogPage(loc({ innerText: async () => null as unknown as string, isEnabled: async () => true }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmBusy()).toBe(false);
  });

  it("reports busy for a disabled confirm even with its idle label", async () => {
    const { page } = dialogPage(loc({ innerText: async () => "Restore", isEnabled: async () => false }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesProjectConfirmBusy()).toBe(true);
  });
});

describe("archives project write shaping", () => {
  const WRITE = "http://new.test/api/workspaces/acme/projects/11111111-1111-1111-1111-111111111111/archive/";
  const ITEM_WRITE =
    "http://new.test/api/workspaces/acme/projects/11111111-1111-1111-1111-111111111111/issues/22222222-2222-2222-2222-222222222222/archive/";
  const OTHER = "http://new.test/api/workspaces/acme/projects/";

  it("delays the next project archive write once, then passes everything through", async () => {
    const timer = vi.spyOn(globalThis, "setTimeout");
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);
    await driver.archivesProjectDelayNextWrite(30);
    const outcome: RouteOutcome = { continued: 0, fulfilled: [] };
    const handler = calls.routes[0]?.handler;
    expect(calls.routes[0]?.matcher).toBe("**/projects/*/archive/");
    if (handler === undefined) throw new Error("expected the delay handler to register");

    await handler(fakeRoute("GET", WRITE, outcome));
    await handler(fakeRoute("POST", OTHER, outcome));
    await handler(fakeRoute("POST", ITEM_WRITE, outcome));
    await handler(fakeRoute("POST", WRITE, outcome));
    await handler(fakeRoute("DELETE", WRITE, outcome));

    expect(outcome.continued).toBe(5);
    expect(timer).toHaveBeenCalledTimes(1);
    expect(timer).toHaveBeenCalledWith(expect.any(Function), 30);
  });

  it("delays a restore write when it arrives first", async () => {
    const timer = vi.spyOn(globalThis, "setTimeout");
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);
    await driver.archivesProjectDelayNextWrite(15);
    const outcome: RouteOutcome = { continued: 0, fulfilled: [] };
    const handler = calls.routes[0]?.handler;
    if (handler === undefined) throw new Error("expected the delay handler to register");

    await handler(fakeRoute("DELETE", WRITE, outcome));

    expect(outcome.continued).toBe(1);
    expect(timer).toHaveBeenCalledWith(expect.any(Function), 15);
  });

  it("fails the next project archive write once with a 500, then passes through", async () => {
    const { page, calls } = stubPage({});
    const driver = new WebNewDriver(page);
    await driver.archivesProjectFailNextWrite();
    const outcome: RouteOutcome = { continued: 0, fulfilled: [] };
    const handler = calls.routes[0]?.handler;
    expect(calls.routes[0]?.matcher).toBe("**/projects/*/archive/");
    if (handler === undefined) throw new Error("expected the fail handler to register");

    await handler(fakeRoute("GET", WRITE, outcome));
    await handler(fakeRoute("POST", OTHER, outcome));
    await handler(fakeRoute("POST", ITEM_WRITE, outcome));
    await handler(fakeRoute("DELETE", WRITE, outcome));
    await handler(fakeRoute("POST", WRITE, outcome));

    expect(outcome.continued).toBe(4);
    expect(outcome.fulfilled).toHaveLength(1);
    expect(outcome.fulfilled[0]?.["status"]).toBe(500);
    expect(outcome.fulfilled[0]?.["contentType"]).toBe("application/json");
    expect(JSON.parse(String(outcome.fulfilled[0]?.["body"]))).toEqual({ error: "parity project archive failure" });
  });
});

describe("archives absence probes", () => {
  function contentPage(textFor: (text: string) => Locator): { page: Page; calls: PageCalls } {
    return stubPage({
      locatorFor: (selector) => (selector === '[data-testid="archives-content"]' ? loc({ byText: textFor }) : loc()),
    });
  }

  function draggableNode(closest: unknown, queried: unknown): unknown {
    return {
      closest: (_selector: string) => closest,
      querySelector: (_selector: string) => queried,
    };
  }

  it("reports a missing row as not draggable", async () => {
    const { page } = contentPage(() => loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowDraggable("Cycle A")).toBe(false);
  });

  it("reports draggable when an ancestor carries the marker", async () => {
    const row = loc({ evaluate: async (fn) => fn(draggableNode({}, null)) });
    const { page } = contentPage(() => row);
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowDraggable("Cycle A")).toBe(true);
  });

  it("reports draggable when a descendant carries the marker", async () => {
    const row = loc({ evaluate: async (fn) => fn(draggableNode(null, {})) });
    const { page } = contentPage(() => row);
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowDraggable("Module B")).toBe(true);
  });

  it("reports not draggable when neither ancestor nor descendant carries it", async () => {
    const row = loc({ evaluate: async (fn) => fn(draggableNode(null, null)) });
    const { page } = contentPage(() => row);
    const driver = new WebNewDriver(page);

    expect(await driver.archivesRowDraggable("Module B")).toBe(false);
  });

  function boxLoc(y: Array<number | null>, drags: string[]): Locator {
    const queue = [...y];
    return loc({
      boundingBox: async () => {
        const next = queue.shift() ?? null;
        return next === null ? null : { x: 0, y: next, width: 10, height: 10 };
      },
      dragTo: async (target: Locator) => {
        drags.push(target === undefined ? "unset" : "set");
      },
    });
  }

  it("reports no reorder when a drag leaves the row order alone", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([100, 100], drags),
      "issue-b": boxLoc([200, 200], drags),
    };
    const { page, calls } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
    expect(drags).toHaveLength(1);
    expect(calls.timeouts).toEqual([2000]);
  });

  it("reports a reorder when a drag flips the row order", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([100, 300], drags),
      "issue-b": boxLoc([200, 150], drags),
    };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(true);
  });

  it("reports no reorder when the source box is missing before the drag", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([null, 100], drags),
      "issue-b": boxLoc([200, 200], drags),
    };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
  });

  it("reports no reorder when the target box is missing before the drag", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([100, 100], drags),
      "issue-b": boxLoc([null, 200], drags),
    };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
  });

  it("reports no reorder when the source box is missing after the drag", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([100, null], drags),
      "issue-b": boxLoc([200, 200], drags),
    };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
  });

  it("reports no reorder when the target box is missing after the drag", async () => {
    const drags: string[] = [];
    const rows: Record<string, Locator> = {
      "issue-a": boxLoc([100, 100], drags),
      "issue-b": boxLoc([200, null], drags),
    };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
  });

  it("reports no reorder when a box read rejects mid-probe", async () => {
    const drags: string[] = [];
    let calls = 0;
    const flaky = loc({
      boundingBox: async () => {
        calls += 1;
        if (calls === 2) throw new Error("detached");
        return { x: 0, y: 100, width: 10, height: 10 };
      },
      dragTo: async () => {
        drags.push("set");
      },
    });
    const rows: Record<string, Locator> = { "issue-a": flaky, "issue-b": boxLoc([200, 200], drags) };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
  });

  it("tolerates a scroll that rejects mid-probe", async () => {
    const drags: string[] = [];
    const src = loc({
      boundingBox: async () => ({ x: 0, y: 100, width: 10, height: 10 }),
      scrollIntoViewIfNeeded: async () => {
        throw new Error("detached");
      },
      dragTo: async () => {
        drags.push("set");
      },
    });
    const rows: Record<string, Locator> = { "issue-a": src, "issue-b": boxLoc([200, 200], drags) };
    const { page } = contentPage((text) => rows[text] ?? loc({ count: async () => 0 }));
    const driver = new WebNewDriver(page);

    expect(await driver.archivesDragReorders("issue-a", "issue-b")).toBe(false);
    expect(drags).toHaveLength(1);
  });

  it("sees an export control when one renders", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc() }) : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesExportControlVisible()).toBe(true);
  });

  it("sees no export control when none renders", async () => {
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === '[data-testid="archives-content"]' ? loc({ byRole: () => loc({ count: async () => 0 }) }) : loc(),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.archivesExportControlVisible()).toBe(false);
  });

  it("presses the key against the page body", async () => {
    const presses: Array<{ key: string; options: unknown }> = [];
    const { page } = stubPage({
      locatorFor: (selector) =>
        selector === "body" ? loc({ press: async (key, options) => void presses.push({ key, options }) }) : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPressKey("a");

    expect(presses).toHaveLength(1);
    expect(presses[0]?.key).toBe("a");
    expect(presses[0]?.options).toMatchObject({ timeout: 30_000 });
  });
});
