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
    fill: vi.fn(async (_value: string) => undefined),
    isVisible: vi.fn(async () => false),
    isDisabled: vi.fn(async () => false),
    isEnabled: vi.fn(async () => true),
    all: vi.fn(async () => []),
    evaluate: vi.fn(async () => ""),
    locator: vi.fn(() => locator),
    getByRole: vi.fn(() => locator),
    getByText: vi.fn(() => locator),
    getByTestId: vi.fn(() => locator),
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
    getByTestId: vi.fn(() => stubLocator()),
    getByPlaceholder: vi.fn(() => stubLocator()),
    url: () => "https://app.test/acme/runners",
    title: async () => "Acme - AI Agents",
    waitForResponse: vi.fn(async () => stubResponse("GET", "https://api.test/api/runners/")),
    waitForURL: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    evaluate: vi.fn(async () => 0),
    getByRole: vi.fn(() => stubLocator()),
    getByText: vi.fn(() => stubLocator()),
    locator: vi.fn(() => stubLocator()),
    route: vi.fn(async (_pattern: string, _handler: unknown) => undefined),
    unroute: vi.fn(async (_pattern: string) => undefined),
    on: vi.fn(),
    off: vi.fn(),
    waitForTimeout: vi.fn(async (_ms: number) => undefined),
    waitForLoadState: vi.fn(async () => undefined),
    reload: vi.fn(async () => undefined),
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

    expect(cancel.click).toHaveBeenCalledWith({ force: true });
  });

  it("forces the click past aria-disabled: the app guard decides (NEWFRONT-557)", async () => {
    // Mid-flight dismissal checks click Cancel and expect the dialog to
    // stay. The confirm's Cancel carries aria-disabled while pending
    // (inert, never unmounted); Playwright treats that as unactionable,
    // so the driver forces the click and the app's own guard ignores it.
    const cancel = stubLocator({ isDisabled: vi.fn(async () => false) });
    const dialog = stubLocator({ getByRole: vi.fn(() => cancel) });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ first: () => dialog })) });
    const driver = new WebNewDriver(page);

    await driver.runnersCancelModal();

    await expect(cancel.isDisabled()).resolves.toBe(false);
    expect(cancel.click).toHaveBeenCalledWith({ force: true });
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

  it("returns a single-line breadcrumb whole and treats an empty one as missing", async () => {
    const single = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "  AI Workers  ") });
    await expect(new WebNewDriver(pageStub({ locator: vi.fn(() => single) })).runnersBreadcrumbLeaf()).resolves.toBe(
      "AI Workers"
    );
    const blank = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "\n") });
    await expect(
      new WebNewDriver(pageStub({ locator: vi.fn(() => blank) })).runnersBreadcrumbLeaf()
    ).resolves.toBeNull();
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

    const empty = pageStub({ getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })) });
    await expect(new WebNewDriver(empty).runnersLastToast()).resolves.toBe("");
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

describe("runner chat shell (NEWFRONT-549)", () => {
  it("opens the chat URL with the session cursor and settles on the page", async () => {
    // The session wait polls for the active history row until it appears.
    // The stub reports it on the first read so the open settles at once:
    // with the default count of zero the wait would spin its whole real-time
    // deadline (millions of retained stub calls under immediate stubs) and
    // OOM the worker (NEWFRONT-917).
    const activeRow = stubLocator({ count: vi.fn(async () => 1) });
    const history = stubLocator({ locator: vi.fn(() => activeRow) });
    const page = pageStub({ getByTestId: vi.fn(() => history) });
    const driver = new WebNewDriver(page);
    await driver.runnerChatOpen("acme", "r-1", "s-9");
    expect(page.goto).toHaveBeenCalledWith("/acme/runners/chat/r-1?sessionId=s-9", { timeout: 60_000 });
    await driver.runnerChatOpen("acme", "r-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/runners/chat/r-1", { timeout: 60_000 });
  });

  it("opens an unknown session once the row wait lapses", async () => {
    // Unknown sessions legitimately never see the row; the open still
    // settles once the bounded wait lapses. Time jumps past the deadline
    // so the test neither waits out the real window nor spins against it.
    const page = pageStub();
    const driver = new WebNewDriver(page);
    const start = 1_000_000;
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValueOnce(start)
      .mockReturnValue(start + 15_001);
    try {
      await driver.runnerChatOpen("acme", "r-1", "s-9");
    } finally {
      now.mockRestore();
    }
    expect(page.goto).toHaveBeenCalledWith("/acme/runners/chat/r-1?sessionId=s-9", { timeout: 60_000 });
  });

  it("lists contacts from the rail", async () => {
    const link = stubLocator({ innerText: vi.fn(async () => " alpha ") });
    const page = pageStub({
      getByTestId: vi.fn(() =>
        stubLocator({ getByTestId: vi.fn(() => stubLocator({ all: vi.fn(async () => [link]) })) })
      ),
    });
    await expect(new WebNewDriver(page).runnerChatContactNames()).resolves.toEqual(["alpha"]);
  });

  it("reads the contact dot's state class", async () => {
    const dot = stubLocator({ getAttribute: vi.fn(async () => "text-(--success)") });
    const contact = stubLocator({ locator: vi.fn(() => dot) });
    const page = pageStub({
      getByTestId: vi.fn(() =>
        stubLocator({ getByTestId: vi.fn(() => stubLocator({ filter: vi.fn(() => contact) })) })
      ),
    });
    await expect(new WebNewDriver(page).runnerChatContactDotClass("alpha")).resolves.toBe("text-(--success)");
  });

  it("reads the header name, secondary line and badge", async () => {
    const header = stubLocator({
      getByTestId: vi.fn((id: string) => {
        if (id === "runner-chat-name") return stubLocator({ innerText: vi.fn(async () => " alpha ") });
        if (id === "runner-chat-secondary") return stubLocator({ innerText: vi.fn(async () => " Default ") });
        return stubLocator({ innerText: vi.fn(async () => "online") });
      }),
    });
    const page = pageStub({ getByTestId: vi.fn(() => header) });
    await expect(new WebNewDriver(page).runnerChatHeader()).resolves.toEqual({
      name: "alpha",
      secondary: "Default",
      badge: "online",
    });
  });

  it("treats a missing secondary line as empty", async () => {
    const header = stubLocator({
      getByTestId: vi.fn((id: string) => {
        if (id === "runner-chat-name") return stubLocator({ innerText: vi.fn(async () => "Runner") });
        return stubLocator({
          innerText: vi.fn(async () => {
            throw new Error("detached");
          }),
        });
      }),
    });
    const driver = new WebNewDriver(pageStub({ getByTestId: vi.fn(() => header) }));
    await expect(driver.runnerChatHeader()).resolves.toMatchObject({ secondary: "" });
  });

  it("bounds the optional header reads so an unknown runner cannot hang the test (NEWFRONT-557)", async () => {
    // Real innerText on an absent element waits out the action budget
    // instead of rejecting; the secondary line and the badge are absent
    // while the runner is unknown, so both reads carry a short timeout.
    const secondaryText = vi.fn(async () => {
      throw new Error("detached");
    });
    const badgeText = vi.fn(async () => {
      throw new Error("detached");
    });
    const header = stubLocator({
      getByTestId: vi.fn((id: string) => {
        if (id === "runner-chat-name") return stubLocator({ innerText: vi.fn(async () => "Runner") });
        if (id === "runner-chat-secondary") return stubLocator({ innerText: secondaryText });
        return stubLocator({ innerText: badgeText });
      }),
    });
    const driver = new WebNewDriver(pageStub({ getByTestId: vi.fn(() => header) }));
    await expect(driver.runnerChatHeader()).resolves.toEqual({ name: "Runner", secondary: "", badge: "" });
    expect(secondaryText).toHaveBeenCalledWith({ timeout: expect.any(Number) });
    expect(badgeText).toHaveBeenCalledWith({ timeout: expect.any(Number) });
  });

  it("reports the empty history line", async () => {
    const empty = stubLocator({ isVisible: vi.fn(async () => true) });
    const page = pageStub({
      getByTestId: vi.fn(() => stubLocator({ getByTestId: vi.fn(() => empty) })),
    });
    await expect(new WebNewDriver(page).runnerChatHistoryEmptyVisible()).resolves.toBe(true);
    empty.isVisible.mockRejectedValueOnce(new Error("gone"));
    await expect(new WebNewDriver(page).runnerChatHistoryEmptyVisible()).resolves.toBe(false);
  });

  it("reads history entries with their activity labels and active marker", async () => {
    const entry = stubLocator({
      locator: vi.fn(() => stubLocator({ allInnerTexts: vi.fn(async () => ["Oct 18, 10:00 AM", "1 hour ago"]) })),
      getAttribute: vi.fn(async () => "true"),
    });
    const page = pageStub({
      getByTestId: vi.fn(() => stubLocator({ locator: vi.fn(() => stubLocator({ all: vi.fn(async () => [entry]) })) })),
    });
    await expect(new WebNewDriver(page).runnerChatHistoryItems()).resolves.toEqual([
      { title: "Oct 18, 10:00 AM", subtitle: "1 hour ago", active: true },
    ]);
  });

  it("reports the composer gate reason and the disabled textarea", async () => {
    const reason = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => " Runner offline ") });
    const box = stubLocator({ isDisabled: vi.fn(async () => true) });
    const page = pageStub({
      getByTestId: vi.fn(() =>
        stubLocator({
          getByTestId: vi.fn(() => reason),
        })
      ),
      getByPlaceholder: vi.fn(() => box),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.runnerChatComposerReason()).resolves.toBe("Runner offline");
    await expect(driver.runnerChatTextareaDisabled()).resolves.toBe(true);
  });

  it("reports no reason when chat can proceed", async () => {
    const reason = stubLocator({ count: vi.fn(async () => 0) });
    const page = pageStub({ getByTestId: vi.fn(() => stubLocator({ getByTestId: vi.fn(() => reason) })) });
    await expect(new WebNewDriver(page).runnerChatComposerReason()).resolves.toBeNull();
  });

  it("reports the send control's enabled state", async () => {
    const send = stubLocator({ isEnabled: vi.fn(async () => true) });
    const page = pageStub({ getByTestId: vi.fn(() => stubLocator({ getByRole: vi.fn(() => send) })) });
    await expect(new WebNewDriver(page).runnerChatSendEnabled()).resolves.toBe(true);
    send.isEnabled.mockRejectedValueOnce(new Error("detached"));
    await expect(new WebNewDriver(page).runnerChatSendEnabled()).resolves.toBe(false);
  });

  it("reads the transcript bubbles with their roles", async () => {
    const item = (role: string, text: string) =>
      stubLocator({
        innerText: vi.fn(async () => text),
        locator: vi.fn(() => stubLocator({ getAttribute: vi.fn(async () => role) })),
      });
    const list = stubLocator({
      count: vi.fn(async () => 1),
      getByRole: vi.fn(() => stubLocator({ all: vi.fn(async () => [item("user", "hi"), item("assistant", "hello")]) })),
    });
    const page = pageStub({
      getByTestId: vi.fn(() => stubLocator({ getByRole: vi.fn(() => list) })),
    });
    await expect(new WebNewDriver(page).runnerChatMessageBubbles()).resolves.toEqual([
      { role: "user", text: "hi" },
      { role: "assistant", text: "hello" },
    ]);
  });

  it("reports an empty transcript when no list is rendered", async () => {
    const page = pageStub({
      getByTestId: vi.fn(() => stubLocator({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) })),
    });
    await expect(new WebNewDriver(page).runnerChatMessageBubbles()).resolves.toEqual([]);
  });

  it("reads the newest visible toast", async () => {
    const toast = stubLocator({ innerText: vi.fn(async () => "Chat failed\nRunner is offline\nDismiss") });
    const page = pageStub({
      getByTestId: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), nth: vi.fn(() => toast) })),
    });
    await expect(new WebNewDriver(page).runnerChatLastToast()).resolves.toEqual({
      title: "Chat failed",
      message: "Runner is offline Dismiss",
    });
  });

  it("reports no toast when none is on screen", async () => {
    const page = pageStub({ getByTestId: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(page).runnerChatLastToast()).resolves.toBeNull();
  });

  it("counts chat API calls through the classifier route", async () => {
    let handler: ((route: unknown) => Promise<void>) | undefined;
    const page = pageStub({
      route: vi.fn(async (_pattern: string, next: unknown) => {
        handler = next as typeof handler;
      }),
    });
    const driver = new WebNewDriver(page);
    await driver.runnerChatStartApiSpy();
    expect(handler).toBeDefined();
    const route = (method: string, path: string) => {
      const fallback = vi.fn(async () => undefined);
      return {
        request: () => ({ method: () => method, url: () => `https://api.test${path}` }),
        fallback,
      };
    };
    await handler?.(route("POST", "/api/runners/chat/sessions/s-1/warm/"));
    await handler?.(route("POST", "/api/runners/chat/sessions/"));
    await handler?.(route("GET", "/api/runners/chat/sessions/"));
    await handler?.(route("POST", "/api/runners/chat/sessions/s-1/messages/"));
    expect(await driver.runnerChatApiCounts()).toMatchObject({ warm: 1, sessionCreate: 1, sessionList: 1, send: 1 });
    await driver.runnerChatStopApiSpy();
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/**");
  });

  it("stubs a session stream and reports the requested URLs", async () => {
    let handler: ((route: unknown) => Promise<void>) | undefined;
    const page = pageStub({
      route: vi.fn(async (_pattern: string, next: unknown) => {
        handler = next as typeof handler;
      }),
    });
    const driver = new WebNewDriver(page);
    await driver.runnerChatStubStream("s-1", [
      { seq: 1, kind: "assistant_delta", payload: { params: { delta: "hi" } }, message: "m-1" },
      { seq: 2, kind: "raw-invalid", payload: {} },
    ]);
    // Re-stubbing swaps the frames without adding a second route.
    await driver.runnerChatStubStream("s-1", []);
    expect(page.route).toHaveBeenCalledTimes(1);
    const fulfill = vi.fn(async () => undefined);
    await handler?.({
      request: () => ({ url: () => "https://api.test/api/runners/chat/sessions/s-1/events/?after=0" }),
      fulfill,
    });
    expect(fulfill).toHaveBeenCalledTimes(1);
    await expect(driver.runnerChatStreamRequestUrls("s-1")).resolves.toEqual([
      "https://api.test/api/runners/chat/sessions/s-1/events/?after=0",
    ]);
    await driver.runnerChatClearStreamStub("s-1");
    expect(page.unroute).toHaveBeenCalledWith("**/chat/sessions/s-1/events**");
  });
});

