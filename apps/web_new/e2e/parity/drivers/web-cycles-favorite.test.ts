// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for the cycle row-favorite drive (NEWFRONT-367): the
// row occasionally swallows a toggle click, but retrying blindly
// double-drives — a delayed first handler plus a second click reads the
// optimistic flip as the opposite toggle, and the old app's remove path
// resolves without a server call while the favorite entry is still
// optimistic, leaving server-true/row-false permanently (CYC-029 failed
// at the row-star assertion with the server true and the sidebar favorite
// present). The converge drive re-clicks at most once and only while the
// row and the server agree the toggle never happened; the settle
// agreement then holds the row/server agreement until it sticks, healing
// the stale list reads that overwrite the optimistic flip. A stub Page
// stands in for Playwright; the driver only uses locator, filter, first,
// click, waitForTimeout, and route.
import type { Page, Route } from "@playwright/test";
import { describe, expect, it } from "vitest";
import { WebDriver } from "./web";

type RowState = "starred" | "unstarred" | "absent";

interface ScriptedPage {
  clicks: number;
  settles: number;
  routes: number;
  predicate: ((url: URL) => boolean) | null;
  handler: ((route: Route) => Promise<void>) | null;
  page: Page;
}

/** A stub page with a chainable row locator. Clicks, settles and routes are counted. */
function scriptedPage(): ScriptedPage {
  const state = {
    clicks: 0,
    settles: 0,
    routes: 0,
    predicate: null as ((url: URL) => boolean) | null,
    handler: null as ((route: Route) => Promise<void>) | null,
  };
  const chain = {
    filter: () => chain,
    first: () => chain,
    locator: () => chain,
    click: async () => {
      state.clicks += 1;
    },
  };
  const page = {
    locator: () => chain,
    waitForTimeout: async () => {
      state.settles += 1;
    },
    route: async (predicate: (url: URL) => boolean, handler: (route: Route) => Promise<void>) => {
      state.routes += 1;
      state.predicate = predicate;
      state.handler = handler;
    },
  };
  return Object.assign(state, { page: page as unknown as Page });
}

/** A driver whose row reads replay `states`, holding the last one. */
function driverWithRowStates(script: ScriptedPage, states: RowState[]): WebDriver {
  const queue = [...states];
  const driver = new WebDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () =>
    queue.length > 1 ? (queue.shift() ?? "absent") : (queue[0] ?? "absent");
  return driver;
}

/**
 * A driver whose row flips to `want` once the `flipOnClick`-th click has
 * landed, modeling a swallowed click. Keying the flip to the click
 * count instead of a wall-clock delay keeps the first drive's poll on
 * `initial` however many reads the poll makes before its budget runs
 * out, and the second drive's poll on `want`, on any machine speed.
 */
function driverWithClickFlip(script: ScriptedPage, initial: RowState, want: RowState, flipOnClick: number): WebDriver {
  const driver = new WebDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () => (script.clicks >= flipOnClick ? want : initial);
  return driver;
}

/** A scripted server read resolving `value` while counting calls. */
function scriptedServer(value: boolean): { calls: number; isServerFavorite: () => Promise<boolean> } {
  const state = { calls: 0 };
  return Object.assign(state, {
    isServerFavorite: async () => {
      state.calls += 1;
      return value;
    },
  });
}

describe("cyclesRowFavoriteToggleConverge", () => {
  it("returns without clicking when already starred", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["starred"]);
    const server = scriptedServer(true);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(0);
    expect(server.calls).toBe(0);
  });

  it("clicks once and converges on the flip", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["unstarred", "unstarred", "starred"]);
    const server = scriptedServer(false);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(1);
    expect(server.calls).toBe(0);
  });

  it("converges in the remove direction", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["starred", "starred", "unstarred"]);
    const server = scriptedServer(true);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "unstarred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(1);
    expect(server.calls).toBe(0);
  });

  it("re-drives after a swallowed click and converges", async () => {
    const script = scriptedPage();
    // The first 30ms poll never sees the flip; the second drive lands it.
    const driver = driverWithClickFlip(script, "unstarred", "starred", 2);
    const server = scriptedServer(false);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(2);
    expect(server.calls).toBe(1);
  });

  it("throws without a second click when the server flipped but the row did not", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["unstarred"]);
    const server = scriptedServer(true);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30)
    ).rejects.toThrow("refusing another toggle");
    expect(script.clicks).toBe(1);
    expect(server.calls).toBe(1);
  });

  it("refuses a second toggle in the remove direction when the server already flipped", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["starred"]);
    const server = scriptedServer(false);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "unstarred", server.isServerFavorite, 30)
    ).rejects.toThrow("refusing another toggle");
    expect(script.clicks).toBe(1);
    expect(server.calls).toBe(1);
  });

  it("throws after two toggles when nothing flips", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["unstarred"]);
    const server = scriptedServer(false);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30)
    ).rejects.toThrow("after 2 toggles");
    expect(script.clicks).toBe(2);
    expect(server.calls).toBe(2);
  });

  it("throws after two toggles in the remove direction when nothing flips", async () => {
    const script = scriptedPage();
    const driver = driverWithRowStates(script, ["starred"]);
    const server = scriptedServer(true);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "unstarred", server.isServerFavorite, 30)
    ).rejects.toThrow("after 2 toggles");
    expect(script.clicks).toBe(2);
    expect(server.calls).toBe(2);
  });
});

