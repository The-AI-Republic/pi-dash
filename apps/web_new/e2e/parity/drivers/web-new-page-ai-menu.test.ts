// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver page-editor AI menu probes
// (NEWFRONT-891, AGT-063). A Page double proves the address each method
// opens, the selectors it reads, the hover sweep and the request watch —
// never a real browser or the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => vi.restoreAllMocks());

interface FakeOptions {
  count?: number;
  visible?: boolean;
  items?: FakeLocator[];
  waitForRejects?: boolean;
  role?: () => FakeLocator;
}

class FakeLocator {
  readonly hover = vi.fn(async () => undefined);
  readonly waitFor = vi.fn(async (_options?: unknown) => {
    if (this.options.waitForRejects === true) throw new Error("timed out");
  });

  constructor(private readonly options: FakeOptions = {}) {}

  count(): Promise<number> {
    return Promise.resolve(this.options.items?.length ?? this.options.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    return Promise.resolve(this.options.visible ?? true);
  }

  getByRole(_role: string, _options?: { name?: string | RegExp }): FakeLocator {
    return this.options.role?.() ?? absent();
  }

  first(): FakeLocator {
    return this.options.items?.[0] ?? this;
  }

  nth(index: number): FakeLocator {
    return this.options.items?.[index] ?? absent();
  }
}

function absent(): FakeLocator {
  return new FakeLocator({ count: 0, visible: false, waitForRejects: true });
}

interface PageOptions {
  blocks?: number;
  handleCount?: number;
  menuCount?: number;
  menuVisible?: boolean;
  canvasPresent?: boolean;
  titlePresent?: boolean;
  refusalPresent?: boolean;
}

function pageDouble(options: PageOptions = {}) {
  const blocks = Array.from({ length: options.blocks ?? 3 }, () => new FakeLocator());
  const menu = new FakeLocator({ count: options.menuCount ?? 0, visible: options.menuVisible ?? true });
  const canvas = new FakeLocator({
    count: options.canvasPresent === false ? 0 : 1,
    waitForRejects: options.canvasPresent === false,
    role: () => menu,
  });
  const title = options.titlePresent === false ? absent() : new FakeLocator();
  const refusal = options.refusalPresent === true ? new FakeLocator() : absent();
  const handle = new FakeLocator({ count: options.handleCount ?? 0 });

  const page = {
    goto: vi.fn(async (_url: string) => undefined),
    waitForLoadState: vi.fn(async (_state: string) => undefined),
    waitForTimeout: vi.fn(async (_ms: number) => undefined),
    route: vi.fn(async (_pattern: string, _handler: (route: Route) => Promise<void>) => undefined),
    unroute: vi.fn(async (_pattern: string) => undefined),
    locator: vi.fn((selector: string) => {
      if (selector === "#page-content-container") return canvas;
      if (selector === '[data-testid="page-detail-title"] [role="textbox"]') return title;
      if (selector === '[data-testid="page-detail-panel"]') return refusal;
      if (selector === "#page-content-container .rich-text-content p") {
        return new FakeLocator({ items: blocks });
      }
      if (selector === '[data-testid="page-ai-handle"]') return handle;
      return absent();
    }),
  };
  return { page: page as unknown as Page, pageObject: page, blocks, handle, menu };
}

function routeHandler(pageObject: ReturnType<typeof pageDouble>["pageObject"], pattern: string) {
  const call = pageObject.route.mock.calls.find((args) => args[0] === pattern);
  if (call === undefined) throw new Error(`no watch installed for ${pattern}`);
  return call[1] as (route: Route) => Promise<void>;
}

function requestRoute(url: string) {
  return {
    request: () => ({ url: () => url }),
    fallback: vi.fn(async () => undefined),
  };
}

describe("pageEditorOpen", () => {
  it("opens the page address and settles on the editable title", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pageEditorOpen("acme", "p1", "page-1");

    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/projects/p1/pages/page-1");
    expect(stub.pageObject.waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    // Both assistant endpoints are watched before navigation: the menu
    // posts while the page is open, and the scenario reads the log after.
    expect(stub.pageObject.route).toHaveBeenCalledWith("**/api/workspaces/*/ai-assistant/", expect.any(Function));
    expect(stub.pageObject.route).toHaveBeenCalledWith("**/api/workspaces/*/rephrase-grammar/", expect.any(Function));
  });

  it("settles on the refusal panel instead of throwing when the page is not readable", async () => {
    const stub = pageDouble({ canvasPresent: false, titlePresent: false, refusalPresent: true });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.pageEditorOpen("acme", "p1", "locked")).resolves.toBeUndefined();
  });

  it("throws when neither the canvas nor the refusal panel settles", async () => {
    const stub = pageDouble({ canvasPresent: false, titlePresent: false, refusalPresent: false });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.pageEditorOpen("acme", "p1", "missing")).rejects.toThrow(
      "settled on neither a canvas nor the refusal panel"
    );
  });

