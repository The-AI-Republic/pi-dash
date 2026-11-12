// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver view-layout methods (NEWFRONT-658,
// ISS-219). A stub Page stands in for Playwright: each case pins the
// views list open, the create form, and the layout picker's options,
// search absence, selection mark, and pick-and-close flow.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  click?: (options?: unknown) => Promise<void>;
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  getByLabel?: (label: string) => Locator;
  locator?: (selector: string) => Locator;
  first?: () => Locator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    click: async () => undefined,
    count: async () => 1,
    innerText: async () => "",
    allInnerTexts: async () => [],
    fill: async () => undefined,
    waitFor: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  const defaults: Record<string, () => Locator> = {
    getByRole: () => self,
    getByTestId: () => self,
    getByLabel: () => self,
    locator: () => self,
    first: () => self,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return self;
}

interface PageParts {
  goto?: (url: string, options?: unknown) => Promise<void>;
  waitForLoadState?: (state: string) => Promise<void>;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByTestId?: (id: string | RegExp) => Locator;
  locator?: (selector: string) => Locator;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    goto: parts.goto ?? (async () => undefined),
    waitForLoadState: parts.waitForLoadState ?? (async () => undefined),
    getByRole: (role: string, options?: unknown) => parts.getByRole?.(role, options) ?? locator(),
    getByTestId: (id: string | RegExp) => parts.getByTestId?.(id) ?? locator(),
    locator: (selector: string) => parts.locator?.(selector) ?? locator(),
  } as unknown as Page;
}

describe("view layout picker", () => {
  it("opens the views list and settles on the create entry", async () => {
    const gotos: string[] = [];
    const calls: string[] = [];
    const page = pageOf({
      goto: async (url: string) => void gotos.push(url),
      getByRole: (role: string, roleOptions?: unknown) => {
        calls.push(`${role}:${JSON.stringify(roleOptions)}`);
        return locator();
      },
    });
    await new WebNewDriver(page).viewsOpenList("ws", "pid");
    expect(gotos).toEqual(["/ws/projects/pid/views"]);
    expect(calls).toContain('button:{"name":"Create view"}');
  });

  it("opens the create form from the create entry", async () => {
    const calls: string[] = [];
    const page = pageOf({
      getByRole: () => locator({ click: async () => void calls.push("create") }),
    });
    await new WebNewDriver(page).viewsOpenCreate();
    expect(calls).toContain("create");
  });

  it("reads the layout trigger value", async () => {
    const form = locator({ getByRole: () => locator({ innerText: async () => "  List " }) });
    const page = pageOf({ getByTestId: () => form });
    await expect(new WebNewDriver(page).viewsLayoutValue()).resolves.toBe("List");
  });

  it("leaves an open layout popup alone", async () => {
    let clicks = 0;
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1 }),
      getByTestId: () => locator({ getByRole: () => locator({ click: async () => void (clicks += 1) }) }),
    });
    await new WebNewDriver(page).viewsLayoutOpenPicker();
    expect(clicks).toBe(0);
  });

  it("opens a closed layout popup and waits for options", async () => {
    const calls: string[] = [];
    const page = pageOf({
      getByRole: () => locator({ count: async () => 0 }),
      getByTestId: () => locator({ getByRole: () => locator({ click: async () => void calls.push("trigger") }) }),
      locator: (selector: string) => {
        calls.push(`list:${selector}`);
        return locator();
      },
    });
    await new WebNewDriver(page).viewsLayoutOpenPicker();
    expect(calls).toContain("trigger");
    expect(calls).toContain('list:ul[aria-label="Layouts"]');
  });

  it("reads option labels in order, dropping blanks", async () => {
    const page = pageOf({
      locator: () =>
        locator({
          getByRole: () => locator({ allInnerTexts: async () => ["List", "", "Board"] }),
        }),
    });
    await expect(new WebNewDriver(page).viewsLayoutOptionTexts()).resolves.toEqual(["List", "Board"]);
  });

  it("reports the search box only when the popup carries one", async () => {
    const withSearch = pageOf({
      locator: () => locator({ locator: () => locator({ count: async () => 1 }) }),
    });
    await expect(new WebNewDriver(withSearch).viewsLayoutHasSearch()).resolves.toBe(true);
    const bare = pageOf({
      locator: () => locator({ locator: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(bare).viewsLayoutHasSearch()).resolves.toBe(false);
  });

  it("reads the selected mark off the option", async () => {
    const marked = pageOf({
      locator: () => locator({ getByRole: () => locator({ count: async () => 1 }) }),
    });
    await expect(new WebNewDriver(marked).viewsLayoutSelectedMarked("List")).resolves.toBe(true);
    const plain = pageOf({
      locator: () => locator({ getByRole: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(plain).viewsLayoutSelectedMarked("Board")).resolves.toBe(false);
  });

  it("picks by substring and waits for the popup to close", async () => {
    const calls: string[] = [];
    const page = pageOf({
      locator: () =>
        locator({
          getByRole: (_role: string, roleOptions?: unknown) => {
            calls.push(`pick:${String((roleOptions as { name?: unknown } | undefined)?.name)}`);
            return locator({ click: async () => void calls.push("clicked") });
          },
          waitFor: async (waitOptions?: unknown) => void calls.push(`wait:${JSON.stringify(waitOptions)}`),
        }),
    });
    await new WebNewDriver(page).viewsLayoutPick("Board");
    expect(calls).toContain("pick:/Board/");
    expect(calls).toContain("clicked");
    expect(calls).toContain(`wait:${JSON.stringify({ state: "detached", timeout: 30_000 })}`);
  });

  it("fills the title and submits the form", async () => {
    const calls: string[] = [];
    const form = locator({
      getByLabel: (label: string) =>
        locator({
          fill: async (value: string) => void calls.push(`fill:${label}:${value}`),
        }),
      getByRole: () => locator({ click: async () => void calls.push("submit") }),
    });
    const page = pageOf({ getByTestId: () => form });
    await new WebNewDriver(page).viewsFillName("Nightly");
    await new WebNewDriver(page).viewsSubmit();
    expect(calls).toContain("fill:Title:Nightly");
    expect(calls).toContain("submit");
  });
});
