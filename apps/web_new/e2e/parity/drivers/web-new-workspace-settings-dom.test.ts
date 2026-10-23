// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Executes the page-side predicates of the workspace-settings general driver
// (NEWFRONT-535) against a real DOM: the settle probe after navigation, the
// idle-label probe after saving, and the PATCH matcher after a logo removal.
import type { Page, Response } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebNewDriver, wsSettingsSurfaceSettled } from "./web-new";

type Fn = (...args: never[]) => unknown;

function capturePage(overrides: Record<string, unknown> = {}): Page {
  const empty = () => ({
    count: async () => 0,
    first: () => empty(),
    locator: () => empty(),
    waitFor: async () => undefined,
  });
  return {
    getByTestId: () => empty(),
    goto: vi.fn(async () => undefined),
    waitForLoadState: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => undefined),
    // wsSettingsOpen installs the asset bridge before it navigates.
    route: vi.fn(async () => undefined),
    url: () => "http://localhost:3000/acme/settings/",
    getByRole: () => ({
      first: () => ({ click: vi.fn(async () => undefined) }),
      click: vi.fn(async () => undefined),
    }),
    locator: () => ({ first: () => ({ waitFor: vi.fn(async () => undefined) }) }),
    ...overrides,
  } as unknown as Page;
}

function predicates(page: Page, method: "waitForFunction" | "waitForResponse"): Fn[] {
  const mock = page[method] as unknown as ReturnType<typeof vi.fn>;
  return mock.mock.calls.map((call) => call[0] as Fn);
}

