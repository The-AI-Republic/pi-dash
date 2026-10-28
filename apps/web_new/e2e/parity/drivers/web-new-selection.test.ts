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
  accepts: number;
  leaveGuardArmed: boolean;
  dialogHandlers: Array<(dialog: { type: () => string; accept: () => Promise<void> }) => void>;
}

function pageOf(parts: { testId?: (id: string) => Locator; locator?: (selector: string) => Locator } = {}): PageStub {
  const state: PageStub = {
    page: undefined as unknown as Page,
    testIds: [],
    selectors: [],
    reloads: 0,
    accepts: 0,
    leaveGuardArmed: false,
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
    evaluate: async () => state.leaveGuardArmed,
    on: (event: string, handler: (dialog: never) => void) => {
      if (event === "dialog") state.dialogHandlers.push(handler as never);
    },
    off: () => undefined,
    reload: async () => {
      state.reloads += 1;
      for (const handler of state.dialogHandlers) {
        handler({
          type: () => "beforeunload",
          accept: async () => {
            state.accepts += 1;
          },
        });
      }
      return null;
    },
    waitForLoadState: async () => undefined,
    waitForTimeout: async () => undefined,
  } as unknown as Page;
  return state;
}

describe("web_new selection reads", () => {
  // The count and bar reads follow the shared bulk-selection suite's
  // role-based strategy (base wholesale at the NEWFRONT-658 rebase); the
  // leave-guard pins below cover this gate's headless armed signal.
  it("reports no bulk bar when neither surface renders", async () => {
    const { page } = pageOf({ testId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(false);
  });

  it("reports an armed leave guard even without a native dialog", async () => {
    const stub = pageOf();
    stub.leaveGuardArmed = true;
    (stub.page as unknown as { reload: () => Promise<null> }).reload = async () => null;
    await expect(new WebNewDriver(stub.page).reloadSawDialog()).resolves.toBe(true);
  });

  it("reports a leave dialog when the page arms one", async () => {
    const stub = pageOf();
    await expect(new WebNewDriver(stub.page).reloadSawDialog()).resolves.toBe(true);
    expect(stub.reloads).toBe(1);
  });

  it("accepts the leave dialog so the reload proceeds", async () => {
    const stub = pageOf();
    await new WebNewDriver(stub.page).reloadSawDialog();
    expect(stub.accepts).toBe(1);
  });

  it("reports no leave dialog when the page arms none", async () => {
    const { page } = pageOf();
    (page as unknown as { reload: () => Promise<null> }).reload = async () => null;
    await expect(new WebNewDriver(page).reloadSawDialog()).resolves.toBe(false);
  });

  it("resolves instead of hanging when the reload rejects", async () => {
    const stub = pageOf();
    (stub.page as unknown as { reload: () => Promise<null> }).reload = async () => {
      throw new Error("navigation cancelled");
    };
    await expect(new WebNewDriver(stub.page).reloadSawDialog()).resolves.toBe(false);
    stub.leaveGuardArmed = true;
    await expect(new WebNewDriver(stub.page).reloadSawDialog()).resolves.toBe(true);
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