  it("replaces an earlier watch rather than stacking a second one", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.pageEditorOpen("acme", "p1", "page-1");
    await driver.pageEditorOpen("acme", "p1", "page-2");
    expect(stub.pageObject.unroute).toHaveBeenCalledWith("**/api/workspaces/*/ai-assistant/");
    expect(stub.pageObject.unroute).toHaveBeenCalledWith("**/api/workspaces/*/rephrase-grammar/");
  });
});

describe("pageEditorAiHandleCount", () => {
  it("sweeps up to five body blocks and counts the revealed handles", async () => {
    const stub = pageDouble({ blocks: 7, handleCount: 0 });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pageEditorAiHandleCount()).resolves.toBe(0);

    expect(stub.blocks).toHaveLength(7);
    for (const block of stub.blocks.slice(0, 5)) expect(block.hover).toHaveBeenCalled();
    expect(stub.blocks[5]?.hover).not.toHaveBeenCalled();
    expect(stub.pageObject.waitForTimeout).toHaveBeenCalledWith(2000);
  });

  it("returns the count when a build enables the handle", async () => {
    const stub = pageDouble({ blocks: 2, handleCount: 3 });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.pageEditorAiHandleCount()).resolves.toBe(3);
  });

  it("reads an empty body as zero handles without hovering anything", async () => {
    const stub = pageDouble({ blocks: 0 });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.pageEditorAiHandleCount()).resolves.toBe(0);
  });
});

describe("pageEditorAiMenuVisible", () => {
  it("is true only when the menu entry exists and is visible", async () => {
    const shown = pageDouble({ menuCount: 1, menuVisible: true });
    await expect(new WebNewDriver(shown.page).pageEditorAiMenuVisible()).resolves.toBe(true);

    const hidden = pageDouble({ menuCount: 1, menuVisible: false });
    await expect(new WebNewDriver(hidden.page).pageEditorAiMenuVisible()).resolves.toBe(false);
  });

  it("is false when no entry renders", async () => {
    const stub = pageDouble({ menuCount: 0 });
    await expect(new WebNewDriver(stub.page).pageEditorAiMenuVisible()).resolves.toBe(false);
  });
});

describe("pageEditorRephraseRequests", () => {
  it("starts empty and records the assistant task call the page fires", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.pageEditorOpen("acme", "p1", "page-1");
    await expect(driver.pageEditorRephraseRequests()).resolves.toEqual([]);

    const taskUrl = "https://api.test/api/workspaces/acme/ai-assistant/";
    await routeHandler(stub.pageObject, "**/api/workspaces/*/ai-assistant/")(requestRoute(taskUrl) as unknown as Route);

    await expect(driver.pageEditorRephraseRequests()).resolves.toEqual([taskUrl]);
  });

  it("also records the retired rephrase endpoint if a build still fires it", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.pageEditorOpen("acme", "p1", "page-1");
    const rephraseUrl = "https://api.test/api/workspaces/acme/rephrase-grammar/";
    await routeHandler(
      stub.pageObject,
      "**/api/workspaces/*/rephrase-grammar/"
    )(requestRoute(rephraseUrl) as unknown as Route);

    await expect(driver.pageEditorRephraseRequests()).resolves.toEqual([rephraseUrl]);
  });

  it("clears the log on the next open so one scenario never reads the last page's calls", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.pageEditorOpen("acme", "p1", "page-1");
    await routeHandler(
      stub.pageObject,
      "**/api/workspaces/*/ai-assistant/"
    )(requestRoute("https://api.test/api/workspaces/acme/ai-assistant/") as unknown as Route);
    await driver.pageEditorOpen("acme", "p1", "page-2");

    await expect(driver.pageEditorRephraseRequests()).resolves.toEqual([]);
  });
});
