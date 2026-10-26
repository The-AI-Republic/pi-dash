// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver selection and layout-switch methods
// (NEWFRONT-658, ISS-108–ISS-111, ISS-116). A stub Page records the
// selectors each method reads, so dropping the markup hooks that the
// bulk-selection scenarios depend on fails here before the parity stack.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  first?: () => StubLocator;
  filter?: (options: unknown) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    filter: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

interface PageStub {
  page: Page;
  testIds: string[];
  selectors: string[];
  reloads: number;
  dialogHandlers: Array<(dialog: { type: () => string; dismiss: () => Promise<void> }) => void>;
}

function pageOf(parts: { testId?: (id: string) => Locator; locator?: (selector: string) => Locator } = {}): PageStub {
  const state: PageStub = {
    page: undefined as unknown as Page,
    testIds: [],
    selectors: [],
    reloads: 0,
    dialogHandlers: [],
  };
  state.page = {
    getByTestId: (id: string) => {
      state.testIds.push(id);
      return parts.testId?.(id) ?? locator();
    },
    locator: (selector: string) => {
      state.selectors.push(selector);
      return parts.locator?.(selector) ?? locator();
    },
    getByRole: () => locator({ count: async () => 0 }),
    keyboard: { down: async () => undefined, up: async () => undefined, press: async () => undefined },
    on: (event: string, handler: (dialog: never) => void) => {
      if (event === "dialog") state.dialogHandlers.push(handler as never);
    },
    off: () => undefined,
    reload: async () => {
      state.reloads += 1;
      for (const handler of state.dialogHandlers) {
        handler({ type: () => "beforeunload", dismiss: async () => undefined });
      }
      return null;
    },
    waitForLoadState: async () => undefined,
    waitForTimeout: async () => undefined,
  } as unknown as Page;
  return state;
}

describe("web_new selection reads", () => {
  it("counts only the marked selection controls", async () => {
    const { page, selectors } = pageOf({ locator: () => locator({ count: async () => 3 }) });
    await expect(new WebNewDriver(page).selectionCheckboxCount()).resolves.toBe(3);
    expect(selectors).toContain('[data-testid="selection-checkbox"]');
  });

  it("reports the cloud toolbar as a bulk bar", async () => {
    const { page, testIds } = pageOf({
      testId: (id) => locator({ isVisible: async () => id === "bulk-toolbar" }),
    });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(true);
    expect(testIds).toEqual(["bulk-toolbar"]);
  });

  it("falls back to the upgrade banner when no toolbar shows", async () => {
    const { page, testIds } = pageOf({
      testId: (id) => locator({ count: async () => (id === "bulk-upgrade-banner" ? 1 : 0) }),
    });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(true);
    expect(testIds).toEqual(["bulk-toolbar", "bulk-upgrade-banner"]);
  });

  it("reports no bulk bar when neither surface renders", async () => {
    const { page } = pageOf({ testId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(false);
  });

  it("reports a leave dialog when the page arms one", async () => {
    const stub = pageOf();
    await expect(new WebNewDriver(stub.page).reloadSawDialog()).resolves.toBe(true);
    expect(stub.reloads).toBe(1);
  });

  it("reports no leave dialog when the page arms none", async () => {
    const { page } = pageOf();
    (page as unknown as { reload: () => Promise<null> }).reload = async () => null;
    await expect(new WebNewDriver(page).reloadSawDialog()).resolves.toBe(false);
  });
});

describe("web_new layout switching", () => {
  it("switches to the timeline through the segmented switcher", async () => {
    const { page, selectors } = pageOf();
    await new WebNewDriver(page).switchIssueLayout("Timeline Layout");
    expect(selectors).toContain('[data-layout="gantt_chart"]');
  });

  it("maps a spacing and case variation of the list label", async () => {
    const { page, selectors } = pageOf();
    await new WebNewDriver(page).switchIssueLayout("list  layout");
    expect(selectors).toContain('[data-layout="list"]');
  });

  it("swallows a switch failure in the best-effort list helper", async () => {
    const { page } = pageOf({
      locator: () =>
        locator({
          click: async () => {
            throw new Error("no switcher");
          },
        }),
    });
    await expect(new WebNewDriver(page).ensureListLayout()).resolves.toBeUndefined();
  });
});
