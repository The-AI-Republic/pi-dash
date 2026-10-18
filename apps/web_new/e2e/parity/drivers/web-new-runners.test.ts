// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver runners-area methods (NEWFRONT-540,
// RUN-001..005, RUN-010/011). A stub Page stands in for Playwright; the
// methods use goto, waitForResponse, getByRole, locator, getByText, url,
// waitForURL, waitForFunction, route and unroute.
import { expect as playwrightExpect } from "@playwright/test";
import type { Locator, Page, Response, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";
import { WebNewDriver } from "./web-new";

function stubRoute(method: string) {
  return {
    request: () => ({ method: () => method }),
    continue: vi.fn(async () => undefined),
    fulfill: vi.fn(async (_options: unknown) => undefined),
    abort: vi.fn(async (_errorCode?: string) => undefined),
  };
}

function stubResponse(method: string, url: string): Response {
  return { request: () => ({ method: () => method }), url: () => url } as unknown as Response;
}

function stubLocator(overrides: Record<string, unknown> = {}) {
  const locator: Record<string, unknown> = {
    waitFor: vi.fn(async () => undefined),
    count: vi.fn(async () => 0),
    first: () => locator,
    innerText: vi.fn(async () => ""),
    getAttribute: vi.fn(async () => null),
    click: vi.fn(async () => undefined),
    isVisible: vi.fn(async () => false),
    isDisabled: vi.fn(async () => false),
    all: vi.fn(async () => []),
    evaluate: vi.fn(async () => ""),
    locator: vi.fn(() => locator),
    getByRole: vi.fn(() => locator),
    getByText: vi.fn(() => locator),
    filter: vi.fn(() => locator),
    or: vi.fn(() => locator),
    allTextContents: vi.fn(async () => []),
    nth: vi.fn(() => locator),
    ...overrides,
  };
  return locator as unknown as Locator & Record<string, ReturnType<typeof vi.fn> | unknown>;
}

function pageStub(overrides: Record<string, unknown> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    url: () => "https://app.test/acme/runners",
    title: async () => "Acme - AI Agents",
    waitForResponse: vi.fn(async () => stubResponse("GET", "https://api.test/api/runners/")),
    waitForURL: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    getByRole: vi.fn(() => stubLocator()),
    getByText: vi.fn(() => stubLocator()),
    locator: vi.fn(() => stubLocator()),
    route: vi.fn(async (_pattern: string, _handler: unknown) => undefined),
    unroute: vi.fn(async (_pattern: string) => undefined),
    ...overrides,
  } as unknown as Page;
}

describe("runnersOpenOverview", () => {
  it("opens the workspace base and settles on the tab strip", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);

    await driver.runnersOpenOverview("acme");

    expect(page.goto).toHaveBeenCalledWith("/acme/runners", { timeout: 60_000 });
    expect(page.waitForResponse).toHaveBeenCalledWith(expect.any(Function), { timeout: 90_000 });
    // The waiter filters for GETs to the runner API.
    const predicate = (page.waitForResponse as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![0] as (
      response: Response
    ) => boolean;
    expect(predicate(stubResponse("GET", "https://api.test/api/runners/"))).toBe(true);
    expect(predicate(stubResponse("POST", "https://api.test/api/runners/"))).toBe(false);
  });

  it("opens the project twin with a subpath and returns early when denied", async () => {
    const denied = stubLocator({ count: vi.fn(async () => 1) });
    const page = pageStub({ getByRole: vi.fn(() => denied) });
    const driver = new WebNewDriver(page);

    await driver.runnersOpenOverview("acme", "p-1", "approvals");

    expect(page.goto).toHaveBeenCalledWith("/acme/projects/p-1/runners/approvals", { timeout: 60_000 });
    expect(await driver.runnersDeniedVisible()).toBe(true);
  });

  it("throws when an admitted page never fetched the list", async () => {
    const page = pageStub({
      waitForResponse: vi.fn(async () => {
        throw new Error("timeout");
      }),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersOpenOverview("acme")).rejects.toThrow(/list fetch never settled/);
  });
});