describe("pod methods (NEWFRONT-542, RUN-012–015)", () => {
  it("reads tiles off the labelled filter buttons, tolerating a missing label", async () => {
    const tiles = [
      stubLocator({
        getAttribute: vi.fn(async () => null),
        locator: vi.fn(() =>
          stubLocator({
            getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
          })
        ),
      }),
      stubLocator({
        getAttribute: vi.fn(async () => "Filter runners by pod WEB_beefy"),
        locator: vi.fn(() =>
          stubLocator({
            getByText: vi.fn((text: RegExp | string) =>
              String(text) === "default"
                ? stubLocator({ count: vi.fn(async () => 1) })
                : stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "3 runner(s)") })
            ),
          })
        ),
      }),
      stubLocator({
        getAttribute: vi.fn(async () => "Filter runners by pod WEB_slim"),
        locator: vi.fn(() =>
          stubLocator({
            getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
          })
        ),
      }),
    ];
    const page = pageStub({
      getByRole: vi.fn(() => stubLocator({ locator: vi.fn(() => stubLocator({ all: vi.fn(async () => tiles) })) })),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.runnersPodTiles()).toEqual([
      { name: "", isDefault: false, runnerCount: "" },
      { name: "WEB_beefy", isDefault: true, runnerCount: "3 runner(s)" },
      { name: "WEB_slim", isDefault: false, runnerCount: "" },
    ]);
  });

  it("selects a tile and reads the clear-filter line", async () => {
    const clear = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() =>
        stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Filtering runners by pod WEB_beefy") })
      ),
    });
    const page = pageStub({
      getByRole: vi.fn(() => stubLocator({ getByRole: vi.fn(() => clear) })),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.runnersFilterText()).toBe("Filtering runners by pod WEB_beefy");
    await driver.runnersClearPodFilter();
    expect(clear.click).toHaveBeenCalled();
    expect(clear.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: expect.any(Number) });
  });

  it("reads null when no filter is active", async () => {
    const page = pageStub({
      getByRole: vi.fn(() => stubLocator({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) })),
    });
    expect(await new WebNewDriver(page).runnersFilterText()).toBeNull();
  });

  it("reports the pods failure line or null", async () => {
    const message = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Failed to load pods") });
    const failing = pageStub({ getByRole: vi.fn(() => stubLocator({ getByText: vi.fn(() => message) })) });
    expect(await new WebNewDriver(failing).runnersPodsError()).toBe("Failed to load pods");
    const quiet = pageStub({
      getByRole: vi.fn(() => stubLocator({ getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) })),
    });
    expect(await new WebNewDriver(quiet).runnersPodsError()).toBeNull();
  });

  it("opens the create dialog and reads its form, errors and state", async () => {
    const submit = stubLocator({
      count: vi.fn(async () => 1),
      isDisabled: vi.fn(async () => true),
      isVisible: vi.fn(async () => true),
    });
    const cancel = stubLocator({
      count: vi.fn(async () => 1),
      first: (): unknown => cancel,
      isEnabled: vi.fn(async () => false),
    });
    const picker = stubLocator({ innerText: vi.fn(async () => "Select a project\nChoose") });
    const form = stubLocator({
      locator: vi.fn(() => stubLocator({ first: () => picker })),
      getByRole: vi.fn(() => stubLocator({ first: () => picker })),
    });
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn((selector: string) => {
        if (selector === 'button[type="submit"]') return submit;
        if (selector === 'form [role="alert"]')
          return stubLocator({ allTextContents: vi.fn(async () => ["Pick a project.", " "]) });
        if (selector === "form") return form;
        return stubLocator();
      }),
      getByRole: vi.fn((role: string) => (role === "button" ? cancel : submit)),
      getByLabel: vi.fn((label: string) =>
        stubLocator({ inputValue: vi.fn(async () => (label === "Name" ? "beefy" : "")) })
      ),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog"
          ? stubLocator({
              filter: vi.fn(() => dialog),
              locator: dialog.locator,
              count: dialog.count,
              getByRole: dialog.getByRole,
              getByLabel: dialog.getByLabel,
            })
          : stubLocator()
      ),
    });
    const driver = new WebNewDriver(page);
    await driver.runnersOpenCreatePod();
    expect(await driver.runnersPodFormErrors()).toEqual(["Pick a project."]);
    expect(await driver.runnersCreatePodSubmitting()).toBe(true);
    expect(await driver.runnersPodCancelEnabled()).toBe(false);
    expect(await driver.runnersCreatePodForm()).toEqual({
      project: "Select a project",
      name: "beefy",
      description: "",
    });
    expect(await driver.runnersPodModalOpen()).toBe(true);
  });

  it("reports a closed pod dialog", async () => {
    const page = pageStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.runnersPodModalOpen()).toBe(false);
    expect(await driver.runnersCreatePodSubmitting()).toBe(false);
    expect(await driver.runnersPodCancelEnabled()).toBe(false);
    expect(await driver.runnersPodFormErrors()).toEqual([]);
  });

  it("reads the edit form including the one-way default switch", async () => {
    const toggle = stubLocator({
      getAttribute: vi.fn(async (name: string) => (name === "aria-checked" ? "true" : null)),
      isDisabled: vi.fn(async () => false),
    });
    const dialog = stubLocator({
      getByRole: vi.fn(() => toggle),
      getByLabel: vi.fn((label: string) =>
        stubLocator({ inputValue: vi.fn(async () => (label === "Name" ? "beefy" : "")) })
      ),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
    });
    expect(await new WebNewDriver(page).runnersEditPodForm()).toEqual({
      name: "beefy",
      description: "",
      isDefault: true,
      defaultDisabled: false,
    });
  });

  it("captures the PATCH bodies of an edit", async () => {
    const submit = stubLocator();
    const dialog = stubLocator({
      locator: vi.fn(() => submit),
      getByRole: vi.fn(() =>
        stubLocator({ getAttribute: vi.fn(async () => "false"), click: vi.fn(async () => undefined) })
      ),
      getByLabel: vi.fn(() => stubLocator({ fill: vi.fn(async () => undefined) })),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
      route: vi.fn(async (pattern: string, handler: unknown) => {
        if (pattern !== "**/api/runners/pods/**") return;
        const route = {
          request: () => ({ method: () => "PATCH", postData: () => JSON.stringify({ name: "heavier" }) }),
          continue: vi.fn(async () => undefined),
        };
        await (handler as (route: unknown) => Promise<void>)(route);
      }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.runnersSavePodEditCapturing({ name: "heavier", makeDefault: true })).toEqual([
      { name: "heavier" },
    ]);
    expect(dialog.getByLabel).toHaveBeenCalledWith("Name", { exact: true });
  });

  it("counts no PATCH for an unchanged save", async () => {
    const dialog = stubLocator({
      locator: vi.fn(() => stubLocator()),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
      route: vi.fn(async (_pattern: string, handler: unknown) => {
        const route = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
        await (handler as (route: unknown) => Promise<void>)(route);
      }),
    });
    expect(await new WebNewDriver(page).runnersUnchangedEditSkipsSave()).toBe(true);
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/pods/**");
  });

  it("deletes a pod through its menu and waits for the tile to vanish", async () => {
    const tile = stubLocator();
    const menuItem = stubLocator();
    const confirm = stubLocator({
      getByRole: vi.fn(() => stubLocator({ click: vi.fn(async () => undefined) })),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) => {
        if (role === "main") return stubLocator({ getByRole: vi.fn(() => tile) });
        if (role === "menuitem") return menuItem;
        if (role === "dialog") return stubLocator({ filter: vi.fn(() => confirm) });
        return stubLocator();
      }),
    });
    const driver = new WebNewDriver(page);
    await driver.runnersDeletePod("WEB_beefy");
    expect(menuItem.click).toHaveBeenCalled();
    expect(tile.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: expect.any(Number) });
  });

  it("stops after the failure toast when the delete is refused", async () => {
    const tile = stubLocator();
    const confirm = stubLocator({
      getByRole: vi.fn(() => stubLocator({ click: vi.fn(async () => undefined) })),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) => {
        if (role === "main") return stubLocator({ getByRole: vi.fn(() => tile) });
        if (role === "menuitem") return stubLocator();
        if (role === "dialog") return stubLocator({ filter: vi.fn(() => confirm) });
        if (role === "text") return stubLocator({ count: vi.fn(async () => 0) });
        return stubLocator();
      }),
    });
    const driver = new WebNewDriver(page);
    Object.defineProperty(driver, "runnersLastToast", { value: vi.fn(async () => "Error! refusal") });
    await driver.runnersDeletePod("WEB_beefy", { expectFailure: true });
    expect(tile.waitFor).not.toHaveBeenCalled();
  });
});

describe("pod driver methods (NEWFRONT-542, RUN-012–015)", () => {
  it("clicks the labelled tile for a pod", async () => {
    const tile = stubLocator();
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ getByRole: vi.fn(() => tile) })) });
    const driver = new WebNewDriver(page);

    await driver.runnersSelectPod("WEB_beefy");

    expect(page.getByRole).toHaveBeenCalledWith("main");
    expect(tile.click).toHaveBeenCalled();
  });

  it("reads a bare filter line as null when there is no sibling span", async () => {
    const clear = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ getByRole: vi.fn(() => clear) })) });

    expect(await new WebNewDriver(page).runnersFilterText()).toBeNull();
  });

  it("submits the open pod form", async () => {
    const submit = stubLocator();
    const dialog = stubLocator({ locator: vi.fn(() => submit) });
    const page = pageStub({ getByRole: vi.fn(() => dialog) });

    await new WebNewDriver(page).runnersSubmitPodForm();

    expect(submit.click).toHaveBeenCalled();
  });

  it("reports a submitting form without its submit button as not submitting", async () => {
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const page = pageStub({ getByRole: vi.fn(() => dialog) });

    expect(await new WebNewDriver(page).runnersCreatePodSubmitting()).toBe(false);
  });

  it("reports no cancel button as disabled", async () => {
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const page = pageStub({ getByRole: vi.fn(() => dialog) });

    expect(await new WebNewDriver(page).runnersPodCancelEnabled()).toBe(false);
  });

  it("takes the first non-empty line of the project picker as its label", async () => {
    const labels = ["  Web  ", "\n", ""];
    for (const label of labels) {
      const picker = stubLocator({ innerText: vi.fn(async () => label) });
      const dialog = stubLocator({
        locator: vi.fn(() => stubLocator({ first: () => picker })),
        getByLabel: vi.fn(() => stubLocator({ inputValue: vi.fn(async () => "") })),
      });
      const page = pageStub({
        getByRole: vi.fn((role: string) =>
          role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
        ),
      });

      expect((await new WebNewDriver(page).runnersCreatePodForm()).project).toBe(label.trim());
    }
  });

  it("reports a dialog without a submit button as open while its first action shows", async () => {
    const action = stubLocator({ count: vi.fn(async () => 1), isVisible: vi.fn(async () => true) });
    const dialogs = stubLocator({
      count: vi.fn(async () => 1),
      first: () => dialogs,
      locator: vi.fn((selector: string) =>
        selector === 'button[type="submit"]' ? stubLocator({ count: vi.fn(async () => 0) }) : action
      ),
    });
    const page = pageStub({ getByRole: vi.fn(() => dialogs) });

    expect(await new WebNewDriver(page).runnersPodModalOpen()).toBe(true);
  });

  it("reports a dialog with neither submit nor action as closed", async () => {
    const dialogs = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const page = pageStub({ getByRole: vi.fn(() => dialogs) });

    expect(await new WebNewDriver(page).runnersPodModalOpen()).toBe(false);
  });

  it("creates a pod through the picker and the form", async () => {
    const option = stubLocator();
    const name = stubLocator({ fill: vi.fn(async () => undefined) });
    const description = stubLocator({ fill: vi.fn(async () => undefined) });
    const submit = stubLocator();
    const dialog = stubLocator({
      getByRole: vi.fn((role: string) => (role === "button" ? stubLocator() : option)),
      getByLabel: vi.fn((label: string) => (label === "Name" ? name : description)),
      locator: vi.fn(() => submit),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) => {
        if (role === "dialog") return stubLocator({ filter: vi.fn(() => dialog) });
        if (role === "option") return option;
        return stubLocator();
      }),
    });

    await new WebNewDriver(page).runnersCreatePod({
      projectName: "Web",
      name: "beefy",
      description: "the big boxes",
    });

    expect(name.fill).toHaveBeenCalledWith("beefy");
    expect(description.fill).toHaveBeenCalledWith("the big boxes");
    expect(submit.click).toHaveBeenCalled();
    // The closed wait keys on the dialog itself (NEWFRONT-557): the delete
    // confirm has no form submit, and the open modal hides the tiles.
    expect(dialog.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: expect.any(Number) });
  });

  it("leaves the description alone and waits for the toast on a refused create", async () => {
    const name = stubLocator();
    const description = stubLocator();
    const dialog = stubLocator({
      getByRole: vi.fn(() => stubLocator()),
      getByLabel: vi.fn((label: string) => (label === "Name" ? name : description)),
      locator: vi.fn(() => stubLocator()),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
    });
    const driver = new WebNewDriver(page);
    Object.defineProperty(driver, "runnersLastToast", { value: vi.fn(async () => "Error! duplicate") });

    await driver.runnersCreatePod({ projectName: "Web", name: "beefy" }, { expectFailure: true });

    expect(description.fill).not.toHaveBeenCalled();
    expect(dialog.locator('button[type="submit"]').waitFor).not.toHaveBeenCalled();
  });

  it("selects a pod twice and counts no runner fetches as a client-side filter", async () => {
    let handler: ((route: unknown) => Promise<void>) | undefined;
    const page = pageStub({
      route: vi.fn(async (pattern: string, h: unknown) => {
        if (pattern === "**/api/runners/?*") handler = h as (route: unknown) => Promise<void>;
      }),
    });
    const select = vi.fn(async () => undefined);
    Object.defineProperty(page, "waitForTimeout", { value: vi.fn(async () => undefined) });
    const driver = new WebNewDriver(page);
    const spy = vi.spyOn(driver, "runnersSelectPod").mockImplementation(select);

    expect(await driver.runnersPodFilterIsClientSide("WEB_beefy")).toBe(true);
    expect(select).toHaveBeenCalledTimes(2);
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/?*", handler);

    const get = stubRoute("GET");
    await handler!(get);
    expect(get.continue).toHaveBeenCalled();
    const post = stubRoute("POST");
    await handler!(post);
    expect(post.continue).toHaveBeenCalled();
    void spy;
  });

  it("counts a runner GET during selection and through the route as server-side", async () => {
    let handler: ((route: unknown) => Promise<void>) | undefined;
    const page = pageStub({
      route: vi.fn(async (_pattern: string, h: unknown) => {
        handler = h as (route: unknown) => Promise<void>;
      }),
    });
    Object.defineProperty(page, "waitForTimeout", { value: vi.fn(async () => undefined) });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "runnersSelectPod").mockImplementation(async () => {
      if (handler !== undefined) await handler(stubRoute("GET"));
    });

    expect(await driver.runnersPodFilterIsClientSide("WEB_beefy")).toBe(false);
  });

  it("opens a pod's edit dialog from its menu", async () => {
    const tile = stubLocator();
    const edit = stubLocator();
    const page = pageStub({
      getByRole: vi.fn((role: string) => {
        if (role === "main") return stubLocator({ getByRole: vi.fn(() => tile) });
        if (role === "menuitem") return edit;
        return stubLocator();
      }),
    });

    await new WebNewDriver(page).runnersOpenEditPod("WEB_beefy");

    expect(edit.click).toHaveBeenCalled();
  });

  it("fills only the fields an edit passes", async () => {
    const name = stubLocator();
    const description = stubLocator();
    const toggle = stubLocator();
    const submit = stubLocator();
    const dialog = stubLocator({
      getByLabel: vi.fn((label: string) => (label === "Name" ? name : description)),
      getByRole: vi.fn(() => toggle),
      locator: vi.fn(() => submit),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
    });

    await new WebNewDriver(page).runnersSavePodEdit({ description: "moved" });

    expect(name.fill).not.toHaveBeenCalled();
    expect(description.fill).toHaveBeenCalledWith("moved");
    expect(toggle.click).not.toHaveBeenCalled();
    expect(submit.click).toHaveBeenCalled();
  });

  it("skips the default toggle when the switch already reads on", async () => {
    const toggle = stubLocator({ getAttribute: vi.fn(async () => "true") });
    const dialog = stubLocator({
      getByLabel: vi.fn(() => stubLocator()),
      getByRole: vi.fn(() => toggle),
      locator: vi.fn(() => stubLocator()),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
    });

    await new WebNewDriver(page).runnersSavePodEdit({ makeDefault: true });

    expect(toggle.click).not.toHaveBeenCalled();
  });

  it("counts a PATCH during an unchanged save and still releases the route", async () => {
    const dialog = stubLocator({ locator: vi.fn(() => stubLocator()) });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
      route: vi.fn(async (_pattern: string, handler: unknown) => {
        await (handler as (route: unknown) => Promise<void>)(stubRoute("PATCH"));
      }),
    });

    expect(await new WebNewDriver(page).runnersUnchangedEditSkipsSave()).toBe(false);
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/pods/**");
  });

  it("captures a PATCH without a body as empty", async () => {
    const dialog = stubLocator({ locator: vi.fn(() => stubLocator()) });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
      route: vi.fn(async (_pattern: string, handler: unknown) => {
        const route = {
          request: () => ({ method: () => "PATCH", postData: () => null }),
          continue: vi.fn(async () => undefined),
        };
        await (handler as (route: unknown) => Promise<void>)(route);
      }),
    });

    expect(await new WebNewDriver(page).runnersSavePodEditCapturing({})).toEqual([{}]);
  });

  it("captures an unreadable PATCH body as empty", async () => {
    const dialog = stubLocator({
      getByLabel: vi.fn(() => stubLocator()),
      getByRole: vi.fn(() => stubLocator({ getAttribute: vi.fn(async () => "false") })),
      locator: vi.fn(() => stubLocator()),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) =>
        role === "dialog" ? stubLocator({ filter: vi.fn(() => dialog) }) : stubLocator()
      ),
      route: vi.fn(async (_pattern: string, handler: unknown) => {
        const route = {
          request: () => ({
            method: () => "PATCH",
            postData: () => {
              throw new Error("no body");
            },
          }),
          continue: vi.fn(async () => undefined),
        };
        await (handler as (route: unknown) => Promise<void>)(route);
      }),
    });

    expect(await new WebNewDriver(page).runnersSavePodEditCapturing({})).toEqual([{}]);
  });
});

// --- Runs list and detail (NEWFRONT-545, RUN-016–022). These exercise the
// --- new-app DOM contract: runs test ids, the runs API URLs, and pager state.

function testIdPage(ids: Record<string, Locator>, overrides: Record<string, unknown> = {}): Page {
  return pageStub({
    getByTestId: vi.fn((id: string) => ids[id] ?? stubLocator()),
    ...overrides,
  });
}

function textNode(text: string, overrides: Record<string, unknown> = {}) {
  return stubLocator({
    count: vi.fn(async () => 1),
    innerText: vi.fn(async () => text),
    isVisible: vi.fn(async () => true),
    ...overrides,
  });
}

describe("runs list navigation", () => {
  it("opens workspace and project runs with the idempotent list waiter", async () => {
    const table = textNode("", { getByTestId: vi.fn(() => textNode("No runs yet.")) });
    const settled = stubLocator();
    const page = testIdPage({ "runs-table": table }, { locator: vi.fn(() => settled) });
    const driver = new WebNewDriver(page);

    await driver.openRunsList("acme");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/runners/runs", { timeout: 60_000 });
    await driver.openRunsListAtPage("acme", 2, "p-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/projects/p-1/runners/runs?page=2", { timeout: 60_000 });
    await driver.openRunDetail("acme", "run-1", "p-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/projects/p-1/runners/runs/run-1", { timeout: 60_000 });
    expect(await driver.runsHistoryLength()).toBe(0);
    expect((page.evaluate as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![0]).toBeTypeOf("function");
    expect(page.locator).toHaveBeenCalledWith('[data-testid="runs-row"], [data-testid="runs-empty"]');
    expect(settled.waitFor).toHaveBeenCalled();
  });

  it("does not settle on the pending spinner row", async () => {
    const table = textNode("");
    const pending = stubLocator({
      first: () => pending,
      waitFor: vi.fn(async () => {
        throw new Error("timeout waiting for runs content");
      }),
    });
    const page = testIdPage({ "runs-table": table }, { locator: vi.fn(() => pending) });
    const driver = new WebNewDriver(page);

    await expect(driver.openRunsList("acme")).rejects.toThrow("timeout waiting for runs content");
    expect(page.locator).toHaveBeenCalledWith('[data-testid="runs-row"], [data-testid="runs-empty"]');
    expect(page.locator).not.toHaveBeenCalledWith('[data-testid="runs-row"], [data-testid="runs-list"] tbody tr');
  });
});

describe("runsRows, runsPager, runsEmptyVisible", () => {
  it("reads status and prompt from each row and skips the empty-state cell", async () => {
    const rows = [
      stubLocator({
        getByTestId: vi.fn((id: string) =>
          id === "runs-row-status" ? textNode("running") : textNode("parity prompt alpha")
        ),
      }),
      stubLocator({
        getByTestId: vi.fn((id: string) =>
          id === "runs-row-status" ? textNode("cancelled") : textNode("parity prompt beta")
        ),
      }),
    ];
    const table = stubLocator({ getByTestId: vi.fn(() => stubLocator({ all: vi.fn(async () => rows) })) });
    const driver = new WebNewDriver(testIdPage({ "runs-table": table }));

    await expect(driver.runsRows()).resolves.toEqual([
      { statusLabel: "running", prompt: "parity prompt alpha" },
      { statusLabel: "cancelled", prompt: "parity prompt beta" },
    ]);
  });

  it("reads the pager label and both disabled ends", async () => {
    const buttons = stubLocator({
      count: vi.fn(async () => 2),
      first: () => buttons,
      nth: (index: number) => stubLocator({ isDisabled: vi.fn(async () => (index === 0 ? false : true)) }),
    });
    const pager = stubLocator({
      count: vi.fn(async () => 1),
      getByTestId: vi.fn(() => textNode("Page 2 of 2")),
      getByRole: vi.fn(() => buttons),
    });
    const driver = new WebNewDriver(testIdPage({ "runs-pager": pager }));

    await expect(driver.runsPager()).resolves.toEqual({
      label: "Page 2 of 2",
      prevDisabled: false,
      nextDisabled: true,
    });
  });

  it("hides the pager when it is not rendered and reports an empty list only without rows", async () => {
    const list = stubLocator({ getByText: vi.fn(() => textNode("No runs yet.")) });
    const noPager = new WebNewDriver(
      testIdPage({ "runs-list": list, "runs-row": stubLocator({ count: vi.fn(async () => 0) }) })
    );
    await expect(noPager.runsPager()).resolves.toBeNull();
    await expect(noPager.runsEmptyVisible()).resolves.toBe(true);

    const withRow = new WebNewDriver(
      testIdPage({ "runs-list": list, "runs-row": stubLocator({ count: vi.fn(async () => 1) }) })
    );
    await expect(withRow.runsEmptyVisible()).resolves.toBe(false);
  });

  it("turns the pager and waits for the visible page label to change", async () => {
    let label = "Page 2 of 2";
    const pageLabelNode = stubLocator({ innerText: vi.fn(async () => label) });
    const next = stubLocator({
      click: vi.fn(async () => {
        label = "Page 1 of 2";
      }),
    });
    const pager = stubLocator({
      getByTestId: vi.fn(() => pageLabelNode),
      getByRole: vi.fn(() => next),
    });
    const table = stubLocator({ getByTestId: vi.fn(() => stubLocator({ all: vi.fn(async () => []) })) });
    const page = testIdPage({ "runs-pager": pager, "runs-table": table }, { locator: vi.fn(() => stubLocator()) });
    const driver = new WebNewDriver(page);

    await driver.runsPrevPage();
    expect(next.click).toHaveBeenCalled();

    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(0);
    const stuckPage = testIdPage(
      { "runs-pager": pager },
      {
        waitForTimeout: vi.fn(async () => {
          vi.setSystemTime(10_000_000);
        }),
      }
    );
    const stuck = new WebNewDriver(stuckPage);
    await expect(stuck.runsPrevPage()).rejects.toThrow(/did not turn prev/);
    vi.useRealTimers();
  });

  it("selects the row whose prompt contains the marker", async () => {
    const first = stubLocator({
      getByTestId: vi.fn((id: string) => textNode(id === "runs-row-prompt" ? "other" : "queued")),
    });
    const second = stubLocator({
      getByTestId: vi.fn((id: string) => textNode(id === "runs-row-prompt" ? "wanted marker" : "running")),
      click: vi.fn(async () => undefined),
    });
    const table = stubLocator({ getByTestId: vi.fn(() => stubLocator({ all: vi.fn(async () => [first, second]) })) });
    const page = testIdPage({ "runs-table": table, "run-detail": textNode("detail", { count: vi.fn(async () => 1) }) });
    const driver = new WebNewDriver(page);

    await driver.runsSelectRun("wanted");
    expect(second.click).toHaveBeenCalled();
    await expect(driver.runsSelectRun("missing")).rejects.toThrow(/run row/);
  });
});

describe("run detail reads", () => {
  function detailPage() {
    const ids: Record<string, Locator> = {
      "run-detail": textNode("detail"),
      "run-detail-id": textNode("run-1"),
      "run-status": textNode("running"),
      "run-executor": textNode("Local runner"),
      "run-prompt": textNode("full prompt"),
      "run-scheduler-link": textNode("Nightly", { getAttribute: vi.fn(async () => "/acme/projects/p/schedulers/b") }),
      "run-error": textNode("raw failure"),
      "run-error-source": textNode("Agent CLI"),
      "run-error-kind": textNode("Sign-in failed"),
      "run-error-summary": textNode("the summary"),
      "run-error-action": textNode("Re-authenticate"),
      "run-result": textNode('{"answer":42}'),
      "run-result-summary": textNode("cloud summary"),
      "run-cloud-panel": textNode("cloud"),
      "run-cloud-tool": stubLocator({ all: vi.fn(async () => [textNode("search"), textNode("fetch")]) }),
      "run-cloud-call": stubLocator({
        all: vi.fn(async () => [
          stubLocator({
            getByTestId: vi.fn((id: string) =>
              id === "run-cloud-call-name" ? textNode("search") : textNode("read · succeeded")
            ),
          }),
        ]),
      }),
      "run-events": stubLocator({
        locator: vi.fn(() =>
          stubLocator({
            all: vi.fn(async () => [
              stubLocator({
                getAttribute: vi.fn(async () => "run-event-row"),
                locator: vi.fn(() => stubLocator({ nth: (i: number) => textNode(i === 0 ? "1" : "agent/message") })),
              }),
              stubLocator({
                getAttribute: vi.fn(async () => "run-event-narrative-row"),
                getByTestId: vi.fn(() => textNode("hello")),
              }),
              stubLocator({
                getAttribute: vi.fn(async () => "run-event-row"),
                locator: vi.fn(() => stubLocator({ nth: (i: number) => textNode(i === 0 ? "2" : "run/started") })),
              }),
            ]),
          })
        ),
      }),
      "run-cancel": textNode("Cancel run"),
    };
    return testIdPage(ids);
  }

  it("reports loaded state and every header, prompt and scheduler field", async () => {
    const driver = new WebNewDriver(detailPage());
    await expect(driver.runDetailState()).resolves.toBe("loaded");
    await expect(driver.runDetailHeader()).resolves.toEqual({
      id: "run-1",
      statusLabel: "running",
      executor: "Local runner",
    });
    await expect(driver.runDetailPrompt()).resolves.toBe("full prompt");
    await expect(driver.runDetailScheduler()).resolves.toEqual({
      name: "Nightly",
      href: "/acme/projects/p/schedulers/b",
    });
  });

  it("reads the error diagnostic, result summary and cloud panel", async () => {
    const driver = new WebNewDriver(detailPage());
    await expect(driver.runDetailError()).resolves.toEqual({
      raw: "raw failure",
      source: "Agent CLI",
      kind: "Sign-in failed",
      summary: "the summary",
      action: "Re-authenticate",
    });
    await expect(driver.runDetailResult()).resolves.toEqual({ summary: "cloud summary", raw: '{"answer":42}' });
    await expect(driver.runCloudPanel()).resolves.toEqual({
      tools: ["search", "fetch"],
      calls: [{ tool: "search", risk: "read", status: "succeeded" }],
    });
    await expect(driver.runEvents()).resolves.toEqual([
      { seq: "1", kind: "agent/message", narrative: "hello" },
      { seq: "2", kind: "run/started", narrative: null },
    ]);
  });

  it("returns null blocks when the run does not carry them", async () => {
    const driver = new WebNewDriver(testIdPage({}));
    await expect(driver.runDetailHeader()).resolves.toBeNull();
    await expect(driver.runDetailPrompt()).resolves.toBeNull();
    await expect(driver.runDetailScheduler()).resolves.toBeNull();
    await expect(driver.runDetailError()).resolves.toBeNull();
    await expect(driver.runDetailResult()).resolves.toBeNull();
    await expect(driver.runCloudPanel()).resolves.toBeNull();
  });

  it("distinguishes none, loading and unavailable panes", async () => {
    await expect(new WebNewDriver(testIdPage({ "run-detail-none": textNode("none") })).runDetailState()).resolves.toBe(
      "none"
    );
    await expect(
      new WebNewDriver(testIdPage({ "run-detail-loading": textNode("loading") })).runDetailState()
    ).resolves.toBe("loading");
    await expect(
      new WebNewDriver(testIdPage({ "run-detail-unavailable": textNode("gone") })).runDetailState()
    ).resolves.toBe("unavailable");
  });
});

describe("run detail loading and polling", () => {
  it("reports the loading pane while a delayed detail fetch is in flight", async () => {
    const page = testIdPage({
      "run-detail-loading": stubLocator({ count: vi.fn(async () => 1) }),
      "run-detail": stubLocator({ count: vi.fn(async () => 0) }),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.runDetailLoadingObserved("acme", "run-1")).resolves.toBe(true);
    expect(page.route).toHaveBeenCalledWith("**/api/runners/runs/run-1/**", expect.any(Function));
  });

  it("counts only matching GET requests for list and detail polling", async () => {
    let handler: ((request: { url(): string; method(): string }) => void) | null = null;
    let requests: string[] = [];
    const page = pageStub({
      on: vi.fn((_event: string, fn: typeof handler) => {
        handler = fn;
      }),
      off: vi.fn(),
      waitForTimeout: vi.fn(async () => {
        for (const url of requests) handler?.({ url: () => url, method: () => "GET" });
        handler?.({ url: () => "https://api.test/api/runners/runs/run-1/", method: () => "POST" });
      }),
    });
    const driver = new WebNewDriver(page);

    requests = ["https://api.test/api/runners/runs/?page=1", "https://api.test/api/runners/runs/run-1/"];
    await expect(driver.runsListPollCount(10)).resolves.toBe(1);
    await expect(driver.runDetailPollCount("run-1", 10)).resolves.toBe(1);
    expect(page.off).toHaveBeenCalled();
  });
});

describe("run canceling", () => {
  it("opens, reads and confirms the cancel dialog", async () => {
    const confirm = stubLocator({ click: vi.fn(async () => undefined) });
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      getByRole: vi.fn(() => stubLocator({ first: () => textNode("Cancel run?") })),
      getByTestId: vi.fn((id: string) => (id === "run-cancel-confirm" ? confirm : textNode("stop copy"))),
      waitFor: vi.fn(async () => undefined),
    });
    const cancel = textNode("Cancel run", { click: vi.fn(async () => undefined) });
    const page = testIdPage({ "run-cancel": cancel }, { getByRole: vi.fn(() => dialog) });
    const driver = new WebNewDriver(page);

    await expect(driver.runCancelAvailable()).resolves.toBe(true);
    await driver.runCancelOpen();
    await expect(driver.runCancelDialog()).resolves.toEqual({ title: "Cancel run?", body: "stop copy" });
    await driver.runCancelConfirm();
    expect(confirm.click).toHaveBeenCalled();
  });
});

function approvalCardStub(fields: {
  header: string;
  kind: string;
  reason?: string | null;
  expiry?: string | null;
  payload: string;
}) {
  return stubLocator({
    getByTestId: vi.fn((id: string) => {
      if (id === "approval-card-header") return textNode(fields.header);
      if (id === "approval-card-kind") return textNode(fields.kind);
      if (id === "approval-card-reason") {
        return fields.reason === undefined || fields.reason === null
          ? stubLocator({ count: vi.fn(async () => 0) })
          : textNode(fields.reason);
      }
      if (id === "approval-card-expiry") {
        return fields.expiry === undefined || fields.expiry === null
          ? stubLocator({ count: vi.fn(async () => 0) })
          : textNode(fields.expiry);
      }
      return textNode(fields.payload);
    }),
  });
}

describe("approvals queue (NEWFRONT-547, RUN-023/RUN-024)", () => {
  it("opens the queue in both scopes and settles on cards or the empty marker", async () => {
    const list = textNode("");
    const settled = stubLocator();
    const page = testIdPage({ "approvals-list": list }, { locator: vi.fn(() => settled) });
    const driver = new WebNewDriver(page);

    await driver.openApprovals("acme");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/runners/approvals", { timeout: 60_000 });
    await driver.openApprovals("acme", "p-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/projects/p-1/runners/approvals", { timeout: 60_000 });
    expect(settled.waitFor).toHaveBeenCalled();
  });

  it("reads header, kind, reason, expiry and payload from each card", async () => {
    const cards = [
      approvalCardStub({
        header: "Run run-9 · asked 10:00:00",
        kind: "Run a shell command",
        reason: "listing files",
        expiry: "Ends 11:00:00",
        payload: '{"command":"ls"}',
      }),
      approvalCardStub({ header: "Run run-8", kind: "Change a file", payload: '{"path":"/tmp/x"}' }),
    ];
    const collection = stubLocator({ all: vi.fn(async () => cards) });
    const driver = new WebNewDriver(testIdPage({ "approval-card": collection }));

    await expect(driver.approvalsCards()).resolves.toEqual([
      {
        header: "Run run-9 · asked 10:00:00",
        kindLabel: "Run a shell command",
        reason: "listing files",
        expiry: "Ends 11:00:00",
        payload: '{"command":"ls"}',
      },
      { header: "Run run-8", kindLabel: "Change a file", reason: null, expiry: null, payload: '{"path":"/tmp/x"}' },
    ]);
  });

  it("reads blank reason and expiry as absent and survives unreadable text", async () => {
    const blank = approvalCardStub({ header: "h", kind: "k", reason: "  ", expiry: "", payload: "p" });
    const flaky = stubLocator({
      getByTestId: vi.fn(() => stubLocator({ innerText: vi.fn(async () => "") })),
    });
    const collection = stubLocator({ all: vi.fn(async () => [blank, flaky]) });
    const driver = new WebNewDriver(testIdPage({ "approval-card": collection }));

    await expect(driver.approvalsCards()).resolves.toEqual([
      { header: "h", kindLabel: "k", reason: null, expiry: null, payload: "p" },
      { header: "", kindLabel: "", reason: null, expiry: null, payload: "" },
    ]);
  });

  it("reports the empty state only without cards and a visible marker", async () => {
    const list = stubLocator();
    const visible = new WebNewDriver(
      testIdPage({
        "approvals-list": list,
        "approval-card": stubLocator({ count: vi.fn(async () => 0) }),
        "approvals-empty": textNode("Nothing waiting for review."),
      })
    );
    await expect(visible.approvalsEmptyVisible()).resolves.toBe(true);

    const withCard = new WebNewDriver(
      testIdPage({
        "approvals-list": list,
        "approval-card": stubLocator({ count: vi.fn(async () => 1) }),
        "approvals-empty": textNode("Nothing waiting for review."),
      })
    );
    await expect(withCard.approvalsEmptyVisible()).resolves.toBe(false);

    const noMarker = new WebNewDriver(
      testIdPage({
        "approvals-list": list,
        "approval-card": stubLocator({ count: vi.fn(async () => 0) }),
        "approvals-empty": stubLocator({ count: vi.fn(async () => 0) }),
      })
    );
    await expect(noMarker.approvalsEmptyVisible()).resolves.toBe(false);

    const hidden = new WebNewDriver(
      testIdPage({
        "approvals-list": list,
        "approval-card": stubLocator({ count: vi.fn(async () => 0) }),
        "approvals-empty": textNode("Nothing waiting for review.", { isVisible: vi.fn(async () => false) }),
      })
    );
    await expect(hidden.approvalsEmptyVisible()).resolves.toBe(false);
  });

  it.each([
    ["accept", "Allow once"],
    ["accept_for_session", "Allow for session"],
    ["decline", "Deny"],
  ] as const)("decides %s on the matching card and waits for it to drop", async (decision, name) => {
    const click = vi.fn(async () => undefined);
    const target = stubLocator({
      getByRole: vi.fn(() => stubLocator({ click })),
      waitFor: vi.fn(async () => undefined),
    });
    const filtered = stubLocator({ first: () => target });
    const filter = vi.fn(() => filtered);
    const page = testIdPage({ "approval-card": stubLocator({ filter }) });
    const driver = new WebNewDriver(page);

    await driver.approvalsDecide("parity-marker", decision);
    expect(filter).toHaveBeenCalledWith({ hasText: "parity-marker" });
    expect(target.getByRole).toHaveBeenCalledWith("button", { name });
    expect(click).toHaveBeenCalled();
    expect(target.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 15_000 });
    expect(page.waitForTimeout).not.toHaveBeenCalled();
  });

  it("treats a staying card as data after the detach window", async () => {
    const target = stubLocator({
      getByRole: vi.fn(() => stubLocator({ click: vi.fn(async () => undefined) })),
      waitFor: vi.fn(async () => {
        throw new Error("still attached");
      }),
    });
    const filtered = stubLocator({ first: () => target });
    const page = testIdPage({ "approval-card": stubLocator({ filter: vi.fn(() => filtered) }) });
    const driver = new WebNewDriver(page);

    await driver.approvalsDecide("parity-marker", "decline");
    expect(page.waitForTimeout).toHaveBeenCalledWith(2_000);
  });
});

describe("dev machines and runner detail (NEWFRONT-555, RUN-037–043)", () => {
  it("reads machine rows and state text from the table", async () => {
    const table = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => [
        {
          name: "build box",
          subline: "host / id",
          status: "Active",
          runners: "1 active / 1 total",
          lastSeen: "Jan 1",
          lastHeartbeat: "Jan 2",
          actions: ["Rotate", "Revoke", "Delete"],
        },
      ]),
      locator: vi.fn(() => stubLocator({ innerText: vi.fn(async () => "No dev machines registered") })),
    });
    const page = pageStub({ getByTestId: vi.fn(() => table) });
    const driver = new WebNewDriver(page);

    await expect(driver.devMachinesRows()).resolves.toEqual([
      {
        name: "build box",
        subline: "host / id",
        status: "Active",
        runners: "1 active / 1 total",
        lastSeen: "Jan 1",
        lastHeartbeat: "Jan 2",
        actions: ["Rotate", "Revoke", "Delete"],
      },
    ]);
    await expect(driver.devMachinesRowByName("build box")).resolves.toMatchObject({ status: "Active" });
    await expect(driver.devMachinesRowByName("missing")).resolves.toBeNull();
    await expect(driver.devMachinesEmptyVisible()).resolves.toBe(true);
    await expect(driver.devMachinesLoadingVisible()).resolves.toBe(false);
    await expect(driver.devMachinesErrorVisible()).resolves.toBe(false);
  });

  it("reports absent state text without a table", async () => {
    const driver = new WebNewDriver(
      pageStub({ getByTestId: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) })
    );
    await expect(driver.devMachinesRows()).resolves.toEqual([]);
    await expect(driver.devMachinesEmptyVisible()).resolves.toBe(false);
  });

  it("opens the machines page and waits for the table", async () => {
    const table = stubLocator();
    const page = pageStub({ getByTestId: vi.fn(() => table) });
    await new WebNewDriver(page).devMachinesOpen("acme");
    expect(page.goto).toHaveBeenCalledWith("/acme/ai-dev-machines");
    expect(table.waitFor).toHaveBeenCalled();
  });

  it("reads the kit dialog title and description from h2 and p", async () => {
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn((selector: string) =>
        selector === "h2"
          ? stubLocator({ first: () => stubLocator({ innerText: vi.fn(async () => "Rotate token?") }) })
          : stubLocator({ first: () => stubLocator({ innerText: vi.fn(async () => "The old token stops working.") }) })
      ),
      getByRole: vi.fn(() =>
        stubLocator({
          count: vi.fn(async () => 2),
          last: () => stubLocator({ innerText: vi.fn(async () => "Rotate") }),
        })
      ),
    });
    const page = pageStub({
      locator: vi.fn(() => stubLocator({ filter: vi.fn(() => dialog) })),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.devMachinesModal()).resolves.toEqual({
      title: "Rotate token?",
      body: "The old token stops working.",
      confirmLabel: "Rotate",
    });
    await expect(driver.devMachinesModalVisible()).resolves.toBe(true);
  });

  it("returns null without an open dialog or without buttons", async () => {
    const driver = new WebNewDriver(
      pageStub({
        locator: vi.fn(() => stubLocator({ filter: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) })),
      })
    );
    await expect(driver.devMachinesModal()).resolves.toBeNull();
    await expect(driver.devMachinesModalVisible()).resolves.toBe(false);
  });

  it("opens each row action and confirms or cancels the dialog", async () => {
    const clicked: string[] = [];
    const action = (label: string) =>
      stubLocator({
        count: vi.fn(async () => 1),
        click: vi.fn(async () => {
          clicked.push(label);
        }),
      });
    const row = stubLocator({
      getByRole: vi.fn((_role: string, options?: { name?: string }) => action(options?.name ?? "")),
    });
    const table = stubLocator({ locator: vi.fn(() => stubLocator({ filter: vi.fn(() => row) })) });
    const dialog = stubLocator({
      count: vi.fn(async () => 1),
      getByRole: vi.fn(() =>
        stubLocator({ last: () => action("confirm"), first: () => action("Cancel"), count: vi.fn(async () => 1) })
      ),
    });
    const page = pageStub({
      getByTestId: vi.fn(() => table),
      getByText: vi.fn(() => stubLocator()),
      locator: vi.fn(() => stubLocator({ filter: vi.fn(() => dialog) })),
      keyboard: { press: vi.fn(async () => undefined) } as unknown as Page["keyboard"],
    });
    const driver = new WebNewDriver(page);

    await driver.devMachinesOpenRotate("build box");
    await driver.devMachinesOpenRevoke("build box");
    await driver.devMachinesOpenDelete("build box");
    expect(clicked).toEqual(["Rotate", "Revoke", "Delete"]);

    await driver.devMachinesModalConfirm();
    expect(clicked).toContain("confirm");
    await driver.devMachinesModalCancel();
    expect(clicked).toContain("Cancel");
    await driver.devMachinesModalPressEscape();
    expect(page.keyboard.press).toHaveBeenCalledWith("Escape");
  });

  it("forces the modal cancel past aria-disabled: the app guard decides (NEWFRONT-557)", async () => {
    // In-flight dismissal checks click Cancel and expect the modal to
    // stay. The modal's Cancel carries aria-disabled while pending
    // (inert, never unmounted); Playwright treats that as unactionable,
    // so the driver forces the click and the app's own guard ignores it.
    const click = vi.fn(async () => undefined);
    const cancel = stubLocator({ click });
    const dialog = stubLocator({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), first: () => cancel })),
    });
    const page = pageStub({ locator: vi.fn(() => stubLocator({ filter: vi.fn(() => dialog) })) });
    await new WebNewDriver(page).devMachinesModalCancel();
    expect(click).toHaveBeenCalledWith({ force: true });
  });

  it("stubs, delays, counts and fails machine reads", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.devMachinesStubListOnce([{ id: "m-1" }]);
    let handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;
    const get = stubRoute("GET");
    await handler(get as unknown as Route);
    expect(get.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 200 }));
    const second = stubRoute("GET");
    await handler(second as unknown as Route);
    expect(second.continue).toHaveBeenCalledTimes(1);

    await driver.devMachinesFailListOnce();
    handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[1]![1] as (
      route: Route
    ) => Promise<void>;
    const fail = stubRoute("GET");
    await handler(fail as unknown as Route);
    expect(fail.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 500 }));

    await driver.devMachinesDelayListOnce(5);
    handler = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[2]![1] as (
      route: Route
    ) => Promise<void>;
    await handler(stubRoute("GET") as unknown as Route);
    expect(page.waitForTimeout).toHaveBeenCalledWith(5);

    await expect(driver.devMachinesListPollCount(0)).resolves.toBe(0);
    expect(page.unroute).toHaveBeenCalled();
  });

  it("delays and fails one action, reads the toast, and spies deletes", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.devMachinesDelayActionOnce(10);
    const delay = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;
    const rotate = stubRoute("POST");
    (rotate.request as unknown as () => unknown) = () => ({
      method: () => "POST",
      url: () => "https://api.test/api/runners/dev-machines/rotate/x",
    });
    await delay(rotate as unknown as Route);
    expect(page.waitForTimeout).toHaveBeenCalledWith(10);

    await driver.devMachinesFailActionOnce();
    const fail = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[1]![1] as (
      route: Route
    ) => Promise<void>;
    const del = stubRoute("DELETE");
    (del.request as unknown as () => unknown) = () => ({
      method: () => "DELETE",
      url: () => "https://api.test/api/runners/dev-machines/x",
    });
    await fail(del as unknown as Route);
    expect(del.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 500 }));

    await expect(driver.devMachinesLastToast()).resolves.toBe(null);

    await driver.devMachinesDeleteSpyStart();
    const spy = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[2]![1] as (
      route: Route
    ) => Promise<void>;
    const del2 = stubRoute("DELETE");
    (del2.request as unknown as () => unknown) = () => ({
      method: () => "DELETE",
      url: () => "https://api.test/api/runners/dev-machines/x",
    });
    await spy(del2 as unknown as Route);
    await expect(driver.devMachinesDeleteSpyUrls()).resolves.toEqual(["https://api.test/api/runners/dev-machines/x"]);
    await driver.devMachinesDeleteSpyStop();
  });

  it("reads install cards, prereq, copy state and clipboard helpers", async () => {
    const label = stubLocator({ count: vi.fn(async () => 1), textContent: vi.fn(async () => "Linux") });
    const card = stubLocator({
      locator: vi.fn(() => stubLocator({ first: () => label })),
      getByRole: vi.fn(() =>
        stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Copy command") })
      ),
    });
    const pre = stubLocator({
      count: vi.fn(async () => 1),
      nth: () => pre,
      locator: vi.fn(() => card),
    });
    const section = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => [{ label: "Linux", command: "curl ...", downloadHref: null }]),
      locator: vi.fn((selector: string) =>
        selector === "pre"
          ? pre
          : stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Install doctor first") })
      ),
    });
    const page = pageStub({
      locator: vi.fn(() => section),
      evaluate: vi.fn(async () => "clipboard"),
      context: vi.fn(() => ({ grantPermissions: vi.fn(async () => undefined) })) as unknown as Page["context"],
    });
    const driver = new WebNewDriver(page);
    await expect(driver.devMachinesInstallCards()).resolves.toEqual([
      { label: "Linux", command: "curl ...", downloadHref: null },
    ]);
    await expect(driver.devMachinesInstallPrereq()).resolves.toBe("Install doctor first");
    await expect(driver.devMachinesReadClipboard()).resolves.toBe("clipboard");
    await driver.devMachinesInstallCopy("Linux");
    await driver.devMachinesInstallBreakClipboard();
    await expect(driver.devMachinesInstallCopyState("Linux")).resolves.not.toBeUndefined();
  });

  it("opens runner detail in both scopes and reads header, meta, back and chat", async () => {
    const badge = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Online") });
    const header = stubLocator({
      count: vi.fn(async () => 1),
      isVisible: vi.fn(async () => true),
      innerText: vi.fn(async () => "runner-1"),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), getByTestId: vi.fn(() => badge) })),
    });
    const page = pageStub({
      locator: vi.fn(() => header),
      getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
      getByRole: vi.fn((role: string) =>
        role === "link"
          ? stubLocator({
              count: vi.fn(async () => 1),
              getAttribute: vi.fn(async () => "/acme/runners"),
              click: vi.fn(async () => undefined),
            })
          : stubLocator()
      ),
    });
    const driver = new WebNewDriver(page);
    await driver.runnerDetailOpen("acme", "r-1");
    expect(page.goto).toHaveBeenCalledWith("/acme/runners/detail/r-1");
    await driver.runnerDetailOpen("acme", "r-1", "p-1");
    expect(page.goto).toHaveBeenCalledWith("/acme/projects/p-1/runners/detail/r-1");
    await expect(driver.runnerDetailState()).resolves.toBe("loaded");
    await expect(driver.runnerDetailHeader()).resolves.toEqual({ name: "runner-1", status: "Online" });
    await expect(driver.runnerDetailBackHref()).resolves.toBe("/acme/runners");
    await driver.runnerDetailOpenChat();
    expect(page.waitForURL).toHaveBeenCalledWith("**/chat/**", expect.anything());
  });

  it("reads activity badge and telemetry and observes aging", async () => {
    const badge = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "1m ago") });
    let reads = 0;
    const lastActivity = stubLocator({
      count: vi.fn(async () => 1),
      innerText: vi.fn(async () => {
        reads += 1;
        return reads < 3 ? "1m ago" : "2m ago";
      }),
    });
    const grid = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => [{ label: "Last activity", value: "1m ago" }]),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), first: () => lastActivity })),
    });
    const section = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() => grid),
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1), locator: vi.fn(() => badge) })),
    });
    const page = pageStub({
      getByRole: vi.fn(() => section),
      route: vi.fn(async () => undefined),
      unroute: vi.fn(async () => undefined),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.runnerActivityTelemetry()).resolves.toEqual([{ label: "Last activity", value: "1m ago" }]);
    await expect(driver.runnerActivityBadge()).resolves.toBe("1m ago");
    await expect(driver.runnerActivityAgingObserved()).resolves.toBe(true);
  });

  it("reads the runner metadata grid from dt/dd pairs", async () => {
    const list = stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async () => [
        { label: "Status", value: "Online" },
        { label: "Last seen", value: "Jan 2, 10:00" },
      ]),
    });
    const section = stubLocator({
      count: vi.fn(async () => 1),
      locator: vi.fn(() => list),
    });
    const page = pageStub({
      locator: vi.fn(() => stubLocator({ first: () => section })),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.runnerDetailMeta()).resolves.toEqual([
      { label: "Status", value: "Online" },
      { label: "Last seen", value: "Jan 2, 10:00" },
    ]);
    expect(page.locator).toHaveBeenCalledWith("section", { hasText: "Metadata" });
    expect(section.locator).toHaveBeenCalledWith("dl");
  });

  it("returns an empty metadata grid without a section or list", async () => {
    const missingSection = new WebNewDriver(
      pageStub({ locator: vi.fn(() => stubLocator({ first: () => stubLocator({ count: vi.fn(async () => 0) }) })) })
    );
    await expect(missingSection.runnerDetailMeta()).resolves.toEqual([]);

    const missingList = new WebNewDriver(
      pageStub({
        locator: vi.fn(() =>
          stubLocator({
            first: () =>
              stubLocator({
                count: vi.fn(async () => 1),
                locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
              }),
          })
        ),
      })
    );
    await expect(missingList.runnerDetailMeta()).resolves.toEqual([]);
  });

  it("counts detail GET polls inside the window and unroutes afterwards", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    const count = driver.runnerDetailPollCount("r-1", 250);
    const calls = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls;
    expect(calls[0]![0]).toBe("**/api/runners/r-1/*");
    const handler = calls[0]![1] as (route: Route) => Promise<void>;
    const get = stubRoute("GET");
    await handler(get as unknown as Route);
    const post = stubRoute("POST");
    await handler(post as unknown as Route);
    await expect(count).resolves.toBe(1);
    expect(get.continue).toHaveBeenCalledTimes(1);
    expect(post.continue).toHaveBeenCalledTimes(1);
    expect(page.waitForTimeout).toHaveBeenCalledWith(250);
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/r-1/*", handler);
  });

  it("shapes runner detail requests and delays/fails only detail GETs", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnerDetailDelayOnce(5);
    const delay = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[0]![1] as (
      route: Route
    ) => Promise<void>;
    const get = stubRoute("GET");
    (get.request as unknown as () => unknown) = () => ({
      method: () => "GET",
      url: () => "https://api.test/api/runners/r-1",
    });
    await delay(get as unknown as Route);
    expect(page.waitForTimeout).toHaveBeenCalledWith(5);

    await driver.runnerDetailFailOnce();
    const fail = (page.route as unknown as { mock: { calls: unknown[][] } }).mock.calls[1]![1] as (
      route: Route
    ) => Promise<void>;
    const bad = stubRoute("GET");
    (bad.request as unknown as () => unknown) = () => ({ method: () => "GET", url: () => "not a url" });
    await fail(bad as unknown as Route);
    expect(bad.continue).toHaveBeenCalled();

    const list = stubRoute("GET");
    (list.request as unknown as () => unknown) = () => ({
      method: () => "GET",
      url: () => "https://api.test/api/runners/dev-machines/x",
    });
    await fail(list as unknown as Route);
    expect(list.continue).toHaveBeenCalledTimes(1);
  });
});

describe("issue dispatch and re-tick (NEWFRONT-547, RUN-046/RUN-048)", () => {
  it("dispatches Run AI, dedupes API urls, and unsubscribes", async () => {
    const handlers: Array<(request: { url(): string }) => void> = [];
    const on = vi.fn((_event: string, handler: (request: { url(): string }) => void) => {
      handlers.push(handler);
    });
    const off = vi.fn();
    const runButton = stubLocator({
      click: vi.fn(async () => {
        for (const handler of handlers) {
          handler({ url: () => "https://api.test/api/runners/runs/" });
          handler({ url: () => "https://api.test/api/runners/runs/" });
          handler({ url: () => "https://app.test/static.js" });
        }
      }),
    });
    const page = testIdPage({ "issue-run-ai": runButton }, { on, off });
    const driver = new WebNewDriver(page);
    // No toast yet on the first poll, so the driver waits and polls again.
    vi.spyOn(driver, "lastToast")
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
      .mockResolvedValue("Run started");

    await expect(driver.issueRunAiDispatch()).resolves.toEqual({
      toast: "Run started",
      requestUrls: ["https://api.test/api/runners/runs/"],
    });
    expect(page.waitForTimeout).toHaveBeenCalledWith(1_000);
    expect(off).toHaveBeenCalledWith("request", handlers[0]);
  });

  it("gives up dispatch with the latest toast after the deadline", async () => {
    const off = vi.fn();
    const page = testIdPage({ "issue-run-ai": textNode("Run AI") }, { off });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "lastToast").mockResolvedValue(null);
    const start = 1_000_000;
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValueOnce(start)
      .mockReturnValue(start + 200_000);
    try {
      await expect(driver.issueRunAiDispatch()).resolves.toEqual({ toast: null, requestUrls: [] });
    } finally {
      now.mockRestore();
    }
    expect(off).toHaveBeenCalled();
  });

  it("reads re-tick visibility from the issue control", async () => {
    const shown = new WebNewDriver(testIdPage({ "issue-re-tick": textNode("Re-tick") }));
    await expect(shown.issueReTickVisible()).resolves.toBe(true);

    const missing = new WebNewDriver(testIdPage({ "issue-re-tick": stubLocator({ count: vi.fn(async () => 0) }) }));
    await expect(missing.issueReTickVisible()).resolves.toBe(false);

    const hidden = new WebNewDriver(
      testIdPage({ "issue-re-tick": textNode("Re-tick", { isVisible: vi.fn(async () => false) }) })
    );
    await expect(hidden.issueReTickVisible()).resolves.toBe(false);
  });

  it("clicks re-tick and waits for a fresh toast", async () => {
    const click = vi.fn(async () => undefined);
    const page = testIdPage({ "issue-re-tick": stubLocator({ click }) });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "lastToast")
      .mockResolvedValueOnce("Older notice")
      .mockResolvedValueOnce("Older notice")
      .mockResolvedValue("Budget refilled");

    await expect(driver.issueReTickClick()).resolves.toBe("Budget refilled");
    expect(click).toHaveBeenCalled();
    expect(page.waitForTimeout).toHaveBeenCalledWith(1_000);
  });

  it("gives up re-tick with the latest toast after the deadline", async () => {
    const page = testIdPage({ "issue-re-tick": textNode("Re-tick") });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "lastToast").mockResolvedValue("Older notice");
    const start = 1_000_000;
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValueOnce(start)
      .mockReturnValue(start + 200_000);
    try {
      await expect(driver.issueReTickClick()).resolves.toBe("Older notice");
    } finally {
      now.mockRestore();
    }
  });
});

describe("desktop runtime + gate reads (NEWFRONT-557)", () => {
  it("opens the runners area and settles on the rail", async () => {
    const rail = stubLocator();
    const page = pageStub({ getByTestId: vi.fn(() => rail) });
    await new WebNewDriver(page).desktopRuntimeOpenRunners("acme");
    expect(page.goto).toHaveBeenCalledWith("/acme/runners");
    expect(page.waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    expect(page.getByTestId).toHaveBeenCalledWith("runners-rail");
    expect(rail.waitFor).toHaveBeenCalledWith({ timeout: 60_000 });
  });

  it("reads rail section headers trimmed", async () => {
    const headers = stubLocator({ count: vi.fn(async () => 2), innerText: vi.fn(async () => " Built-in ") });
    const rail = stubLocator({ locator: vi.fn(() => headers) });
    const page = pageStub({ getByTestId: vi.fn(() => rail) });
    await expect(new WebNewDriver(page).desktopRuntimeRailSectionHeaders()).resolves.toEqual(["Built-in", "Built-in"]);
    expect(rail.locator).toHaveBeenCalledWith("div.uppercase");
  });

  it("tolerates a header whose text read fails", async () => {
    const headers = stubLocator({
      count: vi.fn(async () => 1),
      innerText: vi.fn(async () => Promise.reject(new Error("gone"))),
    });
    const rail = stubLocator({ locator: vi.fn(() => headers) });
    const page = pageStub({ getByTestId: vi.fn(() => rail) });
    await expect(new WebNewDriver(page).desktopRuntimeRailSectionHeaders()).resolves.toEqual([""]);
  });

  it("collects rail chat links with their names", async () => {
    const links = stubLocator({
      count: vi.fn(async () => 1),
      innerText: vi.fn(async () => " alpha "),
      getAttribute: vi.fn(async () => "/acme/runners/chat/r-1"),
    });
    const rail = stubLocator({ locator: vi.fn(() => links) });
    const page = pageStub({ getByTestId: vi.fn(() => rail) });
    await expect(new WebNewDriver(page).desktopRuntimeRailChatLinks()).resolves.toEqual([
      { name: "alpha", href: "/acme/runners/chat/r-1" },
    ]);
    expect(rail.locator).toHaveBeenCalledWith('a[href*="/chat/"]');
  });

  it("opens a chat through the chat opener, with and without a session", async () => {
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.desktopRuntimeOpenChat("acme", "r-1", "s-9");
    expect(page.goto).toHaveBeenCalledWith("/acme/runners/chat/r-1?sessionId=s-9");
    await driver.desktopRuntimeOpenChat("acme", "r-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/runners/chat/r-1");
    expect(page.getByTestId).toHaveBeenCalledWith("runner-chat");
    expect(page.getByTestId).toHaveBeenCalledWith("runner-chat-composer");
  });

  it("reports the approval prompt and mode control from their roles", async () => {
    const role = vi.fn(() => stubLocator({ count: vi.fn(async () => 1) }));
    const page = pageStub({ getByRole: role });
    const driver = new WebNewDriver(page);
    await expect(driver.desktopRuntimeApprovalPromptVisible()).resolves.toBe(true);
    await expect(driver.desktopRuntimeApprovalModeVisible()).resolves.toBe(true);
    expect(role).toHaveBeenCalledWith("alertdialog", { name: "Approval requested" });
    expect(role).toHaveBeenCalledWith("group", { name: "Approval mode" });
    const absent = pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(absent).desktopRuntimeApprovalPromptVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(absent).desktopRuntimeApprovalModeVisible()).resolves.toBe(false);
  });

  it("reports the runtime banner only when the notice is shown", async () => {
    const shown = stubLocator({ count: vi.fn(async () => 1), isVisible: vi.fn(async () => true) });
    const page = pageStub({ getByTestId: vi.fn(() => shown) });
    await expect(new WebNewDriver(page).desktopRuntimeRuntimeBannerVisible()).resolves.toBe(true);
    const hidden = stubLocator({ count: vi.fn(async () => 1), isVisible: vi.fn(async () => false) });
    await expect(
      new WebNewDriver(pageStub({ getByTestId: vi.fn(() => hidden) })).desktopRuntimeRuntimeBannerVisible()
    ).resolves.toBe(false);
    const absent = stubLocator({ count: vi.fn(async () => 0) });
    await expect(
      new WebNewDriver(pageStub({ getByTestId: vi.fn(() => absent) })).desktopRuntimeRuntimeBannerVisible()
    ).resolves.toBe(false);
  });

  it("reads the native bridge flag from the page", async () => {
    const page = pageStub({ evaluate: vi.fn(async () => true) });
    await expect(new WebNewDriver(page).desktopRuntimeIsTauriPresent()).resolves.toBe(true);
  });

  // The transcript reader runs its extraction inside a page evaluate; the
  // stub hands the callback a minimal DOM-shaped column.
  function bubbleColumn(children: Array<{ role: string | null; text: string }>) {
    const element = {
      children: children.map(({ role, text }) => ({
        querySelector: () => ({ getAttribute: () => role, innerText: text }),
      })),
    };
    return stubLocator({
      count: vi.fn(async () => 1),
      evaluate: vi.fn(async (fn: (node: unknown) => unknown) => fn(element)),
    });
  }

  it("reads transcript bubbles with roles, skipping blanks", async () => {
    const column = bubbleColumn([
      { role: "user", text: "hello" },
      { role: "assistant", text: "  " },
    ]);
    const page = pageStub({ getByTestId: vi.fn(() => stubLocator({ locator: vi.fn(() => column) })) });
    await expect(new WebNewDriver(page).desktopRuntimeChatBubbles()).resolves.toEqual([
      { role: "user", text: "hello" },
    ]);
  });

  it("reads no bubbles without a list and defaults a missing role", async () => {
    const absent = pageStub({ getByTestId: vi.fn(() => stubLocator({ locator: vi.fn(() => stubLocator()) })) });
    await expect(new WebNewDriver(absent).desktopRuntimeChatBubbles()).resolves.toEqual([]);
    const column = bubbleColumn([{ role: null, text: "note" }]);
    const page = pageStub({ getByTestId: vi.fn(() => stubLocator({ locator: vi.fn(() => column) })) });
    await expect(new WebNewDriver(page).desktopRuntimeChatBubbles()).resolves.toEqual([
      { role: "status", text: "note" },
    ]);
  });

  it("reads storage keys from the page", async () => {
    const page = pageStub({ evaluate: vi.fn(async () => ({ local: ["a"], session: [] })) });
    await expect(new WebNewDriver(page).desktopRuntimeStorageKeys()).resolves.toEqual({
      local: ["a"],
      session: [],
    });
  });

  it("logs watched request URLs and forgets them on stop", async () => {
    const handlers: Array<(request: { url(): string }) => void> = [];
    const page = pageStub({
      on: vi.fn((_event: string, handler: (request: { url(): string }) => void) => {
        handlers.push(handler);
      }),
      off: vi.fn((_event: string, handler: (request: { url(): string }) => void) => {
        const at = handlers.indexOf(handler);
        if (at !== -1) handlers.splice(at, 1);
      }),
    });
    const driver = new WebNewDriver(page);
    await driver.desktopRuntimeStartRequestSpy();
    expect(page.on).toHaveBeenCalledWith("request", expect.any(Function));
    handlers[0]!({ url: () => "http://x/api/runners/chat/sessions/" });
    await expect(driver.desktopRuntimeSpyUrls()).resolves.toEqual(["http://x/api/runners/chat/sessions/"]);
    await driver.desktopRuntimeStopRequestSpy();
    expect(page.off).toHaveBeenCalledWith("request", expect.any(Function));
    expect(handlers).toHaveLength(0);
  });

  it("reads the detail title field and null when absent", async () => {
    const title = stubLocator({ count: vi.fn(async () => 1), inputValue: vi.fn(async () => "Parity probe") });
    const page = pageStub({ getByLabel: vi.fn(() => title) });
    await expect(new WebNewDriver(page).issueDetailTitle()).resolves.toBe("Parity probe");
    expect(page.getByLabel).toHaveBeenCalledWith("Title");
    const missing = pageStub({ getByLabel: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(missing).issueDetailTitle()).resolves.toBeNull();
  });

  it("reads a sidebar property minus its label", async () => {
    const row = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "State\nTodo") });
    const page = pageStub({ getByRole: vi.fn(() => row) });
    await expect(new WebNewDriver(page).sidebarProperty("State")).resolves.toBe("Todo");
  });
});