function rect(width: number): DOMRect {
  return { width, height: 10, top: 0, left: 0, right: width, bottom: 10, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
}

afterEach(() => {
  document.title = "";
  document.body.innerHTML = "";
});

describe("wsSettingsOpen settle probe", () => {
  // The served document already has a title, so the title is never the
  // settle signal: the helper shares the surface predicate and then waits
  // for the frame heading (NEWFRONT-550).
  function headingPage(waitFor: ReturnType<typeof vi.fn>): Page {
    return capturePage({
      // Only the plus-shaped shell frame carries a section heading.
      getByTestId: (id: string) =>
        id === "workspace-settings-shell"
          ? { count: async () => 1 }
          : { count: async () => 0, first: () => ({ count: async () => 0 }) },
      locator: vi.fn(() => ({ first: () => ({ waitFor }) })),
    });
  }

  it("hands the surface predicate to Playwright instead of a title check", async () => {
    document.title = "Pi Dash";
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsOpen("acme", "general");
    const [settled] = predicates(page, "waitForFunction");
    expect(settled).toBe(wsSettingsSurfaceSettled);
    expect(settled()).toBe(false);
  });

  it("waits for the frame heading after the surface settles", async () => {
    document.body.innerHTML = "<div data-testid='workspace-settings-shell'></div>";
    document.title = "Pi Dash";
    const waitFor = vi.fn(async () => undefined);
    const page = headingPage(waitFor);
    await new WebNewDriver(page).wsSettingsOpen("acme", "");
    expect(predicates(page, "waitForFunction")[0]()).toBe(true);
    expect(waitFor).toHaveBeenCalledWith({ state: "visible", timeout: expect.any(Number) });
  });

  it("still returns when the frame heading never paints", async () => {
    const waitFor = vi.fn(async () => {
      throw new Error("timeout");
    });
    await expect(new WebNewDriver(headingPage(waitFor)).wsSettingsOpen("acme", "members")).resolves.toBeUndefined();
  });
});

describe("workspace-settings surface settle predicate (NEWFRONT-548)", () => {
  it("settles on the workspace-not-found heading the app renders", () => {
    document.body.innerHTML = "<main><h1> Workspace not found </h1></main>";
    document.title = "Pi Dash";
    expect(wsSettingsSurfaceSettled()).toBe(true);
  });

  it("does not settle on a document title alone", () => {
    document.title = "Workspace not found";
    document.body.innerHTML = "<main><p>Loading</p></main>";
    expect(wsSettingsSurfaceSettled()).toBe(false);
  });

  it("ignores an unrelated heading and settles on each instance screen", () => {
    document.body.innerHTML = "<h2>Recent exports</h2>";
    document.title = "Workspace not found";
    expect(wsSettingsSurfaceSettled()).toBe(false);

    document.body.innerHTML = "<div data-testid='workspace-settings-shell'></div>";
    expect(wsSettingsSurfaceSettled()).toBe(true);
    document.body.innerHTML = "<div data-testid='instance-maintenance'></div>";
    expect(wsSettingsSurfaceSettled()).toBe(true);
    document.body.innerHTML = "<div data-testid='instance-not-ready'></div>";
    expect(wsSettingsSurfaceSettled()).toBe(true);
    document.body.innerHTML = "<div data-testid='workspace-settings-not-authorized'></div>";
    expect(wsSettingsSurfaceSettled()).toBe(true);
  });

  it("treats a heading without text content as unsettled", () => {
    document.body.innerHTML = "<h1></h1>";
    const heading = document.querySelector("h1") as HTMLElement;
    Object.defineProperty(heading, "textContent", { get: () => null, configurable: true });
    expect(wsSettingsSurfaceSettled()).toBe(false);
  });

  it("hands the predicate to Playwright rather than a title check", async () => {
    const page = capturePage({
      context: () => ({ addCookies: vi.fn(async () => undefined) }),
    });
    await new WebNewDriver(page).exportsSettingsOpen("acme", []);
    const [settled] = predicates(page, "waitForFunction");
    expect(settled).toBe(wsSettingsSurfaceSettled);
  });
});

describe("wsSettingsSave idle probe", () => {
  it("waits for the idle Update workspace label", async () => {
    // jsdom has no innerText; the probe reads it from real browsers.
    document.body.innerHTML = "<button></button>";
    const button = document.querySelector("button") as HTMLElement;
    const label = { value: "  Update Workspace " };
    Object.defineProperty(button, "innerText", { get: () => label.value, configurable: true });
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsSave();
    const [idle] = predicates(page, "waitForFunction");
    expect(idle()).toBe(true);

    label.value = "Updating…";
    expect(idle()).toBe(false);
    label.value = undefined as unknown as string;
    expect(idle()).toBe(false);
  });

  it("does not settle when no button exists", async () => {
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsSave();
    expect(predicates(page, "waitForFunction")[0]()).toBe(false);
  });
});

describe("wsSettingsRemoveLogo PATCH matcher", () => {
  it("matches only the workspace PATCH", async () => {
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsRemoveLogo();
    const [matches] = predicates(page, "waitForResponse");
    const response = (method: string, url: string) =>
      ({ request: () => ({ method: () => method, url: () => url }) }) as unknown as Response;

    expect(matches(response("PATCH", "http://localhost:3000/api/workspaces/acme/") as never)).toBe(true);
    expect(matches(response("POST", "http://localhost:3000/api/workspaces/acme/") as never)).toBe(false);
    expect(matches(response("PATCH", "http://localhost:3000/api/workspaces/beta/") as never)).toBe(false);
    expect(matches(response("PATCH", "not a url") as never)).toBe(false);
  });
});

describe("wsSettingsSizeLabel", () => {
  it("returns the collapsed label", async () => {
    const page = capturePage({
      getByRole: () => ({ first: () => ({ innerText: async () => "  51-200\n workspaces " }) }),
    });
    await expect(new WebNewDriver(page).wsSettingsSizeLabel()).resolves.toBe("51-200 workspaces");
  });

  it("returns an empty label when the control is missing or empty", async () => {
    const missing = capturePage({
      getByRole: () => ({
        first: () => ({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
      }),
    });
    await expect(new WebNewDriver(missing).wsSettingsSizeLabel()).resolves.toBe("");

    const blank = capturePage({ getByRole: () => ({ first: () => ({ innerText: async () => null }) }) });
    await expect(new WebNewDriver(blank).wsSettingsSizeLabel()).resolves.toBe("");
  });
});

describe("settings navigation reads (NEWFRONT-550)", () => {
  // The denial and instance screens render the settings frame without a
  // navigation at all. A read that waits on that node spends the whole
  // bound and then fails a row that should simply see no entries.
  function navLocator(count: () => Promise<number>) {
    const node = {
      count,
      evaluate: vi.fn(),
      getByTestId: () => node,
      locator: () => node,
      first: () => node,
    };
    return node;
  }

  it("reads an absent navigation as no groups and no entries", async () => {
    const nav = navLocator(async () => 0);
    const page = capturePage({ getByTestId: () => ({ getByTestId: () => nav, first: () => nav }) });
    const driver = new WebNewDriver(page);

    await expect(driver.wsSettingsGroups()).resolves.toEqual([]);
    await expect(driver.wsSettingsItems()).resolves.toEqual([]);
    expect(nav.evaluate).not.toHaveBeenCalled();
  });

  it("reads visible group headings and entries in document order", async () => {
    document.body.innerHTML = `<nav>
      <section><h2> Workspace </h2><ul><li><a>General</a></li><li><a>Members</a></li></ul></section>
      <section><h2>Developer</h2><ul><li><a>Webhooks</a></li></ul></section>
      <section hidden><h2>Hidden</h2><ul><li><a>Secret</a></li></ul></section>
    </nav>`;
    const root = document.querySelector("nav") as HTMLElement;
    root.querySelectorAll("a").forEach((link) => {
      Object.defineProperty(link, "getBoundingClientRect", { value: () => rect(120), configurable: true });
      Object.defineProperty(link, "innerText", { get: () => link.textContent, configurable: true });
    });
    const hidden = root.querySelector("section[hidden]") as HTMLElement;
    Object.defineProperty(hidden, "getBoundingClientRect", { value: () => rect(0), configurable: true });
    root.querySelectorAll("section:not([hidden])").forEach((section) => {
      Object.defineProperty(section, "getBoundingClientRect", { value: () => rect(400), configurable: true });
    });
    const nav = navLocator(async () => 1);
    nav.evaluate = vi.fn((callback: (node: Element) => unknown) => Promise.resolve(callback(root)));
    const page = capturePage({ getByTestId: () => ({ getByTestId: () => nav, first: () => nav }) });
    const driver = new WebNewDriver(page);

    await expect(driver.wsSettingsGroups()).resolves.toEqual(["Workspace", "Developer"]);
    await expect(driver.wsSettingsItems()).resolves.toEqual(["General", "Members", "Webhooks"]);
  });
});