describe("runnersTabs and runnersOpenTab", () => {
  it("reads label, href and active state off the strip", async () => {
    const links = [
      stubLocator({
        innerText: vi.fn(async () => "Overview"),
        getAttribute: vi.fn(async (name: string) =>
          name === "href" ? "/acme/runners" : name === "aria-current" ? "page" : null
        ),
      }),
      stubLocator({
        innerText: vi.fn(async () => "Runs"),
        getAttribute: vi.fn(async (name: string) => (name === "href" ? "/acme/runners/runs" : null)),
      }),
    ];
    const strip = stubLocator({ all: vi.fn(async () => links), count: vi.fn(async () => links.length) });
    const page = pageStub({ locator: vi.fn(() => strip) });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersTabs()).resolves.toEqual([
      { label: "Overview", href: "/acme/runners", active: true },
      { label: "Runs", href: "/acme/runners/runs", active: false },
    ]);
  });

  it("rejects an unknown tab label", async () => {
    const driver = new WebNewDriver(pageStub());
    await expect(driver.runnersOpenTab("Settings")).rejects.toThrow(/unknown runners tab/);
  });

  it("clicks the named tab and waits for its route", async () => {
    const clicked = stubLocator({ count: vi.fn(async () => 1) });
    const strip = stubLocator({ filter: vi.fn(() => clicked), count: vi.fn(async () => 1) });
    const page = pageStub({ locator: vi.fn(() => strip) });
    const driver = new WebNewDriver(page);

    await driver.runnersOpenTab("Runs");

    expect(clicked.click).toHaveBeenCalled();
    expect(page.waitForURL).toHaveBeenCalledWith(expect.any(Function), { timeout: 30_000 });
    const matcher = (page.waitForURL as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![0] as (
      url: URL
    ) => boolean;
    expect(matcher(new URL("https://app.test/acme/runners/runs"))).toBe(true);
    expect(matcher(new URL("https://app.test/acme/runners/approvals"))).toBe(false);
  });
});

describe("runnersTableRows and runnersEmptyVisible", () => {
  function row(cells: string[] | null) {
    return stubLocator({
      locator: vi.fn(() =>
        stubLocator({ count: vi.fn(async () => cells?.length ?? 1), allTextContents: vi.fn(async () => cells ?? []) })
      ),
    });
  }

  it("reads the six display columns and skips the empty-state row", async () => {
    const rows = [
      row(["alpha-runner", "Alpha pod", "online", "linux / arm64", "0.1.0", "1/2/2024", "DetailsRevokeDelete"]),
      row(["No runners yet. Add a runner."]),
    ];
    const table = stubLocator({ locator: vi.fn(() => stubLocator({ all: vi.fn(async () => rows) })) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ locator: vi.fn(() => table) })) });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersTableRows()).resolves.toEqual([
      {
        name: "alpha-runner",
        pod: "Alpha pod",
        status: "online",
        osArch: "linux / arm64",
        version: "0.1.0",
        heartbeat: "1/2/2024",
      },
    ]);
  });

  it("reports the empty state from the table text", async () => {
    const table = stubLocator({ getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ locator: vi.fn(() => table) })) });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersEmptyVisible()).resolves.toBe(true);
  });
});

