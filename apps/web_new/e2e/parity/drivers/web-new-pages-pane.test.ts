// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver pages navigation-pane + versions
// probes (NEWFRONT-922, PAGE-045-054). A Page double proves the addresses,
// selectors, route-spy shape and member filter; no browser or parity stack
// is involved.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface LocatorOptions {
  count?: number | (() => number);
  visible?: boolean;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: { state?: string; timeout?: number }) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  isVisible?: () => Promise<boolean>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  locator?: (selector: string) => Locator;
  getByRole?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator;
  getByTestId?: (id: string) => Locator;
  getByText?: (text: string | RegExp, options?: { exact?: boolean }) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
  last?: () => Locator;
  evaluate?: (fn: (node: unknown, arg?: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  evaluateAll?: (fn: (nodes: unknown[]) => unknown) => Promise<unknown>;
}

class FakeLocator {
  readonly waitFor: ReturnType<typeof vi.fn>;
  readonly click: ReturnType<typeof vi.fn>;
  readonly hover: ReturnType<typeof vi.fn>;
  readonly scrollIntoViewIfNeeded: ReturnType<typeof vi.fn>;
  readonly evaluate: ReturnType<typeof vi.fn>;
  readonly isVisible: ReturnType<typeof vi.fn>;

  constructor(private readonly options: LocatorOptions = {}) {
    this.waitFor = vi.fn(this.options.waitFor ?? (async () => undefined));
    this.click = vi.fn(this.options.click ?? (async () => undefined));
    this.hover = vi.fn(this.options.hover ?? (async () => undefined));
    this.scrollIntoViewIfNeeded = vi.fn(this.options.scrollIntoViewIfNeeded ?? (async () => undefined));
    this.evaluate = vi.fn(
      this.options.evaluate ??
        (async (fn: (node: unknown, arg?: unknown) => unknown, arg?: unknown) => fn(undefined, arg))
    );
    this.isVisible = vi.fn(this.options.isVisible ?? (async () => this.options.visible ?? this.countValue() > 0));
  }

  private countValue(): number {
    const count = this.options.count ?? 0;
    return typeof count === "function" ? count() : count;
  }

  count(): Promise<number> {
    return Promise.resolve(this.countValue());
  }

  innerText(): Promise<string> {
    return (this.options.innerText ?? (async () => ""))();
  }

  allInnerTexts(): Promise<string[]> {
    return (this.options.allInnerTexts ?? (async () => []))();
  }

  getAttribute(name: string): Promise<string | null> {
    return (this.options.getAttribute ?? (async () => null))(name);
  }

  boundingBox(): Promise<{ x: number; y: number; width: number; height: number } | null> {
    return (this.options.boundingBox ?? (async () => null))();
  }

  locator(selector: string): Locator {
    return this.options.locator?.(selector) ?? empty();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): Locator {
    return this.options.getByRole?.(role, options) ?? empty();
  }

  getByTestId(id: string): Locator {
    return this.options.getByTestId?.(id) ?? empty();
  }

  getByText(text: string | RegExp, options?: { exact?: boolean }): Locator {
    return this.options.getByText?.(text, options) ?? empty();
  }

  evaluateAll(fn: (nodes: unknown[]) => unknown): Promise<unknown> {
    return (this.options.evaluateAll ?? (async () => []))(fn);
  }

  first(): Locator {
    return this.options.first?.() ?? this.asLocator();
  }

  nth(index: number): Locator {
    return this.options.nth?.(index) ?? empty();
  }

  last(): Locator {
    return this.options.last?.() ?? this.asLocator();
  }

  asLocator(): Locator {
    return this as unknown as Locator;
  }
}

function empty(): Locator {
  return new FakeLocator({ count: 0 }) as unknown as Locator;
}

function at(options: LocatorOptions = {}): Locator {
  return new FakeLocator({ count: 1, ...options }) as unknown as Locator;
}

function fake(options: LocatorOptions = {}): FakeLocator {
  return new FakeLocator({ count: 1, ...options });
}

interface PageOptions {
  url?: string;
  testIds?: Record<string, Locator>;
  roles?: Record<string, Locator>;
  selectors?: Record<string, Locator>;
  evaluate?: (fn: (arg?: unknown) => unknown, arg?: unknown) => Promise<unknown>;
}

interface PageDouble {
  page: Page;
  goto: ReturnType<typeof vi.fn>;
  route: ReturnType<typeof vi.fn>;
  type: ReturnType<typeof vi.fn>;
  press: ReturnType<typeof vi.fn>;
  evaluate: ReturnType<typeof vi.fn>;
  waitForURL: ReturnType<typeof vi.fn>;
  goBack: ReturnType<typeof vi.fn>;
}

function pageDouble(options: PageOptions = {}): PageDouble {
  const goto = vi.fn(async () => undefined);
  const route = vi.fn(async () => undefined);
  const type = vi.fn(async () => undefined);
  const press = vi.fn(async () => undefined);
  const waitForURL = vi.fn(async () => undefined);
  const goBack = vi.fn(async () => undefined);
  const evaluate = vi.fn(options.evaluate ?? (async (fn: (arg?: unknown) => unknown, arg?: unknown) => fn(arg)));
  const page = {
    goto,
    route,
    waitForURL,
    goBack,
    evaluate,
    url: () => options.url ?? "https://parity.test/",
    keyboard: { type, press },
    getByTestId: vi.fn((id: string) => options.testIds?.[id] ?? empty()),
    getByRole: vi.fn((role: string) => options.roles?.[role] ?? empty()),
    locator: vi.fn((selector: string) => options.selectors?.[selector] ?? empty()),
  } as unknown as Page;
  return { page, goto, route, type, press, evaluate, waitForURL, goBack };
}

interface RouteDouble {
  route: Route;
  fallback: ReturnType<typeof vi.fn>;
  fulfill: ReturnType<typeof vi.fn>;
  fetch: ReturnType<typeof vi.fn>;
}

function routeDouble(method = "GET"): RouteDouble {
  const fallback = vi.fn(async () => undefined);
  const fulfill = vi.fn(async () => undefined);
  const fetch = vi.fn(async () => {
    throw new Error("no answer staged");
  });
  const route = {
    request: () => ({ method: () => method }),
    fallback,
    fulfill,
    fetch,
  } as unknown as Route;
  return { route, fallback, fulfill, fetch };
}

const BODY_SELECTOR = "#page-content-container .frame-renderer > .editor-container";
const EDITOR_SELECTOR = '[contenteditable="true"]';

/** A live-editor double with paragraph/image children wired by selector. */
function editorOf(children: Record<string, Locator> = {}, options: LocatorOptions = {}): Locator {
  return at({
    ...options,
    locator: (selector: string) => children[selector] ?? empty(),
  });
}

function bodyOf(editor: Locator, extra: Record<string, Locator> = {}, options: LocatorOptions = {}): Locator {
  return at({
    ...options,
    locator: (selector: string) => {
      if (selector === EDITOR_SELECTOR) return editor;
      return extra[selector] ?? empty();
    },
  });
}

function paneOf(options: LocatorOptions = {}): Locator {
  return at({
    getAttribute: async (name: string) => (name === "data-open" ? "true" : null),
    ...options,
  });
}

describe("detail navigation", () => {
  it("opens a detail and waits for the body and the live editor", async () => {
    const editor = fake();
    const body = fake({ locator: (selector: string) => (selector === EDITOR_SELECTOR ? editor.asLocator() : empty()) });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: body.asLocator() } });

    await new WebNewDriver(stub.page).pagesPaneOpenDetail("acme", "project-1", "page-1");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1", { waitUntil: "domcontentloaded" });
    expect(body.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    expect(editor.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 90_000 });
  });

  it("opens a detail with a query string", async () => {
    const editor = at();
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor) } });

    await new WebNewDriver(stub.page).pagesPaneOpenDetailQuery("acme", "project-1", "page-1", "paneTab=info");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1?paneTab=info", {
      waitUntil: "domcontentloaded",
    });
  });

  it("opens a read-only detail and waits for the body only", async () => {
    const editor = fake();
    const body = fake({ locator: (selector: string) => (selector === EDITOR_SELECTOR ? editor.asLocator() : empty()) });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: body.asLocator() } });

    await new WebNewDriver(stub.page).pagesPaneOpenDetailReadOnly("acme", "project-1", "page-1");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1", { waitUntil: "domcontentloaded" });
    expect(body.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    expect(editor.waitFor).not.toHaveBeenCalled();
  });
});

