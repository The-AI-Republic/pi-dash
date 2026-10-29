// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the bulk-selection driver reads and actions
// (NEWFRONT-129, ISS-108–111). The old OSS app renders no selection
// controls, while apps/web_new renders row controls, the upgrade banner
// and a leave guard. A stub Page pins the selectors and the dialog
// listener lifecycle without bringing up the parity stack.
import type { Dialog, Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebDriver } from "./web";
import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  first?: () => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    ...overrides,
  };
  node.first = () => node;
  return node as unknown as Locator;
}

interface RoleCall {
  role: string;
  name: unknown;
}

interface PageState {
  roleCalls: RoleCall[];
  testIds: string[];
  listeners: Array<{ event: string; handler: (arg: never) => unknown }>;
  offCalls: Array<{ event: string; handler: (arg: never) => unknown }>;
  reloads: number;
  loadStates: string[];
  timeouts: number[];
}

function pageOf(
  parts: {
    checkbox?: Locator;
    banner?: Locator;
    onReload?: (state: PageState) => Promise<void>;
  } = {}
): { page: Page; state: PageState } {
  const state: PageState = {
    roleCalls: [],
    testIds: [],
    listeners: [],
    offCalls: [],
    reloads: 0,
    loadStates: [],
    timeouts: [],
  };
  const page = {
    getByRole: (role: string, options?: { name?: unknown }) => {
      state.roleCalls.push({ role, name: options?.name });
      return parts.checkbox ?? locator();
    },
    getByTestId: (id: string) => {
      state.testIds.push(id);
      return parts.banner ?? locator();
    },
    on: (event: string, handler: (arg: never) => unknown) => {
      state.listeners.push({ event, handler });
    },
    off: (event: string, handler: (arg: never) => unknown) => {
      state.offCalls.push({ event, handler });
    },
    reload: async () => {
      state.reloads += 1;
      await parts.onReload?.(state);
    },
    waitForLoadState: async (loadState: string) => {
      state.loadStates.push(loadState);
    },
    waitForTimeout: async (ms: number) => {
      state.timeouts.push(ms);
    },
    // No armed leave guard in these scenarios; the armed path is pinned
    // by the web_new selection tests.
    evaluate: async () => false,
  } as unknown as Page;
  return { page, state };
}

const drivers = [
  ["web", (page: Page) => new WebDriver(page)] as const,
  ["web_new", (page: Page) => new WebNewDriver(page)] as const,
];

describe.each(drivers)("%s bulk selection", (_target, createDriver) => {
  it("counts row and group controls named like selection checkboxes", async () => {
    const checkbox = locator({ count: async () => 3 });
    const { page, state } = pageOf({ checkbox });
    await expect(createDriver(page).selectionCheckboxCount()).resolves.toBe(3);
    expect(state.roleCalls).toHaveLength(1);
    expect(state.roleCalls[0]?.role).toBe("checkbox");
    const name = state.roleCalls[0]?.name as RegExp;
    expect(name.test("Select WEB-1")).toBe(true);
    expect(name.test("Select all work items in Todo")).toBe(true);
  });

  it("clicks the first row control rather than a group control", async () => {
    const click = vi.fn(async () => undefined);
    const { page, state } = pageOf({ checkbox: locator({ count: async () => 2, click }) });
    await createDriver(page).selectFirstRow();
    expect(click).toHaveBeenCalledTimes(1);
    const name = state.roleCalls[0]?.name as RegExp;
    expect(name.test("Select WEB-1")).toBe(true);
    expect(name.test("Select all work items in Todo")).toBe(false);
  });

  it("fails loudly when no row control is rendered", async () => {
    const { page } = pageOf({ checkbox: locator({ count: async () => 0 }) });
    await expect(createDriver(page).selectFirstRow()).rejects.toThrow("[parity] no row selection control is rendered.");
  });
});

describe("web_new OSS upgrade banner", () => {
  it("reports no banner when it is not mounted", async () => {
    const { page, state } = pageOf({ banner: locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(false);
    expect(state.testIds).toEqual(["bulk-upgrade-banner"]);
  });

  it("reports a mounted, visible banner", async () => {
    const { page } = pageOf({
      banner: locator({ count: async () => 1, isVisible: async () => true }),
    });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(true);
  });

  it("does not report a mounted but hidden banner", async () => {
    const { page } = pageOf({
      banner: locator({ count: async () => 1, isVisible: async () => false }),
    });
    await expect(new WebNewDriver(page).bulkBarVisible()).resolves.toBe(false);
  });
});

describe("web_new reload leave guard", () => {
  it("reports a dialog, accepts it and removes the listener", async () => {
    // Accept (not dismiss): dismissing a beforeunload dialog cancels the
    // navigation and the reload would hang to the test timeout (ISS-110).
    const accept = vi.fn(async () => undefined);
    const dialog = { accept } as unknown as Dialog;
    const { page, state } = pageOf({
      onReload: async (current) => {
        const listener = current.listeners[0];
        expect(listener?.event).toBe("dialog");
        await listener?.handler(dialog as never);
      },
    });

    await expect(new WebNewDriver(page).reloadSawDialog()).resolves.toBe(true);
    expect(state.reloads).toBe(1);
    expect(state.loadStates).toEqual(["domcontentloaded"]);
    expect(state.timeouts).toEqual([2000]);
    expect(accept).toHaveBeenCalledTimes(1);
    expect(state.offCalls).toEqual([{ event: "dialog", handler: state.listeners[0]?.handler }]);
  });

  it("reports no dialog when the reload raises none", async () => {
    const { page, state } = pageOf();
    await expect(new WebNewDriver(page).reloadSawDialog()).resolves.toBe(false);
    expect(state.reloads).toBe(1);
    expect(state.offCalls).toHaveLength(1);
  });

  it("resolves and removes the listener when the reload fails", async () => {
    // A failed reload (cancelled navigation, unsettled load) must not
    // propagate: the observation still answers armed-or-saw-dialog, and
    // propagating turned ISS-110 into a 300s hang.
    const { page, state } = pageOf({
      onReload: async () => {
        throw new Error("reload failed");
      },
    });
    await expect(new WebNewDriver(page).reloadSawDialog()).resolves.toBe(false);
    expect(state.offCalls).toHaveLength(1);
  });
});