describe("runnersRowActions and runnersOpenDetails", () => {
  it("reports the three actions offered by the named row", async () => {
    const nameCell = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "alpha-runner") });
    const rowLocator = stubLocator({
      locator: vi.fn(() => stubLocator({ first: () => nameCell })),
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })),
    });
    const table = stubLocator({
      locator: vi.fn(() =>
        stubLocator({ first: () => rowLocator, count: vi.fn(async () => 1), nth: () => rowLocator })
      ),
    });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ locator: vi.fn(() => table) })) });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersRowActions("alpha-runner")).resolves.toEqual({
      hasDetails: true,
      hasRevoke: true,
      hasDelete: true,
    });
  });

  it("follows the Details action into the detail route", async () => {
    const details = stubLocator({ count: vi.fn(async () => 1) });
    const nameCell = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "alpha-runner") });
    const rowLocator = stubLocator({
      locator: vi.fn(() => stubLocator({ first: () => nameCell })),
      getByRole: vi.fn(() => details),
    });
    const table = stubLocator({
      locator: vi.fn(() =>
        stubLocator({ first: () => rowLocator, count: vi.fn(async () => 1), nth: () => rowLocator })
      ),
    });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ locator: vi.fn(() => table) })) });
    const driver = new WebNewDriver(page);

    await driver.runnersOpenDetails("alpha-runner");

    expect(details.click).toHaveBeenCalled();
    expect(page.waitForURL).toHaveBeenCalledWith(/\/detail\//, { timeout: 30_000 });
  });
});

describe("runnersDeleteRunner and runnersRevokeRunner", () => {
  /** A row with the named actions plus an open confirm that records clicks. */
  function harness(overrides: Record<string, unknown> = {}) {
    const clicked: string[] = [];
    const rowCel = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "alpha-runner") });
    const table = stubLocator({
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), first: () => rowCel, nth: () => rowCel })),
    });
    const action = (name: string) =>
      stubLocator({
        count: vi.fn(async () => 1),
        click: vi.fn(async () => {
          clicked.push(name);
        }),
      });
    const row = stubLocator({
      locator: vi.fn(() => stubLocator({ first: () => stubLocator({ count: vi.fn(async () => 0) }) })),
      getByRole: vi.fn((_role: string, options?: { name?: string }) => action(options?.name ?? "row")),
    });
    // rows.nth(i) → the row; its first cell matches the name.
    const rows = stubLocator({
      count: vi.fn(async () => 1),
      nth: () =>
        stubLocator({
          locator: vi.fn(() => stubLocator({ first: () => rowCel })),
          getByRole: vi.fn((_role: string, options?: { name?: string }) => action(options?.name ?? "row")),
        }),
    });
    const tableForRows = stubLocator({ locator: vi.fn(() => rows) });
    const dialog = stubLocator({ getByRole: vi.fn(() => action("confirm")) });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "main"
          ? stubLocator({ locator: vi.fn(() => tableForRows) })
          : stubLocator({ filter: vi.fn(() => dialog) })
      ),
      ...overrides,
    });
    void table;
    void row;
    return { page, clicked };
  }

  it("clicks Delete on the row, then confirms", async () => {
    const { page, clicked } = harness();
    // The post-confirm wait polls the table forever in a stub; the click
    // order is what this suite owns.
    void new WebNewDriver(page).runnersDeleteRunner("alpha-runner").catch(() => undefined);
    await vi.waitFor(() => expect(clicked).toEqual(["Delete", "confirm"]));
  });

  it("returns once the failure toast shows", async () => {
    const toast = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => "Error!\nFailed to delete runner"),
    });
    const { page, clicked } = harness({ getByText: vi.fn(() => toast) });
    await expect(
      new WebNewDriver(page).runnersDeleteRunner("alpha-runner", { expectFailure: true })
    ).resolves.toBeUndefined();
    expect(clicked).toEqual(["Delete", "confirm"]);
  });

  it("clicks Revoke on the row, then confirms", async () => {
    const { page, clicked } = harness();
    void new WebNewDriver(page).runnersRevokeRunner("alpha-runner").catch(() => undefined);
    await vi.waitFor(() => expect(clicked).toEqual(["Revoke", "confirm"]));
  });
});