describe("editor reads and writes", () => {
  it("reads live editor text trimmed", async () => {
    const editor = editorOf({}, { innerText: async () => "  Hello world  " });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor) } });

    await expect(new WebNewDriver(stub.page).pagesPaneEditorText()).resolves.toBe("Hello world");
  });

  it("reads an empty string when the editor detaches or reads null", async () => {
    const detached = editorOf(
      {},
      {
        innerText: async () => {
          throw new Error("detached");
        },
      }
    );
    const nullish = editorOf({}, { innerText: async () => null as unknown as string });
    const detachedDriver = new WebNewDriver(pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(detached) } }).page);
    const nullishDriver = new WebNewDriver(pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(nullish) } }).page);

    await expect(detachedDriver.pagesPaneEditorText()).resolves.toBe("");
    await expect(nullishDriver.pagesPaneEditorText()).resolves.toBe("");
  });

  it("reads an empty string when no body rendered at all", async () => {
    const stub = pageDouble();

    await expect(new WebNewDriver(stub.page).pagesPaneEditorText()).resolves.toBe("");
  });

  it("falls back to the stored body when no live editor exists", async () => {
    const stored = bodyOf(empty(), {}, { innerText: async () => "  Stored body  " });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: stored } });

    await expect(new WebNewDriver(stub.page).pagesPaneEditorText()).resolves.toBe("Stored body");
  });

  it("reads an empty string when the stored body detaches or reads null", async () => {
    const detached = bodyOf(
      empty(),
      {},
      {
        innerText: async () => {
          throw new Error("detached");
        },
      }
    );
    const nullish = bodyOf(empty(), {}, { innerText: async () => null as unknown as string });
    const detachedDriver = new WebNewDriver(pageDouble({ selectors: { [BODY_SELECTOR]: detached } }).page);
    const nullishDriver = new WebNewDriver(pageDouble({ selectors: { [BODY_SELECTOR]: nullish } }).page);

    await expect(detachedDriver.pagesPaneEditorText()).resolves.toBe("");
    await expect(nullishDriver.pagesPaneEditorText()).resolves.toBe("");
  });

  it("clicks the last paragraph before pressing End", async () => {
    const paragraph = fake();
    const paragraphs = at({ count: 3, last: () => paragraph.asLocator() });
    const editor = editorOf({ p: paragraphs });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor) } });

    await new WebNewDriver(stub.page).pagesPaneEditorClickEnd();

    expect(paragraph.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.press).toHaveBeenCalledWith("End");
  });

  it("clicks the editor itself when it holds no paragraphs", async () => {
    const editor = fake({ locator: () => empty() });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor.asLocator()) } });

    await new WebNewDriver(stub.page).pagesPaneEditorClickEnd();

    expect(editor.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.press).toHaveBeenCalledWith("End");
  });

  it("types and presses Enter through the keyboard", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPaneEditorType("Hello");
    await driver.pagesPaneEditorPressEnter();

    expect(stub.type).toHaveBeenCalledWith("Hello");
    expect(stub.press).toHaveBeenCalledWith("Enter");
  });

  it("reads the canvas scroll position", async () => {
    const scroll = at({ evaluate: async () => 120 });
    const stub = pageDouble({ testIds: { "page-canvas-scroll": scroll } });

    await expect(new WebNewDriver(stub.page).pagesPaneEditorScrollTop()).resolves.toBe(120);
  });

  it("pastes markup through a synthetic paste at the end of the document", async () => {
    const editor = fake({ locator: () => empty() });
    let pasted: unknown;
    editor.evaluate.mockImplementation(async (_fn: unknown, markup: unknown) => {
      pasted = markup;
      return undefined;
    });
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor.asLocator()) } });

    await new WebNewDriver(stub.page).pagesPaneEditorPasteHtml("<h1>Hi</h1>");

    expect(stub.press).toHaveBeenCalledWith("End");
    expect(editor.evaluate).toHaveBeenCalled();
    expect(pasted).toBe("<h1>Hi</h1>");
  });

  it("clicks editor text by its content", async () => {
    const target = fake();
    const seen: unknown[] = [];
    const body = bodyOf(
      at(),
      {},
      {
        getByText: (text: string | RegExp) => {
          seen.push(text);
          return target.asLocator();
        },
      }
    );
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: body } });

    await new WebNewDriver(stub.page).pagesPaneEditorClickText("Intro");

    expect(seen).toEqual(["Intro"]);
    expect(target.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("clears the editor with select-all and Backspace", async () => {
    const editor = editorOf();
    const stub = pageDouble({ selectors: { [BODY_SELECTOR]: bodyOf(editor) } });

    await new WebNewDriver(stub.page).pagesPaneEditorClear();

    expect(stub.press.mock.calls).toEqual([["End"], ["ControlOrMeta+a"], ["Backspace"]]);
  });

  it("deletes an image block before falling back to a plain img", async () => {
    const block = fake();
    const editor = editorOf();
    const withBlock = bodyOf(editor, { "image-component": block.asLocator() });
    const blockStub = pageDouble({ selectors: { [BODY_SELECTOR]: withBlock } });

    await new WebNewDriver(blockStub.page).pagesPaneEditorDeleteImage();

    expect(block.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(blockStub.press).toHaveBeenCalledWith("Backspace");

    const plain = fake();
    const withoutBlock = bodyOf(editor, { "image-component": empty(), img: plain.asLocator() });
    const plainStub = pageDouble({ selectors: { [BODY_SELECTOR]: withoutBlock } });

    await new WebNewDriver(plainStub.page).pagesPaneEditorDeleteImage();

    expect(plain.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(plainStub.press).toHaveBeenCalledWith("Backspace");
  });
});

describe("pane shell", () => {
  it("reports a missing pane as closed", async () => {
    const stub = pageDouble();

    await expect(new WebNewDriver(stub.page).pagesPaneIsOpen()).resolves.toBe(false);
  });

  it("reads the open flag from the pane shell", async () => {
    const shut = paneOf({ getAttribute: async () => "false" });
    const open = paneOf();
    const shutDriver = new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": shut } }).page);
    const openDriver = new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": open } }).page);

    await expect(shutDriver.pagesPaneIsOpen()).resolves.toBe(false);
    await expect(openDriver.pagesPaneIsOpen()).resolves.toBe(true);
  });

  it("opens the pane through the header toggle", async () => {
    const header = fake();
    const stub = pageDouble({
      testIds: { "page-navigation-pane": paneOf(), "page-pane-toggle": header.asLocator() },
    });

    await new WebNewDriver(stub.page).pagesPaneOpenViaToolbar();

    expect(header.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("falls back to the floating toggle when the header control is absent", async () => {
    const floating = fake();
    const stub = pageDouble({
      testIds: { "page-navigation-pane": paneOf(), "page-floating-pane-toggle": floating.asLocator() },
    });

    await new WebNewDriver(stub.page).pagesPaneOpenViaToolbar();

    expect(floating.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("closes the pane through its close control", async () => {
    const close = fake();
    const pane = paneOf({
      getAttribute: async () => "false",
      getByRole: () => close.asLocator(),
    });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await new WebNewDriver(stub.page).pagesPaneClose();

    expect(close.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the pane width from its rendered box", async () => {
    const pane = paneOf({ evaluate: async () => 340 });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await expect(new WebNewDriver(stub.page).pagesPaneWidth()).resolves.toBe(340);
  });

  it("lists no tabs while the pane is closed", async () => {
    const pane = paneOf({ getAttribute: async () => "false" });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await expect(new WebNewDriver(stub.page).pagesPaneTabNames()).resolves.toEqual([]);
  });

  it("lists the tab names trimmed and without blanks", async () => {
    const tabs = at({ allInnerTexts: async () => [" Outline ", "", "Assets "] });
    const pane = paneOf({ getByRole: () => tabs });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await expect(new WebNewDriver(stub.page).pagesPaneTabNames()).resolves.toEqual(["Outline", "Assets"]);
  });

  it("reads the active tab from the shareable query key", async () => {
    const withTab = pageDouble({ url: "https://parity.test/w/p/pages/1?paneTab=info" });
    const withoutTab = pageDouble({ url: "https://parity.test/w/p/pages/1" });

    await expect(new WebNewDriver(withTab.page).pagesPaneActiveTab()).resolves.toBe("info");
    await expect(new WebNewDriver(withoutTab.page).pagesPaneActiveTab()).resolves.toBeNull();
  });

  it("reads any shareable query key", async () => {
    const stub = pageDouble({ url: "https://parity.test/w/p/pages/1?paneTab=info&version=v-2" });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesPaneQueryParam("version")).resolves.toBe("v-2");
    await expect(driver.pagesPaneQueryParam("missing")).resolves.toBeNull();
  });

  it("selects a tab and settles on its selected state", async () => {
    const seen: unknown[] = [];
    const tab = fake({ getAttribute: async () => "true" });
    const pane = paneOf({
      getByRole: (_role: string, options?: { name?: string | RegExp }) => {
        seen.push(options?.name);
        return tab.asLocator();
      },
    });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await new WebNewDriver(stub.page).pagesPaneSelectTab("Assets");

    expect(seen).toEqual(["Assets"]);
    expect(tab.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the canvas width from its rendered box", async () => {
    const canvas = at({ boundingBox: async () => ({ x: 0, y: 0, width: 640, height: 800 }) });
    const stub = pageDouble({ selectors: { "#page-content-container": canvas } });

    await expect(new WebNewDriver(stub.page).pagesPaneCanvasWidth()).resolves.toBe(640);
  });

  it("throws when the canvas has no rendered box", async () => {
    const stub = pageDouble();

    await expect(new WebNewDriver(stub.page).pagesPaneCanvasWidth()).rejects.toThrow(/no rendered box/);
  });

  it("reports whether the pane slides with a transition", async () => {
    const driverFor = (duration: string): WebNewDriver =>
      new WebNewDriver(
        pageDouble({ testIds: { "page-navigation-pane": paneOf({ evaluate: async () => duration }) } }).page
      );

    await expect(driverFor("0s").pagesPaneSlideTransition()).resolves.toBe(false);
    await expect(driverFor("").pagesPaneSlideTransition()).resolves.toBe(false);
    await expect(driverFor("0.25s").pagesPaneSlideTransition()).resolves.toBe(true);
    await expect(driverFor("0s, 0.2s").pagesPaneSlideTransition()).resolves.toBe(true);
  });
});

describe("heading preview", () => {
  function previewPage(button: Locator, card: Locator, pane: Locator = paneOf()): PageDouble {
    const container = at({
      getByRole: () => button,
      locator: (selector: string) => (selector === ":scope > div" ? card : empty()),
    });
    return pageDouble({ testIds: { "page-heading-preview": container, "page-navigation-pane": pane } });
  }

  it("reports the preview missing, hidden, or visible", async () => {
    const missing = new WebNewDriver(pageDouble().page);
    const hidden = new WebNewDriver(previewPage(at({ visible: false }), empty()).page);
    const shown = new WebNewDriver(previewPage(at({ visible: true }), empty()).page);

    await expect(missing.pagesPanePreviewVisible()).resolves.toBe(false);
    await expect(hidden.pagesPanePreviewVisible()).resolves.toBe(false);
    await expect(shown.pagesPanePreviewVisible()).resolves.toBe(true);
  });

  it("counts no bars without a preview button", async () => {
    const stub = previewPage(empty(), empty());

    await expect(new WebNewDriver(stub.page).pagesPanePreviewBarCount()).resolves.toBe(0);
  });

  it("counts the density bars inside the preview button", async () => {
    const button = at({ locator: () => at({ count: 3 }) });
    const stub = previewPage(button, empty());

    await expect(new WebNewDriver(stub.page).pagesPanePreviewBarCount()).resolves.toBe(3);
  });

  it("hovers the preview button", async () => {
    const button = fake();
    const stub = previewPage(button.asLocator(), empty());

    await new WebNewDriver(stub.page).pagesPanePreviewHover();

    expect(button.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the preview card state from its opacity", async () => {
    const absent = new WebNewDriver(previewPage(at(), empty()).page);
    const shut = new WebNewDriver(previewPage(at(), at({ evaluate: async () => "0" })).page);
    const open = new WebNewDriver(previewPage(at(), at({ evaluate: async () => "1" })).page);
    const detached = new WebNewDriver(
      previewPage(
        at(),
        at({
          evaluate: async () => {
            throw new Error("detached");
          },
        })
      ).page
    );

    await expect(absent.pagesPanePreviewCardVisible()).resolves.toBe(false);
    await expect(shut.pagesPanePreviewCardVisible()).resolves.toBe(false);
    await expect(open.pagesPanePreviewCardVisible()).resolves.toBe(true);
    await expect(detached.pagesPanePreviewCardVisible()).resolves.toBe(false);
  });

  it("lists no card headings without a card", async () => {
    const stub = previewPage(at(), empty());

    await expect(new WebNewDriver(stub.page).pagesPanePreviewCardHeadings()).resolves.toEqual([]);
  });

  it("lists the card headings trimmed and without blanks", async () => {
    const card = at({ getByRole: () => at({ allInnerTexts: async () => [" Intro ", "", "Deep "] }) });
    const stub = previewPage(at(), card);

    await expect(new WebNewDriver(stub.page).pagesPanePreviewCardHeadings()).resolves.toEqual(["Intro", "Deep"]);
  });

  it("opens the pane through the preview button", async () => {
    const button = fake();
    const stub = previewPage(button.asLocator(), empty());

    await new WebNewDriver(stub.page).pagesPanePreviewOpen();

    expect(button.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("outline tab", () => {
  function outlineDriver(rows: unknown, present = true): WebNewDriver {
    const list = present ? at({ locator: () => at({ evaluateAll: async () => rows }) }) : empty();
    const pane = paneOf({ getByTestId: (id: string) => (id === "page-outline" ? list : empty()) });
    return new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": pane } }).page);
  }

  it("lists no headings without an outline list", async () => {
    const driver = outlineDriver([], false);

    await expect(driver.pagesPaneOutlineHeadings()).resolves.toEqual([]);
  });

  it("ranks heading levels against the smallest indentation", async () => {
    const driver = outlineDriver([
      { text: "Intro", padding: 16 },
      { text: "", padding: 16 },
      { text: "Deep", padding: 32 },
      { text: "Deeper", padding: 64 },
      { text: "Odd", padding: 24 },
    ]);

    await expect(driver.pagesPaneOutlineHeadings()).resolves.toEqual([
      { level: 1, text: "Intro" },
      { level: 2, text: "Deep" },
      { level: 3, text: "Deeper" },
      { level: null, text: "Odd" },
    ]);
  });

  it("ignores blank rows when finding the indentation baseline", async () => {
    // A blank row at zero padding must not drag every heading to null.
    const driver = outlineDriver([
      { text: "", padding: 0 },
      { text: "H1", padding: 16 },
      { text: "H2", padding: 32 },
    ]);

    await expect(driver.pagesPaneOutlineHeadings()).resolves.toEqual([
      { level: 1, text: "H1" },
      { level: 2, text: "H2" },
    ]);
  });

  it("reads null levels when indentation carries no signal", async () => {
    const flat = outlineDriver([{ text: "A", padding: 0 }]);
    const wild = outlineDriver([
      { text: "A", padding: 16 },
      { text: "B", padding: Number.POSITIVE_INFINITY },
    ]);

    await expect(flat.pagesPaneOutlineHeadings()).resolves.toEqual([{ level: null, text: "A" }]);
    await expect(wild.pagesPaneOutlineHeadings()).resolves.toEqual([
      { level: 1, text: "A" },
      { level: null, text: "B" },
    ]);
  });

  it("clicks an outline entry by its text", async () => {
    const seen: unknown[] = [];
    const target = fake();
    const pane = paneOf({
      getByRole: (_role: string, options?: { name?: string | RegExp }) => {
        seen.push(options?.name);
        return target.asLocator();
      },
    });
    const stub = pageDouble({ testIds: { "page-navigation-pane": pane } });

    await new WebNewDriver(stub.page).pagesPaneOutlineClick("Intro");

    expect(seen).toEqual(["Intro"]);
    expect(target.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reports the outline empty prompt missing, hidden, or visible", async () => {
    const missing = new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": paneOf() } }).page);
    const hidden = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": paneOf({ getByTestId: () => at({ visible: false }) }) } }).page
    );
    const shown = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": paneOf({ getByTestId: () => at({ visible: true }) }) } }).page
    );

    await expect(missing.pagesPaneOutlineEmptyVisible()).resolves.toBe(false);
    await expect(hidden.pagesPaneOutlineEmptyVisible()).resolves.toBe(false);
    await expect(shown.pagesPaneOutlineEmptyVisible()).resolves.toBe(true);
  });
});

describe("assets tab", () => {
  interface AssetSpec {
    name?: string | null | Error;
    thumbnail?: "url" | "none" | "throws" | "absent";
    viewHref?: string | null;
    downloadHref?: string | null;
    downloadOpacity?: boolean | Error | "flaky";
  }

  function assetRow(spec: AssetSpec): { row: Locator; scrolled: FakeLocator } {
    const nameCell = at({
      innerText: async () => {
        if (spec.name instanceof Error) throw spec.name;
        return spec.name as string;
      },
    });
    const thumbnail =
      spec.thumbnail === "absent"
        ? empty()
        : at({
            evaluate: async () => {
              if (spec.thumbnail === "throws") throw new Error("detached");
              return spec.thumbnail === "url" ? "url(blob:shot)" : "none";
            },
          });
    let downloadCalls = 0;
    const download = at({
      count: spec.downloadOpacity === undefined && spec.downloadHref === undefined ? 0 : 1,
      getAttribute: async () => spec.downloadHref ?? null,
      evaluate: async () => {
        if (spec.downloadOpacity instanceof Error) throw spec.downloadOpacity;
        if (spec.downloadOpacity === "flaky") {
          downloadCalls += 1;
          if (downloadCalls === 1) throw new Error("detached");
          return true;
        }
        return spec.downloadOpacity ?? true;
      },
    });
    // The row-level gestures spy on the same double the driver holds.
    const row = new FakeLocator({
      locator: (selector: string) => {
        if (selector === "p") return nameCell;
        if (selector === "div") return thumbnail;
        if (selector === "a[data-asset-download]") return download;
        return empty();
      },
      getAttribute: async (name: string) => (name === "href" ? (spec.viewHref ?? null) : null),
    });
    return { row: row.asLocator(), scrolled: row };
  }

  function assetsDriver(specs: AssetSpec[]): WebNewDriver {
    const rows = specs.map((spec) => assetRow(spec).row);
    const scopes = at({ count: rows.length, nth: (index: number) => rows[index] ?? empty() });
    const pane = paneOf({ getByTestId: (id: string) => (id === "page-asset-row" ? scopes : empty()) });
    return new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": pane } }).page);
  }

  it("lists no rows without asset scopes", async () => {
    const driver = assetsDriver([]);

    await expect(driver.pagesPaneAssetsRows()).resolves.toEqual([]);
  });

  it("lists rows with thumbnails and hrefs, skipping blank names", async () => {
    const driver = assetsDriver([
      { name: "shot.png", thumbnail: "url", viewHref: "/v/1", downloadHref: "/d/1" },
      { name: "   " },
      { name: new Error("detached") },
      { name: null },
      { name: "doc.pdf", thumbnail: "absent", viewHref: null, downloadHref: null },
      { name: "flat.png", thumbnail: "none", viewHref: "/v/5", downloadHref: "/d/5" },
      { name: "gone.png", thumbnail: "throws", viewHref: "/v/6", downloadHref: "/d/6" },
    ]);

    await expect(driver.pagesPaneAssetsRows()).resolves.toEqual([
      { name: "shot.png", thumbnail: true, viewHref: "/v/1", downloadHref: "/d/1" },
      { name: "doc.pdf", thumbnail: false, viewHref: "", downloadHref: "" },
      { name: "flat.png", thumbnail: false, viewHref: "/v/5", downloadHref: "/d/5" },
      { name: "gone.png", thumbnail: false, viewHref: "/v/6", downloadHref: "/d/6" },
    ]);
  });

  it("reports the download action missing when no row matches", async () => {
    const driver = assetsDriver([{ name: new Error("detached") }, { name: "b.png" }]);

    await expect(driver.pagesPaneAssetsDownloadVisible("c.png")).resolves.toBe(false);
  });

  it("reports the download action missing when the matched row has none", async () => {
    const { row, scrolled } = assetRow({ name: "a.png" });
    const scopes = at({ count: 1, nth: () => row });
    const pane = paneOf({ getByTestId: () => scopes });
    const driver = new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": pane } }).page);

    await expect(driver.pagesPaneAssetsDownloadVisible("a.png")).resolves.toBe(false);
    expect(scrolled.scrollIntoViewIfNeeded).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(scrolled.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("hovers a row to reveal its download action", async () => {
    const driver = assetsDriver([{ name: "a.png", downloadHref: "/d/1", downloadOpacity: true }]);

    await expect(driver.pagesPaneAssetsDownloadVisible("a.png")).resolves.toBe(true);
  });

  it("retries the reveal read when the row detaches once", async () => {
    const driver = assetsDriver([{ name: "a.png", downloadHref: "/d/1", downloadOpacity: "flaky" }]);

    await expect(driver.pagesPaneAssetsDownloadVisible("a.png")).resolves.toBe(true);
  });

  it("reports the assets empty prompt missing, hidden, or visible", async () => {
    const missing = new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": paneOf() } }).page);
    const hidden = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": paneOf({ getByTestId: () => at({ visible: false }) }) } }).page
    );
    const shown = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": paneOf({ getByTestId: () => at({ visible: true }) }) } }).page
    );

    await expect(missing.pagesPaneAssetsEmptyVisible()).resolves.toBe(false);
    await expect(hidden.pagesPaneAssetsEmptyVisible()).resolves.toBe(false);
    await expect(shown.pagesPaneAssetsEmptyVisible()).resolves.toBe(true);
  });
});

describe("document statistics", () => {
  function statsDriver(cards: Record<string, string | Error>): WebNewDriver {
    const scope = at({
      locator: (selector: string) => {
        const key = selector.replace(/^\[data-stat="([^"]+)"\] h6$/, "$1");
        const value = cards[key];
        if (value === undefined) return empty();
        return at({
          innerText: async () => {
            if (value instanceof Error) throw value;
            return value;
          },
        });
      },
    });
    const pane = paneOf({ getByTestId: (id: string) => (id === "page-document-stats" ? scope : empty()) });
    return new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": pane } }).page);
  }

  it("reads the live counts and the read time", async () => {
    const driver = statsDriver({ words: "12", characters: "345", paragraphs: "6", "read-time": "1 min read" });

    await expect(driver.pagesPaneStats()).resolves.toEqual({
      words: 12,
      characters: 345,
      paragraphs: 6,
      readTime: "1 min read",
    });
  });

  it("throws when any count card is non-numeric", async () => {
    const badWords = statsDriver({ words: "many", characters: "345", paragraphs: "6", "read-time": "1 min read" });
    const badChars = statsDriver({
      words: "12",
      characters: "many",
      paragraphs: "6",
      "read-time": new Error("detached"),
    });
    const badParas = statsDriver({ words: "12", characters: "345", paragraphs: "many", "read-time": "1 min read" });

    await expect(badWords.pagesPaneStats()).rejects.toThrow(/non-numeric/);
    await expect(badChars.pagesPaneStats()).rejects.toThrow(/non-numeric/);
    await expect(badParas.pagesPaneStats()).rejects.toThrow(/non-numeric/);
  });

  it("reads an empty read time when its card detaches", async () => {
    const driver = statsDriver({
      words: "12",
      characters: "345",
      paragraphs: "6",
      "read-time": new Error("detached"),
    });

    await expect(driver.pagesPaneStats()).resolves.toEqual({
      words: 12,
      characters: 345,
      paragraphs: 6,
      readTime: "",
    });
  });
});

describe("actor rows", () => {
  interface ActorSpec {
    name?: string | null | Error;
    href?: string | null;
    time?: string | null | Error;
  }

  function actorRow(spec: ActorSpec): Locator {
    const link = at({
      innerText: async () => {
        if (spec.name instanceof Error) throw spec.name;
        return spec.name as string;
      },
      getAttribute: async () => spec.href ?? null,
    });
    const time = at({
      innerText: async () => {
        if (spec.time instanceof Error) throw spec.time;
        return spec.time as string;
      },
    });
    const identity = at({
      getByRole: () => link,
      locator: (selector: string) => (selector === "span" ? time : empty()),
    });
    return at({ locator: () => identity });
  }

  function actorsDriver(specs: [ActorSpec, ActorSpec]): WebNewDriver {
    const rows = specs.map((spec) => actorRow(spec));
    const scope = at({
      locator: (selector: string) =>
        selector === ":scope > div" ? at({ count: 2, nth: (index: number) => rows[index] ?? empty() }) : empty(),
    });
    const pane = paneOf({ getByTestId: (id: string) => (id === "page-actors" ? scope : empty()) });
    return new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": pane } }).page);
  }

  it("reads the editor and creator rows with profile links", async () => {
    const driver = actorsDriver([
      { name: "Ada", href: "/profile/ada", time: "2 hours ago" },
      { name: "Grace", href: "/profile/grace", time: "Jan 3, 2026" },
    ]);

    await expect(driver.pagesPaneActors()).resolves.toEqual([
      { role: "edited", name: "Ada", profileHref: "/profile/ada", timeText: "2 hours ago" },
      { role: "created", name: "Grace", profileHref: "/profile/grace", timeText: "Jan 3, 2026" },
    ]);
  });

  it("falls back to blanks when rows detach or read null", async () => {
    const driver = actorsDriver([
      { name: new Error("detached"), href: null, time: null },
      { name: null, href: "/profile/grace", time: new Error("detached") },
    ]);

    await expect(driver.pagesPaneActors()).resolves.toEqual([
      { role: "edited", name: "", profileHref: "", timeText: "" },
      { role: "created", name: "", profileHref: "/profile/grace", timeText: "" },
    ]);
  });
});

describe("version timeline", () => {
  interface EntrySpec {
    link?: boolean;
    id?: string | null;
    active?: boolean;
    label?: string | null | Error;
    author?: string | null | Error;
  }

  function timelineDouble(specs: EntrySpec[]): { timeline: Locator; selected: FakeLocator } {
    const items = specs.map((spec) => {
      if (spec.link === false) return at({ getByRole: () => empty() });
      const label = at({
        innerText: async () => {
          if (spec.label instanceof Error) throw spec.label;
          return spec.label as string;
        },
      });
      const author = at({
        innerText: async () => {
          if (spec.author instanceof Error) throw spec.author;
          return spec.author as string;
        },
      });
      const link = fake({
        getAttribute: async (name: string) => {
          if (name === "data-version-id") return spec.id ?? null;
          if (name === "aria-current") return spec.active === true ? "true" : null;
          return null;
        },
        locator: (selector: string) => {
          if (selector === "p") return label;
          if (selector === "span") return author;
          return empty();
        },
      });
      return at({ getByRole: () => link.asLocator() });
    });
    const list = at({
      locator: (selector: string) =>
        selector === ":scope > li"
          ? at({ count: items.length, nth: (index: number) => items[index] ?? empty() })
          : empty(),
    });
    const selected = fake();
    const timeline = at({
      locator: (selector: string) => {
        if (selector === "ul") return list;
        return selected.asLocator();
      },
    });
    return { timeline, selected };
  }

  function timelineDriver(
    specs: EntrySpec[],
    overlay: Locator = empty()
  ): { driver: WebNewDriver; selected: FakeLocator } {
    const { timeline, selected } = timelineDouble(specs);
    const stub = pageDouble({
      roles: { region: timeline },
      testIds: { "page-version-overlay": overlay },
    });
    return { driver: new WebNewDriver(stub.page), selected };
  }

  it("lists no entries without timeline items", async () => {
    const { driver } = timelineDriver([]);

    await expect(driver.pagesPaneVersionEntries()).resolves.toEqual([]);
  });

  it("lists the current row plus snapshots with authors", async () => {
    const { driver } = timelineDriver([
      { link: false },
      { id: null, active: true, label: "Current version" },
      { id: "v-9", active: false, label: " Oct 25, 2026 ", author: " Ada " },
      { id: "v-8", label: new Error("detached"), author: new Error("detached") },
      { id: "v-7", label: null, author: null },
    ]);

    await expect(driver.pagesPaneVersionEntries()).resolves.toEqual([
      { id: null, label: "Current version", author: "", active: true },
      { id: "v-9", label: "Oct 25, 2026", author: "Ada", active: false },
      { id: "v-8", label: "", author: "", active: false },
      { id: "v-7", label: "", author: "", active: false },
    ]);
  });

  it("selects a version and settles on the open overlay", async () => {
    const { driver, selected } = timelineDriver([], at());

    await driver.pagesPaneSelectVersion("v-9");

    expect(selected.click).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesPaneOverlayOpen()).resolves.toBe(true);
  });

  it("selects the current version and settles on the closed overlay", async () => {
    const { driver, selected } = timelineDriver([], empty());

    await driver.pagesPaneSelectCurrentVersion();

    expect(selected.click).toHaveBeenCalledWith({ timeout: 30_000 });
    await expect(driver.pagesPaneOverlayOpen()).resolves.toBe(false);
  });
});

describe("version overlay", () => {
  interface OverlaySpec {
    present?: boolean;
    title?: string | Error;
    marker?: "visible" | "hidden" | "absent";
    body?: string | null | Error | "absent";
    skeleton?: "visible" | "hidden" | "absent";
    error?: "visible" | "hidden" | "absent";
    restore?: "visible" | "hidden" | "absent";
    heading?: string | Error;
    errorBody?: string | "absent";
    retry?: string | "absent";
  }

  function overlayDouble(spec: OverlaySpec, restoreClick?: FakeLocator): Locator {
    const title = at({
      innerText: async () => {
        if (spec.title instanceof Error) throw spec.title;
        return (spec.title ?? "") as string;
      },
    });
    const marker =
      spec.marker === "absent" || spec.marker === undefined ? empty() : at({ visible: spec.marker === "visible" });
    const body =
      spec.body === "absent" || spec.body === undefined
        ? empty()
        : at({
            innerText: async () => {
              if (spec.body instanceof Error) throw spec.body;
              return spec.body as string;
            },
          });
    const skeleton =
      spec.skeleton === "absent" || spec.skeleton === undefined
        ? empty()
        : at({ visible: spec.skeleton === "visible" });
    const errorShown = spec.error === "visible";
    const error =
      spec.error === "absent" || spec.error === undefined
        ? empty()
        : at({
            visible: errorShown,
            locator: (selector: string) => {
              if (selector === "h2") {
                return at({
                  innerText: async () => {
                    if (spec.heading instanceof Error) throw spec.heading;
                    return (spec.heading ?? "") as string;
                  },
                });
              }
              if (selector === "p") {
                if (spec.errorBody === "absent" || spec.errorBody === undefined) return empty();
                return at({ innerText: async () => spec.errorBody as string });
              }
              return empty();
            },
            getByRole: () => {
              if (spec.retry === "absent" || spec.retry === undefined) return empty();
              return at({ innerText: async () => spec.retry as string });
            },
          });
    const restoreShown =
      spec.restore === "absent" || spec.restore === undefined
        ? empty()
        : restoreClick !== undefined
          ? restoreClick.asLocator()
          : at({ visible: spec.restore === "visible" });
    return at({
      count: spec.present === false ? 0 : 1,
      locator: (selector: string) => (selector === "h6" ? title : empty()),
      getByRole: (role: string) => {
        if (role === "region") return body;
        return restoreShown;
      },
      getByTestId: (id: string) => {
        if (id === "page-version-skeleton") return skeleton;
        if (id === "page-version-error") return error;
        return empty();
      },
      getByText: () => marker,
    });
  }

  function overlayDriver(spec: OverlaySpec): WebNewDriver {
    return new WebNewDriver(pageDouble({ testIds: { "page-version-overlay": overlayDouble(spec) } }).page);
  }

  it("reports the overlay closed or open", async () => {
    const closed = new WebNewDriver(pageDouble().page);
    const open = overlayDriver({});

    await expect(closed.pagesPaneOverlayOpen()).resolves.toBe(false);
    await expect(open.pagesPaneOverlayOpen()).resolves.toBe(true);
  });

  it("reads an empty header without an overlay", async () => {
    const driver = new WebNewDriver(pageDouble().page);

    await expect(driver.pagesPaneOverlayHeader()).resolves.toEqual({ title: "", viewOnly: false });
  });

  it("reads the snapshot title and the read-only marker", async () => {
    const marked = overlayDriver({ title: " Snapshot · Oct 25 ", marker: "visible" });
    const hidden = overlayDriver({ title: "Snapshot", marker: "hidden" });
    const plain = overlayDriver({ title: "Snapshot", marker: "absent" });
    const detached = overlayDriver({ title: new Error("detached"), marker: "absent" });

    await expect(marked.pagesPaneOverlayHeader()).resolves.toEqual({ title: "Snapshot · Oct 25", viewOnly: true });
    await expect(hidden.pagesPaneOverlayHeader()).resolves.toEqual({ title: "Snapshot", viewOnly: false });
    await expect(plain.pagesPaneOverlayHeader()).resolves.toEqual({ title: "Snapshot", viewOnly: false });
    await expect(detached.pagesPaneOverlayHeader()).resolves.toEqual({ title: "", viewOnly: false });
  });

  it("reads the frozen overlay body", async () => {
    const absent = overlayDriver({});
    const frozen = overlayDriver({ body: " frozen body " });
    const detached = overlayDriver({ body: new Error("detached") });
    const nullish = overlayDriver({ body: null });

    await expect(absent.pagesPaneOverlayText()).resolves.toBe("");
    await expect(frozen.pagesPaneOverlayText()).resolves.toBe("frozen body");
    await expect(detached.pagesPaneOverlayText()).resolves.toBe("");
    await expect(nullish.pagesPaneOverlayText()).resolves.toBe("");
  });

  it("hides the skeleton once the error panel shows", async () => {
    const driver = overlayDriver({ error: "visible", skeleton: "visible" });

    await expect(driver.pagesPaneOverlaySkeletonVisible()).resolves.toBe(false);
  });

  it("reads the skeleton state while loading", async () => {
    const absent = overlayDriver({});
    const hidden = overlayDriver({ skeleton: "hidden" });
    const shown = overlayDriver({ skeleton: "visible" });

    await expect(absent.pagesPaneOverlaySkeletonVisible()).resolves.toBe(false);
    await expect(hidden.pagesPaneOverlaySkeletonVisible()).resolves.toBe(false);
    await expect(shown.pagesPaneOverlaySkeletonVisible()).resolves.toBe(true);
  });

  it("gates the restore control on the overlay, the error panel, and permission", async () => {
    const closed = new WebNewDriver(pageDouble().page);
    const failed = overlayDriver({ error: "visible", restore: "visible" });
    const forbidden = overlayDriver({ restore: "absent" });
    const hidden = overlayDriver({ restore: "hidden" });
    const allowed = overlayDriver({ restore: "visible" });

    await expect(closed.pagesPaneRestoreVisible()).resolves.toBe(false);
    await expect(failed.pagesPaneRestoreVisible()).resolves.toBe(false);
    await expect(forbidden.pagesPaneRestoreVisible()).resolves.toBe(false);
    await expect(hidden.pagesPaneRestoreVisible()).resolves.toBe(false);
    await expect(allowed.pagesPaneRestoreVisible()).resolves.toBe(true);
  });

  it("restores through the overlay control and settles on its close", async () => {
    const restore = fake();
    const stub = pageDouble({
      testIds: { "page-version-overlay": overlayDouble({ present: false, restore: "visible" }, restore) },
    });

    await new WebNewDriver(stub.page).pagesPaneRestore();

    expect(restore.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("settles a restore on the failure notice when the overlay stays open", async () => {
    const restore = fake();
    const overlay = overlayDouble({ restore: "visible" }, restore);
    const card = at({ innerText: async () => "Restore failed\nSnapshot is stale" });
    const cards = at({ count: 1, nth: () => card });
    const stub = pageDouble({
      testIds: { "page-version-overlay": overlay },
      selectors: { '[data-testid="toast-notification"]': cards },
    });

    await new WebNewDriver(stub.page).pagesPaneRestore();

    expect(restore.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("keeps polling a restore until the overlay closes", async () => {
    const restore = fake();
    let reads = 0;
    const overlay = new FakeLocator({
      count: () => {
        reads += 1;
        return reads === 1 ? 1 : 0;
      },
      getByRole: () => restore.asLocator(),
      getByTestId: () => empty(),
      locator: () => empty(),
      getByText: () => empty(),
    });
    const stub = pageDouble({ testIds: { "page-version-overlay": overlay.asLocator() } });

    await new WebNewDriver(stub.page).pagesPaneRestore();

    expect(restore.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(reads).toBeGreaterThan(1);
  });

  it("reports the error panel missing, hidden, or visible", async () => {
    const missing = overlayDriver({});
    const hidden = overlayDriver({ error: "hidden" });
    const shown = overlayDriver({ error: "visible" });

    await expect(missing.pagesPaneOverlayErrorVisible()).resolves.toBe(false);
    await expect(hidden.pagesPaneOverlayErrorVisible()).resolves.toBe(false);
    await expect(shown.pagesPaneOverlayErrorVisible()).resolves.toBe(true);
  });

  it("reads null error texts without an error panel", async () => {
    const driver = overlayDriver({});

    await expect(driver.pagesPaneOverlayErrorTexts()).resolves.toEqual({ heading: null, body: null, retry: null });
  });

  it("reads the full error panel copy", async () => {
    const driver = overlayDriver({
      error: "visible",
      heading: "Version failed to load",
      errorBody: "Check your connection and try again",
      retry: "Retry",
    });

    await expect(driver.pagesPaneOverlayErrorTexts()).resolves.toEqual({
      heading: "Version failed to load",
      body: "Check your connection and try again",
      retry: "Retry",
    });
  });

  it("reads nulls when error rows detach or go blank", async () => {
    const driver = overlayDriver({
      error: "visible",
      heading: new Error("detached"),
      errorBody: "absent",
      retry: "  ",
    });

    await expect(driver.pagesPaneOverlayErrorTexts()).resolves.toEqual({ heading: null, body: null, retry: null });
  });

  it("reads nulls when the body is blank and no retry renders", async () => {
    const driver = overlayDriver({ error: "visible", heading: "Failed", errorBody: " ", retry: "absent" });

    await expect(driver.pagesPaneOverlayErrorTexts()).resolves.toEqual({ heading: "Failed", body: null, retry: null });
  });

  it("retries the failed version load", async () => {
    const retry = fake();
    const error = at({ getByRole: () => retry.asLocator() });
    const overlay = at({ getByTestId: (id: string) => (id === "page-version-error" ? error : empty()) });
    const stub = pageDouble({ testIds: { "page-version-overlay": overlay } });

    await new WebNewDriver(stub.page).pagesPaneOverlayRetry();

    expect(retry.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("version-detail route spy", () => {
  type RouteHandler = (route: Route) => Promise<void>;

  function spiedDriver(): {
    driver: WebNewDriver;
    stub: PageDouble;
    handler: () => RouteHandler;
    pattern: () => RegExp;
  } {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    return {
      driver,
      stub,
      handler: () => stub.route.mock.calls[0]?.[1] as RouteHandler,
      pattern: () => stub.route.mock.calls[0]?.[0] as RegExp,
    };
  }

  it("installs the spy once and matches version-detail addresses only", async () => {
    const { driver, stub, pattern } = spiedDriver();

    await driver.pagesPaneOverlayStallDetail();
    await driver.pagesPaneFailVersionDetail();

    expect(stub.route).toHaveBeenCalledTimes(1);
    expect(pattern().test("/api/workspaces/acme/projects/p1/pages/pg1/versions/v9/")).toBe(true);
    expect(pattern().test("/api/workspaces/acme/projects/p1/pages/pg1/versions/")).toBe(false);
  });

  it("passes non-GET detail requests through without counting them", async () => {
    const { driver, handler } = spiedDriver();
    await driver.pagesPaneOverlayStallDetail();
    const { route, fallback, fulfill } = routeDouble("POST");

    await handler()(route);

    expect(fallback).toHaveBeenCalledTimes(1);
    expect(fulfill).not.toHaveBeenCalled();
    await expect(driver.pagesPaneVersionDetailRequests()).resolves.toBe(0);
  });

  it("fails detail requests with a 500 while the failure is armed", async () => {
    const { driver, handler } = spiedDriver();
    await driver.pagesPaneFailVersionDetail();
    const { route, fallback, fulfill } = routeDouble();

    await handler()(route);

    expect(fulfill).toHaveBeenCalledWith({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ detail: "parity-forced-version-failure" }),
    });
    expect(fallback).not.toHaveBeenCalled();
    await expect(driver.pagesPaneVersionDetailRequests()).resolves.toBe(1);
  });

  it("passes detail requests through once the failure is lifted", async () => {
    const { driver, handler } = spiedDriver();
    await driver.pagesPaneFailVersionDetail();
    await driver.pagesPaneUnfailVersionDetail();
    const { route, fallback, fulfill } = routeDouble();

    await handler()(route);

    expect(fallback).toHaveBeenCalledTimes(1);
    expect(fulfill).not.toHaveBeenCalled();
    await expect(driver.pagesPaneVersionDetailRequests()).resolves.toBe(1);
  });

  it("holds stalled detail requests until the test releases them", async () => {
    const { driver, handler } = spiedDriver();
    await driver.pagesPaneOverlayStallDetail();
    const { route, fallback } = routeDouble();

    const held = handler()(route);
    await Promise.resolve();
    expect(fallback).not.toHaveBeenCalled();
    await expect(driver.pagesPaneVersionDetailRequests()).resolves.toBe(1);

    await driver.pagesPaneOverlayReleaseDetail();
    await held;

    expect(fallback).toHaveBeenCalledTimes(1);
  });

  it("counts no detail requests before the spy runs", async () => {
    const { driver } = spiedDriver();

    await expect(driver.pagesPaneVersionDetailRequests()).resolves.toBe(0);
  });
});

describe("workspace member filter", () => {
  type RouteHandler = (route: Route) => Promise<void>;

  function filteredDriver(): { driver: WebNewDriver; stub: PageDouble; handlers: () => RouteHandler[] } {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    return {
      driver,
      stub,
      handlers: () => stub.route.mock.calls.map((call) => call[1] as RouteHandler),
    };
  }

  function memberAnswer(payload: unknown, status = 200): { status: () => number; json: () => Promise<unknown> } {
    return { status: () => status, json: async () => payload };
  }

  it("installs the member filter on both directory addresses once", async () => {
    const { driver, stub } = filteredDriver();

    await driver.pagesPaneOmitWorkspaceMember("u-1");
    await driver.pagesPaneOmitWorkspaceMember("u-2");

    expect(stub.route).toHaveBeenCalledTimes(2);
    const patterns = stub.route.mock.calls.map((call) => String(call[0]));
    expect(patterns[0]).toContain("members");
    expect(patterns[1]).toContain("members");
  });

  it("passes non-GET member requests through", async () => {
    const { driver, handlers } = filteredDriver();
    await driver.pagesPaneOmitWorkspaceMember("u-1");
    const { route, fallback, fetch } = routeDouble("POST");

    await handlers()[0]?.(route);

    expect(fallback).toHaveBeenCalledTimes(1);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("passes member requests through once the omission is lifted", async () => {
    const { driver, handlers } = filteredDriver();
    await driver.pagesPaneOmitWorkspaceMember("u-1");
    await driver.pagesPaneRestoreWorkspaceMembers();
    const { route, fallback, fetch } = routeDouble();

    await handlers()[0]?.(route);

    expect(fallback).toHaveBeenCalledTimes(1);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("omits the staged member from an array directory answer", async () => {
    const { driver, handlers } = filteredDriver();
    await driver.pagesPaneOmitWorkspaceMember("u-1");
    const { route, fallback, fulfill, fetch } = routeDouble();
    fetch.mockResolvedValue(
      memberAnswer([
        { id: "u-1", name: "Ada" },
        { id: "u-2", name: "Grace" },
      ])
    );

    await handlers()[0]?.(route);

    expect(fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify([{ id: "u-2", name: "Grace" }]),
    });
    expect(fallback).not.toHaveBeenCalled();
  });

  it("omits the staged member from a paged directory answer", async () => {
    const { driver, handlers } = filteredDriver();
    await driver.pagesPaneOmitWorkspaceMember("u-1");
    const watched = routeDouble();
    watched.fetch.mockResolvedValue(
      memberAnswer({
        results: [{ member: { id: "u-1" } }, { member: { id: "u-9" }, id: "u-1" }, { id: "u-2" }],
        total: 3,
      })
    );

    await handlers()[1]?.(watched.route);

    expect(watched.fulfill).toHaveBeenCalledWith({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ results: [{ id: "u-2" }], total: 3 }),
    });
    expect(watched.fallback).not.toHaveBeenCalled();
  });

  it("passes the answer through when it is not JSON or not a list", async () => {
    const { driver, handlers } = filteredDriver();
    await driver.pagesPaneOmitWorkspaceMember("u-1");

    const broken = routeDouble();
    broken.fetch.mockResolvedValue({
      status: () => 200,
      json: async () => {
        throw new Error("not json");
      },
    });
    await handlers()[0]?.(broken.route);
    expect(broken.fallback).toHaveBeenCalledTimes(1);
    expect(broken.fulfill).not.toHaveBeenCalled();

    const shaped = routeDouble();
    shaped.fetch.mockResolvedValue(memberAnswer({ results: "nope" }));
    await handlers()[0]?.(shaped.route);
    expect(shaped.fallback).toHaveBeenCalledTimes(1);
    expect(shaped.fulfill).not.toHaveBeenCalled();
  });
});

describe("profile roundtrip", () => {
  function roundtripPage(warm: () => Promise<unknown>): { stub: PageDouble; link: FakeLocator; editor: FakeLocator } {
    const link = fake();
    const identity = at({ getByRole: () => link.asLocator() });
    const scope = at({ locator: () => at({ count: 2, nth: () => identity }) });
    const pane = paneOf({ getByTestId: (id: string) => (id === "page-actors" ? scope : empty()) });
    const editor = fake();
    const body = bodyOf(editor.asLocator());
    let evaluations = 0;
    const stub = pageDouble({
      testIds: { "page-navigation-pane": pane },
      selectors: { [BODY_SELECTOR]: body },
      evaluate: async () => {
        evaluations += 1;
        if (evaluations === 1) return undefined;
        return warm();
      },
    });
    return { stub, link, editor };
  }

  it("visits the creator profile and returns to a warm detail", async () => {
    const { stub, link, editor } = roundtripPage(async () => "alive");
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPaneProfileRoundtrip();

    expect(link.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.waitForURL).toHaveBeenCalledTimes(1);
    const predicate = stub.waitForURL.mock.calls[0]?.[0] as (url: URL) => boolean;
    expect(predicate(new URL("https://parity.test/profile/u-9"))).toBe(true);
    expect(predicate(new URL("https://parity.test/other"))).toBe(false);
    expect(stub.goBack).toHaveBeenCalledTimes(1);
    expect(editor.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 90_000 });
  });

  it("throws when the roundtrip remounts the detail cold", async () => {
    const { stub } = roundtripPage(async () => undefined);
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesPaneProfileRoundtrip()).rejects.toThrow(/remounted cold/);
  });

  it("throws when the mount marker cannot be read", async () => {
    const { stub } = roundtripPage(async () => {
      throw new Error("detached");
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesPaneProfileRoundtrip()).rejects.toThrow(/remounted cold/);
  });
});