/**
 * A driver whose row and server reads follow scripted timelines of the
 * click count and the read counts, so clobbers, rollbacks and late
 * commits replay deterministically without any clock.
 */
function driverWithTimeline(
  script: ScriptedPage,
  rowAt: (clicks: number, rowReads: number) => RowState,
  serverAt: (clicks: number, serverReads: number) => boolean
): { driver: WebDriver; isServerFavorite: () => Promise<boolean> } {
  let rowReads = 0;
  let serverReads = 0;
  const driver = new WebDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () => {
    rowReads += 1;
    return rowAt(script.clicks, rowReads);
  };
  return {
    driver,
    isServerFavorite: async () => {
      serverReads += 1;
      return serverAt(script.clicks, serverReads);
    },
  };
}

describe("cyclesRowFavoriteSettleAgreement", () => {
  it("returns without clicking when already agreed", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      () => "starred",
      () => true
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(0);
    expect(script.settles).toBe(11);
  });

  it("heals a stale read that overwrites the flip", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks, rowReads) => ((clicks >= 1 && rowReads < 8) || clicks >= 2 ? "starred" : "unstarred"),
      () => true
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(2);
  });

  it("throws instead of healing forever when the row never sticks", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks, rowReads) => (clicks === 1 && rowReads < 8 ? "starred" : "unstarred"),
      () => true
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500)
    ).rejects.toThrow("refusing further toggles");
    expect(script.clicks).toBe(3);
  });

  it("waits out a pending server and then agrees", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks) => (clicks >= 1 ? "starred" : "unstarred"),
      (_clicks, serverReads) => serverReads >= 5
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(1);
  });

  it("throws when the server never flips", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks) => (clicks >= 1 ? "starred" : "unstarred"),
      () => false
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 40)
    ).rejects.toThrow("never settled on starred within 40ms");
    expect(script.clicks).toBe(1);
  });

  it("re-drives after a rollback and then agrees", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks, rowReads) => ((clicks === 1 && rowReads < 7) || clicks >= 2 ? "starred" : "unstarred"),
      (_clicks, serverReads) => serverReads >= 6
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(2);
  });

  it("throws after two reconciling toggles when nothing settles", async () => {
    const script = scriptedPage();
    const timeline = driverWithTimeline(
      script,
      (clicks, rowReads) => (clicks === 1 && rowReads < 7 ? "starred" : "unstarred"),
      () => false
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500)
    ).rejects.toThrow("after 2 reconciling toggles");
    expect(script.clicks).toBe(3);
  });
});

/** A stub route with a scripted method/body and a counted continue. */
function stubRoute(options: { method: string; body: unknown; throws: boolean }): {
  continued: number;
  route: Route;
} {
  const state = { continued: 0 };
  const request = {
    method: () => options.method,
    postDataJSON: () => {
      if (options.throws) throw new Error("not json");
      return options.body;
    },
  };
  const route = {
    request: () => request,
    continue: async () => {
      state.continued += 1;
    },
  };
  return Object.assign(state, { route: route as unknown as Route });
}

describe("cyclesDelayFavoriteWrites", () => {
  it("installs the route once and matches only the workspace favorites endpoint", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    await driver.cyclesDelayFavoriteWrites(100);
    expect(script.routes).toBe(1);
    const predicate = script.predicate ?? (() => false);
    expect(predicate(new URL("http://localhost/api/workspaces/w/user-favorites/"))).toBe(true);
    expect(predicate(new URL("http://localhost/api/workspaces/w/user-favorites/abc/?all=true"))).toBe(true);
    expect(predicate(new URL("http://localhost/api/workspaces/w/projects/p/user-favorite-cycles/"))).toBe(false);
    expect(predicate(new URL("http://localhost/api/workspaces/w/projects/p/cycles/"))).toBe(false);
  });

  it("holds the cycle-favorite create while the delay stands", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    const stub = stubRoute({ method: "POST", body: { entity_type: "cycle" }, throws: false });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeGreaterThanOrEqual(80);
    expect(stub.continued).toBe(1);
  });

  it("continues immediately once the delay is cleared", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    await driver.cyclesClearFavoriteDelays();
    const stub = stubRoute({ method: "POST", body: { entity_type: "cycle" }, throws: false });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeLessThan(80);
    expect(stub.continued).toBe(1);
  });

  it("continues reads immediately while the delay stands", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    const stub = stubRoute({ method: "GET", body: null, throws: false });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeLessThan(80);
    expect(stub.continued).toBe(1);
  });

  it("continues non-cycle creates immediately while the delay stands", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    const stub = stubRoute({ method: "POST", body: { entity_type: "folder" }, throws: false });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeLessThan(80);
    expect(stub.continued).toBe(1);
  });

  it("continues empty bodies immediately while the delay stands", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    const stub = stubRoute({ method: "POST", body: null, throws: false });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeLessThan(80);
    expect(stub.continued).toBe(1);
  });

  it("continues non-JSON bodies immediately while the delay stands", async () => {
    const script = scriptedPage();
    const driver = new WebDriver(script.page);
    await driver.cyclesDelayFavoriteWrites(100);
    const stub = stubRoute({ method: "POST", body: null, throws: true });
    const started = Date.now();
    await script.handler?.(stub.route);
    expect(Date.now() - started).toBeLessThan(80);
    expect(stub.continued).toBe(1);
  });
});