describe("runnersModalCopy and runnersCancelModal", () => {
  it("reads the title and the copy block under it", async () => {
    const dialog = stubLocator({
      getByRole: vi.fn(() =>
        stubLocator({
          count: vi.fn(async () => 1),
          isVisible: vi.fn(async () => true),
          innerText: vi.fn(async () => "Delete runner?"),
          evaluate: vi.fn(async () => "Historic runs are preserved."),
        })
      ),
      locator: vi.fn(() => stubLocator({ innerText: vi.fn(async () => " Historic runs are preserved. ") })),
    });
    const page = pageStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), first: () => dialog })),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersModalCopy()).resolves.toEqual({
      title: "Delete runner?",
      body: "Historic runs are preserved.",
    });
  });

  it("returns null without an open dialog", async () => {
    const driver = new WebNewDriver(pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) }));
    await expect(driver.runnersModalCopy()).resolves.toBeNull();
  });

  it("clicks the cancel control", async () => {
    const cancel = stubLocator();
    const dialog = stubLocator({ getByRole: vi.fn(() => cancel) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ first: () => dialog })) });
    const driver = new WebNewDriver(page);

    await driver.runnersCancelModal();

    expect(cancel.click).toHaveBeenCalled();
  });

  it("does not assert a disabled cancel: the app leaves it inert, not disabled", async () => {
    // Mid-flight dismissal checks click Cancel and expect the dialog to
    // stay. The kit button stays enabled so the click resolves and the
    // handler ignores it; a disabled control would make the driver click
    // time out instead.
    const cancel = stubLocator({ isDisabled: vi.fn(async () => false) });
    const dialog = stubLocator({ getByRole: vi.fn(() => cancel) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ first: () => dialog })) });
    const driver = new WebNewDriver(page);

    await driver.runnersCancelModal();

    await expect(cancel.isDisabled()).resolves.toBe(false);
    expect(cancel.click).toHaveBeenCalled();
  });
});

