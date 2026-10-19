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
    const page = pageStub();
    const driver = new WebNewDriver(page);
    await driver.runnerChatOpen("acme", "r-1", "s-9");
    expect(page.goto).toHaveBeenCalledWith("/acme/runners/chat/r-1?sessionId=s-9", { timeout: 60_000 });
    await driver.runnerChatOpen("acme", "r-1");
    expect(page.goto).toHaveBeenLastCalledWith("/acme/runners/chat/r-1", { timeout: 60_000 });
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
    expect(submit.waitFor).toHaveBeenCalledWith({ state: "hidden", timeout: expect.any(Number) });
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