describe("mutation shaping", () => {
  it("stalls only non-GET requests", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnersStallMutations(1);
    const handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;

    const get = stubRoute("GET");
    await handler(get as unknown as Route);
    expect(get.continue).toHaveBeenCalledTimes(1);

    const post = stubRoute("POST");
    await handler(post as unknown as Route);
    expect(post.continue).toHaveBeenCalledTimes(1);
  });

  it("aborts the first mutation only", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnersFailNextMutation();
    const handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;

    const first = stubRoute("DELETE");
    await handler(first as unknown as Route);
    expect(first.continue).not.toHaveBeenCalled();
    expect(first.abort).toHaveBeenCalledWith("failed");

    const second = stubRoute("DELETE");
    await handler(second as unknown as Route);
    expect(second.continue).toHaveBeenCalledTimes(1);

    const read = stubRoute("GET");
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledTimes(1);
  });

  it("releases the route", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnersReleaseMutationShaping();
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/**");
  });

  it("fails pod reads with a 500 and releases them", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnersFailPodsLoad();
    const handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;
    const read = stubRoute("GET");
    await handler(read as unknown as Route);
    expect(read.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 500 }));
    const write = stubRoute("POST");
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledTimes(1);

    await driver.runnersReleasePodsFailure();
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/pods**");
  });
});

describe("runnersRail", () => {
  it("reads the header, overview link, contacts and empty state", async () => {
    const contact = stubLocator({
      getAttribute: vi.fn(async () => "/acme/runners/chat/r-1"),
      innerText: vi.fn(async () => "alpha"),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 2) })),
    });
    const overview = stubLocator({ count: vi.fn(async () => 1), getAttribute: vi.fn(async () => "/acme/runners") });
    const rail = stubLocator({
      locator: vi.fn(() => stubLocator({ innerText: vi.fn(async () => "AI Agents") })),
      getByRole: vi.fn((role: string, options?: { name?: string }) =>
        role === "link" && options?.name === undefined
          ? stubLocator({ all: vi.fn(async () => [overview, contact]) })
          : overview
      ),
      getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const main = stubLocator({ locator: vi.fn(() => rail) });
    const page = pageStub({ getByRole: vi.fn(() => main) });
    const driver = new WebNewDriver(page);

    await expect(driver.runnersRail()).resolves.toEqual({
      header: "AI Agents",
      overviewHref: "/acme/runners",
      contacts: [{ name: "alpha", href: "/acme/runners/chat/r-1", hasDot: true }],
      emptyVisible: false,
    });
    // The rail anchors on the area's named content main: a bare `main`
    // resolves to the shell main and the nested content main, so `../aside`
    // also catches the workspace sidebar.
    expect((page.getByRole as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]).toEqual([
      "main",
      { name: "Runners content" },
    ]);
  });
});

describe("page facts", () => {
  it("reports the document title", async () => {
    const driver = new WebNewDriver(pageStub());
    await expect(driver.runnersPageTitle()).resolves.toBe("Acme - AI Agents");
  });

  it("reports the section headings present in the content main", async () => {
    const main = stubLocator({
      getByText: vi.fn((title: string) => stubLocator({ count: vi.fn(async () => (title === "Runners" ? 1 : 0)) })),
    });
    const page = pageStub({ getByRole: vi.fn(() => main) });
    const driver = new WebNewDriver(page);
    await expect(driver.runnersSectionHeadings()).resolves.toEqual(["Runners"]);
  });

  it("reads the breadcrumb leaf from the trailing-slash link", async () => {
    const crumb = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "AI Workers\n") });
    const page = pageStub({ locator: vi.fn(() => crumb) });
    const driver = new WebNewDriver(page);
    await expect(driver.runnersBreadcrumbLeaf()).resolves.toBe("AI Workers");
    expect(page.locator).toHaveBeenCalledWith('a[href$="/runners/"]');
  });

  it("returns null without a breadcrumb", async () => {
    const driver = new WebNewDriver(pageStub());
    await expect(driver.runnersBreadcrumbLeaf()).resolves.toBeNull();
  });

  it("recognizes the surrounding chrome", async () => {
    const page = pageStub({
      getByRole: vi.fn((role: string) => stubLocator({ count: vi.fn(async () => (role === "button" ? 1 : 0)) })),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.runnersChromePresent()).resolves.toBe(true);

    const fallback = pageStub({
      getByRole: vi.fn((role: string) => stubLocator({ count: vi.fn(async () => (role === "complementary" ? 1 : 0)) })),
    });
    await expect(new WebNewDriver(fallback).runnersChromePresent()).resolves.toBe(true);

    const bare = pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(bare).runnersChromePresent()).resolves.toBe(false);
  });

  it("reads the newest error toast", async () => {
    const title = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => "Error!\nFailed to delete runner"),
    });
    const page = pageStub({ getByText: vi.fn(() => title) });
    await expect(new WebNewDriver(page).runnersLastToast()).resolves.toBe("Error!\nFailed to delete runner");

    const none = pageStub({ getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(none).runnersLastToast()).resolves.toBeNull();
  });

  it("returns the current URL", async () => {
    const driver = new WebNewDriver(pageStub());
    await expect(driver.runnersCurrentUrl()).resolves.toBe("https://app.test/acme/runners");
  });
});

describe("in-flight confirm gates", () => {
  it("waits for the delete confirm to disable its action", async () => {
    let calls = 0;
    const action = stubLocator({ isDisabled: vi.fn(async () => (calls += 1) > 0) });
    const dialog = stubLocator({ getByRole: vi.fn(() => action) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ filter: vi.fn(() => dialog) })) });
    const driver = new WebNewDriver(page);

    await driver.runnersWaitRunnerDeleteWorking();

    expect(action.isDisabled).toHaveBeenCalled();
  });

  it("waits for the revoke confirm to disable its action", async () => {
    const action = stubLocator({ isDisabled: vi.fn(async () => true) });
    const dialog = stubLocator({ getByRole: vi.fn(() => action) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ filter: vi.fn(() => dialog) })) });
    const driver = new WebNewDriver(page);

    await driver.runnersWaitRunnerRevokeWorking();

    expect(action.isDisabled).toHaveBeenCalled();
  });
});

describe("playwright stub sanity", () => {
  it("polls a sync predicate", async () => {
    await playwrightExpect.poll(() => false).toBe(false);
  });
});
