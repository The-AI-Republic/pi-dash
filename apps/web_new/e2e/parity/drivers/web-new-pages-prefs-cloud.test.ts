// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver page prefs/key map/assistant/cloud/
// desktop/negative probes (NEWFRONT-925, PAGE-072-073, 075-078, 081-083).
// A Page double proves the addresses, selectors, request watch and local
// storage shape; no browser or parity stack is involved.
import type { Locator, Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => vi.unstubAllGlobals());

interface LocatorOptions {
  count?: number;
  visible?: boolean;
  innerText?: () => Promise<string>;
  innerHTML?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: { state?: string; timeout?: number }) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  inputValue?: (options?: unknown) => Promise<string>;
  locator?: (selector: string) => Locator;
  getByRole?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
  evaluate?: (fn: (node: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  last?: () => Locator;
  isVisible?: () => Promise<boolean>;
}

class FakeLocator {
  readonly waitFor: ReturnType<typeof vi.fn>;
  readonly click: ReturnType<typeof vi.fn>;
  readonly hover: ReturnType<typeof vi.fn>;
  readonly fill: ReturnType<typeof vi.fn>;
  readonly press: ReturnType<typeof vi.fn>;
  readonly inputValue: ReturnType<typeof vi.fn>;
  readonly scrollIntoViewIfNeeded: ReturnType<typeof vi.fn>;
  readonly evaluate: ReturnType<typeof vi.fn>;
  readonly isVisible: ReturnType<typeof vi.fn>;

  constructor(private readonly options: LocatorOptions = {}) {
    this.waitFor = vi.fn(this.options.waitFor ?? (async () => undefined));
    this.click = vi.fn(this.options.click ?? (async () => undefined));
    this.hover = vi.fn(this.options.hover ?? (async () => undefined));
    this.fill = vi.fn(this.options.fill ?? (async () => undefined));
    this.press = vi.fn(this.options.press ?? (async () => undefined));
    this.inputValue = vi.fn(this.options.inputValue ?? (async () => ""));
    this.scrollIntoViewIfNeeded = vi.fn(this.options.scrollIntoViewIfNeeded ?? (async () => undefined));
    this.evaluate = vi.fn(
      this.options.evaluate ?? (async (fn: (node: unknown) => unknown) => fn(undefined))
    );
    this.isVisible = vi.fn(this.options.isVisible ?? (async () => this.options.visible ?? (this.options.count ?? 0) > 0));
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.count ?? 0);
  }

  innerText(): Promise<string> {
    return (this.options.innerText ?? (async () => ""))();
  }

  innerHTML(): Promise<string> {
    return (this.options.innerHTML ?? (async () => ""))();
  }

  allInnerTexts(): Promise<string[]> {
    return (this.options.allInnerTexts ?? (async () => []))();
  }

  getAttribute(name: string): Promise<string | null> {
    return (this.options.getAttribute ?? (async () => null))(name);
  }

  locator(selector: string): Locator {
    return this.options.locator?.(selector) ?? empty();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): Locator {
    return this.options.getByRole?.(role, options) ?? empty();
  }

  filter(options: unknown): Locator {
    return this.options.filter?.(options) ?? this.asLocator();
  }

  first(): Locator {
    return this.options.first?.() ?? this.asLocator();
  }

  nth(index: number): Locator {
    return this.options.nth?.(index) ?? empty();
  }

  last(): Locator {
    return this.options.last?.() ?? this.options.nth?.(Math.max(0, (this.options.count ?? 1) - 1)) ?? this.asLocator();
  }

  private asLocator(): Locator {
    return this as unknown as Locator;
  }
}

function empty(): Locator {
  return new FakeLocator({ count: 0 }) as unknown as Locator;
}

function at(options: LocatorOptions = {}): Locator {
  return new FakeLocator({ count: 1, ...options }) as unknown as Locator;
}

interface PageOptions {
  testIds?: Record<string, Locator>;
  roles?: Record<string, Locator>;
  texts?: Array<{ pattern: RegExp; value: Locator }>;
  selectors?: Record<string, Locator>;
  evaluate?: (fn: (arg: unknown) => unknown, arg?: unknown) => Promise<unknown>;
}

interface PageDouble {
  page: Page;
  goto: ReturnType<typeof vi.fn>;
  reload: ReturnType<typeof vi.fn>;
  route: ReturnType<typeof vi.fn>;
  type: ReturnType<typeof vi.fn>;
  press: ReturnType<typeof vi.fn>;
  waitForTimeout: ReturnType<typeof vi.fn>;
}

function pageDouble(options: PageOptions = {}): PageDouble {
  const goto = vi.fn(async () => undefined);
  const reload = vi.fn(async () => undefined);
  const route = vi.fn(async () => undefined);
  const type = vi.fn(async () => undefined);
  const press = vi.fn(async () => undefined);
  const waitForTimeout = vi.fn(async () => undefined);
  const page = {
    goto,
    reload,
    route,
    waitForTimeout,
    keyboard: { type, press },
    getByTestId: vi.fn((id: string) => options.testIds?.[id] ?? empty()),
    getByRole: vi.fn((role: string) => options.roles?.[role] ?? empty()),
    getByText: vi.fn((pattern: RegExp) =>
      options.texts?.find((entry) => entry.pattern.source === pattern.source)?.value ?? empty()
    ),
    locator: vi.fn((selector: string) => options.selectors?.[selector] ?? empty()),
    evaluate: vi.fn(
      options.evaluate ?? (async (fn: (arg: unknown) => unknown, arg?: unknown) => fn(arg))
    ),
  } as unknown as Page;
  return { page, goto, reload, route, type, press, waitForTimeout };
}

function menuOf(labels: Array<string | null>): Locator {
  return at({
    count: labels.length,
    nth: (index) =>
      at({
        innerText:
          labels[index] === null
            ? async () => {
                throw new Error("menu item detached");
              }
            : async () => labels[index] ?? "",
      }),
  });
}

function bodyEditor(options: LocatorOptions = {}): Locator {
  return at(options);
}

describe("page navigation and editor primitives", () => {
  it("opens the list and settles on either the list or its empty state", async () => {
    const list = at();
    const emptyState = at();
    const stub = pageDouble({ testIds: { "pages-list-screen": list, "pages-empty-state": emptyState } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPrefsOpenList("acme", "project-1");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages", { waitUntil: "domcontentloaded" });
    expect(list.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
    expect(emptyState.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
  });

  it("opens a detail page and waits for the header, title and editable body", async () => {
    const header = at();
    const title = at();
    const editor = bodyEditor();
    const stub = pageDouble({
      testIds: { "page-detail-header": header, "page-detail-title": title },
      selectors: { '[aria-label="Page body"][contenteditable="true"]': editor },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPrefsOpenDetail("acme", "project-1", "page-1");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1", {
      waitUntil: "domcontentloaded",
    });
    expect(header.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
    expect(title.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
    expect(editor.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 60_000 });
  });

  it("reloads into either a detail or a list route", async () => {
    const header = at();
    const title = at();
    const list = at();
    const stub = pageDouble({
      testIds: { "page-detail-header": header, "page-detail-title": title, "pages-list-screen": list },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPrefsReload();

    expect(stub.reload).toHaveBeenCalledWith({ waitUntil: "domcontentloaded" });
    expect(header.waitFor).toHaveBeenCalled();
    expect(list.waitFor).toHaveBeenCalled();
  });

  it("reads editor markup and text, including an absent or detached body", async () => {
    const detached = bodyEditor({
      innerHTML: async () => {
        throw new Error("detached");
      },
      innerText: async () => {
        throw new Error("detached");
      },
    });
    const present = bodyEditor({
      innerHTML: async () => "<p>Hello</p>",
      innerText: async () => "  Hello world  ",
    });

    const absentDriver = new WebNewDriver(pageDouble().page);
    await expect(absentDriver.pagesPrefsEditorHtml()).resolves.toBe("");
    await expect(absentDriver.pagesPrefsEditorText()).resolves.toBe("");

    const detachedDriver = new WebNewDriver(
      pageDouble({ selectors: { '[aria-label="Page body"][contenteditable="true"]': detached } }).page
    );
    await expect(detachedDriver.pagesPrefsEditorHtml()).resolves.toBe("");
    await expect(detachedDriver.pagesPrefsEditorText()).resolves.toBe("");

    const presentDriver = new WebNewDriver(
      pageDouble({ selectors: { '[aria-label="Page body"][contenteditable="true"]': present } }).page
    );
    await expect(presentDriver.pagesPrefsEditorHtml()).resolves.toBe("<p>Hello</p>");
    await expect(presentDriver.pagesPrefsEditorText()).resolves.toBe("Hello world");

    const nullish = bodyEditor({
      innerHTML: async () => null as unknown as string,
      innerText: async () => null as unknown as string,
    });
    const nullishDriver = new WebNewDriver(
      pageDouble({ selectors: { '[aria-label="Page body"][contenteditable="true"]': nullish } }).page
    );
    await expect(nullishDriver.pagesPrefsEditorHtml()).resolves.toBe("");
    await expect(nullishDriver.pagesPrefsEditorText()).resolves.toBe("");
  });

  it("clicks the last paragraph when present and the editor otherwise", async () => {
    const lastParagraph = at();
    const paragraphs = at({ count: 1, last: () => lastParagraph });
    const withParagraphs = bodyEditor({ locator: (selector) => (selector === "p" ? paragraphs : empty()) });
    const first = pageDouble({
      selectors: { '[aria-label="Page body"][contenteditable="true"]': withParagraphs },
    });
    const driver = new WebNewDriver(first.page);

    await driver.pagesPrefsEditorClickEnd();

    expect(withParagraphs.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    expect(lastParagraph.click).toHaveBeenCalledWith({ timeout: 30_000, position: { x: 4, y: 4 } });

    const bare = bodyEditor({ locator: () => at({ count: 0 }) });
    const second = pageDouble({ selectors: { '[aria-label="Page body"][contenteditable="true"]': bare } });
    await new WebNewDriver(second.page).pagesPrefsEditorClickEnd();
    expect(bare.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("types, clears and selects all through the focused editor", async () => {
    const editor = bodyEditor({ locator: () => at({ count: 0 }) });
    const stub = pageDouble({ selectors: { '[aria-label="Page body"][contenteditable="true"]': editor } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPrefsEditorType("typed text");
    await driver.pagesPrefsEditorClear();
    await driver.pagesPrefsEditorSelectAll();

    expect(stub.type).toHaveBeenCalledWith("typed text");
    expect(stub.press).toHaveBeenCalledWith("Control+a");
    expect(stub.press).toHaveBeenCalledWith("Backspace");
    expect(editor.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads, fills, focuses and submits the title field", async () => {
    const field = at({ innerText: async () => "  Page name  " });
    // The wrapper is read directly for its class, while the title field is
    // reached through the same test id, so return a wrapper that chains.
    const stub = pageDouble();
    const driver = new WebNewDriver(
      pageDouble({
        testIds: {
          "page-detail-title": at({ getByRole: () => field, getAttribute: async () => "page-title-editor text-size-small-font" }),
        },
      }).page
    );

    await expect(driver.pagesPrefsTitleValue()).resolves.toBe("Page name");
    await driver.pagesPrefsTitleFill("New name");
    await driver.pagesPrefsTitleFocus();
    await driver.pagesPrefsTitlePressEnter();
    await expect(driver.pagesPrefsTitleClass()).resolves.toContain("small-font");

    expect(field.click).toHaveBeenCalledTimes(3);
    expect(stub.route).not.toHaveBeenCalled();
  });

  it("falls back to an empty title class when the attribute is absent", async () => {
    const title = at({ getAttribute: async () => null });
    const driver = new WebNewDriver(pageDouble({ testIds: { "page-detail-title": title } }).page);

    await expect(driver.pagesPrefsTitleClass()).resolves.toBe("");
  });

  it("reads a null title text as empty", async () => {
    const field = at({ innerText: async () => null as unknown as string });
    const title = at({ getByRole: () => field });
    const driver = new WebNewDriver(pageDouble({ testIds: { "page-detail-title": title } }).page);
    await expect(driver.pagesPrefsTitleValue()).resolves.toBe("");
  });

  it("classifies focus as title, body or elsewhere", async () => {
    const regions: Array<{ active: unknown; expected: string }> = [
      { active: null, expected: "other" },
      { active: { closest: () => null }, expected: "other" },
      { active: { closest: (selector: string) => (selector.includes("page-detail-title") ? {} : null) }, expected: "title" },
      { active: { closest: (selector: string) => (selector.includes("Page body") ? {} : null) }, expected: "body" },
      { active: { closest: (selector: string) => (selector.includes("editor-container") ? {} : null) }, expected: "body" },
    ];
    // A real body element is the explicit "other" case.
    vi.stubGlobal("document", { body: {} });
    for (const region of regions) {
      vi.stubGlobal("document", { activeElement: region.active, body: {} });
      const driver = new WebNewDriver(pageDouble().page);
      await expect(driver.pagesPrefsFocusedRegion()).resolves.toBe(region.expected);
    }
  });
});

describe("page display preferences", () => {
  it("opens the options menu, reads normalized labels and toggles an item", async () => {
    const item = at({ click: async () => undefined });
    const items = menuOf(["Full width", "  Sticky   toolbar  ", null]);
    (items as unknown as FakeLocator).filter = () => item;
    const optionButton = at();
    const stub = pageDouble({ roles: { button: optionButton, menuitem: items } });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesPrefsOptionsOpen();
    await expect(driver.pagesPrefsOptionsItems()).resolves.toEqual(["Full width", "Sticky toolbar"]);
    await driver.pagesPrefsOptionsToggle("Full width");

    expect(optionButton.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(items.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(items.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 30_000 });
    expect(item.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.route).not.toHaveBeenCalled();
  });

  it("returns a menu label when an item has no text", async () => {
    const items = menuOf(["Full width", null]);
    const driver = new WebNewDriver(pageDouble({ roles: { menuitem: items } }).page);
    await expect(driver.pagesPrefsOptionsItems()).resolves.toEqual(["Full width"]);
  });

  it("reads a null menu label as empty", async () => {
    const items = at({ nth: () => at({ innerText: async () => null as unknown as string }) });
    const driver = new WebNewDriver(pageDouble({ roles: { menuitem: items } }).page);
    await expect(driver.pagesPrefsOptionsItems()).resolves.toEqual([]);
  });

  it("reads null for absent or malformed local config and normalizes a stored one", async () => {
    const store = new Map<string, string>();
    const localStorage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    };
    vi.stubGlobal("window", { localStorage });
    const driver = new WebNewDriver(pageDouble().page);

    await expect(driver.pagesPrefsLocalConfig()).resolves.toBeNull();

    store.set("pages_config", "not json");
    await expect(driver.pagesPrefsLocalConfig()).resolves.toBeNull();

    store.set("pages_config", JSON.stringify({ full_width: true, font_size: "small-font", font_style: "serif", sticky_toolbar: true }));
    await expect(driver.pagesPrefsLocalConfig()).resolves.toEqual({
      fullWidth: true,
      fontSize: "small-font",
      fontStyle: "serif",
      stickyToolbar: true,
    });

    store.set("pages_config", JSON.stringify({ full_width: false, font_size: 4, font_style: null }));
    await expect(driver.pagesPrefsLocalConfig()).resolves.toEqual({
      fullWidth: false,
      fontSize: "",
      fontStyle: "",
      stickyToolbar: false,
    });
  });

  it("seeds the local config under the historical key", async () => {
    const stored: Array<[string, string]> = [];
    vi.stubGlobal("window", { localStorage: { getItem: async () => null, setItem: (key: string, value: string) => stored.push([key, value]) } });
    const driver = new WebNewDriver(pageDouble().page);

    await driver.pagesPrefsSeedLocalConfig({
      fullWidth: true,
      fontSize: "large-font",
      fontStyle: "sans-serif",
      stickyToolbar: false,
    });

    expect(stored).toEqual([
      [
        "pages_config",
        JSON.stringify({ full_width: true, font_size: "large-font", font_style: "sans-serif", sticky_toolbar: false }),
      ],
    ]);
  });

  it("reads toolbar visibility and both wide-layout branches", async () => {
    const visibleBar = at({ visible: true });
    const content = at({ getAttribute: async () => "page-toolbar-content flex wide-layout" });
    const driver = new WebNewDriver(
      pageDouble({ testIds: { "page-formatting-bar": at({ visible: true, locator: () => content }) } }).page
    );
    await expect(driver.pagesPrefsToolbarVisible()).resolves.toBe(true);
    await expect(driver.pagesPrefsToolbarWide()).resolves.toBe(true);

    const hidden = new WebNewDriver(
      pageDouble({ testIds: { "page-formatting-bar": at({ visible: false, locator: () => at({ count: 0 }) }) } }).page
    );
    await expect(hidden.pagesPrefsToolbarVisible()).resolves.toBe(false);
    await expect(hidden.pagesPrefsToolbarWide()).resolves.toBe(false);

    const plainContent = at({ getAttribute: async () => "page-toolbar-content flex" });
    const plainBar = at({ locator: () => plainContent });
    const noClassDriver = new WebNewDriver(
      pageDouble({ testIds: { "page-formatting-bar": plainBar } }).page
    );
    await expect(noClassDriver.pagesPrefsToolbarWide()).resolves.toBe(false);

    const nullContent = at({ getAttribute: async () => null });
    const nullDriver = new WebNewDriver(
      pageDouble({ testIds: { "page-formatting-bar": at({ locator: () => nullContent }) } }).page
    );
    await expect(nullDriver.pagesPrefsToolbarWide()).resolves.toBe(false);
    expect(visibleBar).toBeDefined();
  });

  it("reads the settled canvas width, including a transition that never settles", async () => {
    const widths = ["720px", "900px", "1152px", "1152px"];
    let last = "1152px";
    vi.stubGlobal("getComputedStyle", () => ({ maxWidth: widths.shift() ?? last }));
    const measure = at({
      evaluate: async (fn: (node: unknown) => unknown) => {
        const result = fn({});
        last = String(result);
        return result;
      },
    });
    const stub = pageDouble({
      selectors: { ".frame-renderer": at({ first: () => measure }) },
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.pagesPrefsCanvasMaxWidth()).resolves.toBe("1152px");
    expect(measure.waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });

    let widthRead = 0;
    vi.stubGlobal("getComputedStyle", () => ({ maxWidth: widthRead++ % 2 === 0 ? "720px" : "1152px" }));
    const never = at({ evaluate: async (fn: (node: unknown) => unknown) => fn({}) });
    const movingStub = pageDouble({ selectors: { ".frame-renderer": at({ first: () => never }) } });
    const movingDriver = new WebNewDriver(movingStub.page);
    await expect(movingDriver.pagesPrefsCanvasMaxWidth()).resolves.toBe("1152px");
    expect(movingStub.waitForTimeout).toHaveBeenCalledTimes(25);
  });

  it("reads the editor container class or an empty string when absent", async () => {
    const container = at({ getAttribute: async () => "editor-container text-size-large-font text-face-serif" });
    const present = new WebNewDriver(pageDouble({ selectors: { ".editor-container": container } }).page);
    await expect(present.pagesPrefsEditorClass()).resolves.toContain("serif");

    const absent = new WebNewDriver(pageDouble().page);
    await expect(absent.pagesPrefsEditorClass()).resolves.toBe("");

    const noClass = new WebNewDriver(
      pageDouble({ selectors: { ".editor-container": at({ getAttribute: async () => null }) } }).page
    );
    await expect(noClass.pagesPrefsEditorClass()).resolves.toBe("");
  });

  it("watches only page writes and returns a copy of the payloads", async () => {
    const handlers: Array<(route: Route) => Promise<void>> = [];
    const stub = pageDouble();
    stub.route.mockImplementation(async (_pattern: string, handler: (route: Route) => Promise<void>) => {
      handlers.push(handler);
    });
    const driver = new WebNewDriver(stub.page);
    await driver.pagesPrefsSpyPageWrites();

    const route = (method: string, url: string, body?: string) => {
      const request = { method: () => method, url: () => url, postData: () => body ?? null };
      const handler = { request: () => request, continue: vi.fn(async () => undefined) };
      return handler;
    };
    const created = route("POST", "https://api.test/api/workspaces/acme/pages/", "create");
    const updated = route("PATCH", "https://api.test/api/workspaces/acme/pages/p1/", "update");
    const read = route("GET", "https://api.test/api/workspaces/acme/pages/");
    const elsewhere = route("POST", "https://api.test/api/workspaces/acme/issues/", "issue");
    const noBody = route("DELETE", "https://api.test/api/workspaces/acme/pages/p1/");

    for (const handler of [created, updated, read, elsewhere, noBody]) {
      await handlers[0]?.(handler as unknown as Route);
      expect(handler.continue).toHaveBeenCalledTimes(1);
    }

    await expect(driver.pagesPrefsPageWriteCount()).resolves.toBe(3);
    const payloads = await driver.pagesPrefsPageWritePayloads();
    expect(payloads).toEqual(["create", "update", ""]);
    payloads.push("outside");
    await expect(driver.pagesPrefsPageWritePayloads()).resolves.toEqual(["create", "update", ""]);

    await driver.pagesPrefsSpyPageWrites();
    await expect(driver.pagesPrefsPageWriteCount()).resolves.toBe(0);
    await expect(driver.pagesPrefsPageWritePayloads()).resolves.toEqual([]);
  });
});

describe("page toolbar key map and list search", () => {
  it("reads each tooltip, dismissing the previous one and keeping unhinted controls bare", async () => {
    let tooltipText = "";
    let tooltipAvailable = true;
    const tooltip = at({
      innerText: async () => tooltipText,
      waitFor: async () => {
        if (!tooltipAvailable) throw new Error("no tooltip");
      },
    });
    const tips = at({
      waitFor: async () => undefined,
      first: () => tooltip,
    });
    const labels = ["Bold", "Italic", "Underline", "NoTip", "Empty"];
    const buttons = labels.map((label) =>
      at({
        getAttribute: async () => label,
        hover: async () => {
          tooltipAvailable = true;
          tooltipText =
            label === "Bold"
              ? "Bold\nCmd + B"
              : label === "Italic"
                ? "Italic\nCmd+Shift+S"
                : label === "Underline"
                  ? "Underline"
                  : label === "NoTip"
                    ? (tooltipAvailable = false, "")
                    : "";
        },
      })
    );
    const collection = at({ count: buttons.length, nth: (index) => buttons[index] ?? empty() });
    const bar = at({ locator: () => collection });
    const stub = pageDouble({
      testIds: { "page-formatting-bar": bar },
      roles: { tooltip: tips },
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesKeysToolbarTooltips()).resolves.toEqual([
      { label: "Bold", keys: "Cmd + B" },
      { label: "Italic", keys: "Cmd + Shift + S" },
      { label: "Underline", keys: "" },
      { label: "NoTip", keys: "" },
      { label: "Empty", keys: "" },
    ]);
    expect(stub.press).toHaveBeenCalledWith("Escape");
    expect(stub.waitForTimeout).toHaveBeenCalledWith(200);
  });

  it("continues when the stale tooltip never hides or its text read fails", async () => {
    let hiddenReads = 0;
    const tooltip = at({
      waitFor: async () => undefined,
      innerText: async () => {
        throw new Error("gone");
      },
    });
    const tips = at({
      waitFor: async (options?: { state?: string }) => {
        hiddenReads += 1;
        if (options?.state === "hidden") throw new Error("still open");
      },
      first: () => tooltip,
    });
    const button = at({ getAttribute: async () => "Bold" });
    const buttons = at({ count: 1, nth: () => button });
    const stub = pageDouble({
      testIds: { "page-formatting-bar": at({ locator: () => buttons }) },
      roles: { tooltip: tips },
    });

    await expect(new WebNewDriver(stub.page).pagesKeysToolbarTooltips()).resolves.toEqual([
      { label: "Bold", keys: "" },
    ]);
    expect(hiddenReads).toBe(1);
    expect(stub.waitForTimeout).toHaveBeenCalledTimes(10);
  });

  it("normalizes a missing button label and null tooltip text", async () => {
    const tooltip = at({ waitFor: async () => undefined, innerText: async () => null as unknown as string });
    const button = at({ getAttribute: async () => null });
    const buttons = at({ count: 1, nth: () => button });
    const stub = pageDouble({
      testIds: { "page-formatting-bar": at({ locator: () => buttons }) },
      roles: { tooltip: at({ waitFor: async () => undefined, first: () => tooltip }) },
    });

    await expect(new WebNewDriver(stub.page).pagesKeysToolbarTooltips()).resolves.toEqual([
      { label: "", keys: "" },
    ]);
  });

  it("presses keys and drives the list search control", async () => {
    let expanded = false;
    const box = at({
      count: 1,
      visible: false,
      press: async () => undefined,
      fill: async () => undefined,
      inputValue: async () => "alpha",
      isVisible: async () => expanded,
    });
    const open = at({
      click: async () => {
        expanded = true;
      },
    });
    const stub = pageDouble({
      roles: {
        textbox: box,
        button: open,
      },
    });
    const driver = new WebNewDriver(stub.page);

    await driver.pagesKeysPress("Control+b");
    await driver.pagesKeysSearchOpen();
    await driver.pagesKeysSearchFill("alpha");
    await expect(driver.pagesKeysSearchValue()).resolves.toBe("alpha");
    await driver.pagesKeysSearchPressEscape();
    await expect(driver.pagesKeysSearchExpanded()).resolves.toBe(true);

    expect(stub.press).toHaveBeenCalledWith("Control+b");
    expect(open.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(box.fill).toHaveBeenCalledWith("alpha", { timeout: 30_000 });
    expect(box.press).toHaveBeenCalledWith("Escape", { timeout: 30_000 });
  });

  it("leaves an already expanded search control alone", async () => {
    const box = at({ count: 1, visible: true });
    const open = at();
    const driver = new WebNewDriver(pageDouble({ roles: { textbox: box, button: open } }).page);
    await driver.pagesKeysSearchOpen();
    expect(open.click).not.toHaveBeenCalled();
  });

  it("lists row titles from labeled links and ignores unlabeled rows", async () => {
    const links = [
      { getAttribute: () => " Alpha " },
      { getAttribute: () => "" },
      { getAttribute: () => null },
      { getAttribute: () => "Beta" },
    ];
    vi.stubGlobal("document", { querySelectorAll: () => links });
    const driver = new WebNewDriver(pageDouble().page);
    await expect(driver.pagesKeysListRowTitles()).resolves.toEqual(["Alpha", "Beta"]);
  });

  it("detects a shortcut reference from text or a button, and otherwise reports none", async () => {
    const withText = new WebNewDriver(
      pageDouble({ texts: [{ pattern: /keyboard shortcuts?/i, value: at() }] }).page
    );
    await expect(withText.pagesKeysShortcutReferenceVisible()).resolves.toBe(true);

    const withButton = new WebNewDriver(
      pageDouble({ roles: { button: at({ count: 1 }) }, texts: [{ pattern: /keyboard shortcuts?/i, value: at({ count: 0 }) }] }).page
    );
    await expect(withButton.pagesKeysShortcutReferenceVisible()).resolves.toBe(true);

    const absent = new WebNewDriver(
      pageDouble({ roles: { button: at({ count: 0 }) }, texts: [{ pattern: /keyboard shortcuts?/i, value: at({ count: 0 }) }] }).page
    );
    await expect(absent.pagesKeysShortcutReferenceVisible()).resolves.toBe(false);
  });
});

describe("page assistant and cloud slots", () => {
  it("reports the AI handle and menu only when visible", async () => {
    const shown = new WebNewDriver(
      pageDouble({ testIds: { "page-ai-handle": at({ visible: true }), "page-assistant-menu": at({ visible: true }) } }).page
    );
    await expect(shown.pagesAssistAiHandleVisible()).resolves.toBe(true);
    await expect(shown.pagesAssistAiMenuVisible()).resolves.toBe(true);

    const hidden = new WebNewDriver(
      pageDouble({ testIds: { "page-ai-handle": at({ visible: false }), "page-assistant-menu": at({ visible: false }) } }).page
    );
    await expect(hidden.pagesAssistAiHandleVisible()).resolves.toBe(false);
    await expect(hidden.pagesAssistAiMenuVisible()).resolves.toBe(false);
  });

  it("reads header action names in order, skipping unnamed and pane-toggle controls", async () => {
    const names = ["", "Navigation pane", "Add to favorites", "Lock", "Copy link", "Page options", null];
    const buttons = at({ count: names.length, nth: (index) => at({ getAttribute: async () => names[index] ?? null }) });
    const header = at({ getByRole: () => buttons });
    const driver = new WebNewDriver(pageDouble({ testIds: { "page-detail-header": header } }).page);

    await expect(driver.pagesCloudHeaderActionNames()).resolves.toEqual([
      "Add to favorites",
      "Lock",
      "Copy link",
      "Page options",
    ]);
  });

  it("excludes the sibling switcher whose name carries the page title", async () => {
    const action = (aria: string) => at({ getAttribute: async () => aria });
    const switcher = at({
      getAttribute: async () => "Switch page, current NF320 share 123",
      locator: () => at({ count: 1 }),
    });
    const buttons = at({
      count: 3,
      nth: (index) => (index === 0 ? switcher : action(index === 1 ? "Copy link" : "Page options")),
    });
    const header = at({ getByRole: () => buttons });
    const driver = new WebNewDriver(pageDouble({ testIds: { "page-detail-header": header } }).page);

    await expect(driver.pagesCloudHeaderActionNames()).resolves.toEqual(["Copy link", "Page options"]);
  });

  it("names the pane toggle icon and handles missing or unclassed icons", async () => {
    const icon = (className: string | null) => at({ getAttribute: async () => className });
    const toggle = at({ locator: () => at({ first: () => icon("lucide lucide-panel-right") }) });
    const stub = pageDouble({ testIds: { "page-pane-toggle": toggle } });
    await expect(new WebNewDriver(stub.page).pagesCloudToolbarEndNames()).resolves.toEqual(["(icon:panel-right)"]);

    const unclassed = at({ locator: () => at({ first: () => icon("custom") }) });
    await expect(
      new WebNewDriver(pageDouble({ testIds: { "page-pane-toggle": unclassed } }).page).pagesCloudToolbarEndNames()
    ).resolves.toEqual(["(icon-only)"]);

    const failing = at({
      locator: () =>
        at({
          first: () =>
            at({
              getAttribute: async () => {
                throw new Error("gone");
              },
            }),
        }),
    });
    await expect(
      new WebNewDriver(pageDouble({ testIds: { "page-pane-toggle": failing } }).page).pagesCloudToolbarEndNames()
    ).resolves.toEqual(["(icon-only)"]);

    await expect(new WebNewDriver(pageDouble().page).pagesCloudToolbarEndNames()).resolves.toEqual([]);
  });

  it("opens the navigation pane only when it is closed and reads its tab names", async () => {
    const alreadyOpenPane = at({ getAttribute: async () => "true", getByRole: () => at() });
    const toggle = at();
    const alreadyOpen = new WebNewDriver(
      pageDouble({ testIds: { "page-navigation-pane": alreadyOpenPane, "page-pane-toggle": toggle } }).page
    );
    await alreadyOpen.pagesCloudPaneOpen();
    expect(toggle.click).not.toHaveBeenCalled();

    const closedPane = at({
      getAttribute: async () => "false",
      getByRole: () => at({ first: () => at() }),
    });
    const closedToggle = at();
    const stub = pageDouble({ testIds: { "page-navigation-pane": closedPane, "page-pane-toggle": closedToggle } });
    const closed = new WebNewDriver(stub.page);
    await closed.pagesCloudPaneOpen();
    expect(closedToggle.click).toHaveBeenCalledWith({ timeout: 30_000 });

    const openTabs = at({ getAttribute: async () => "true", getByRole: () => at({ allInnerTexts: async () => [" Outline ", "Info", ""] }) });
    await expect(
      new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": openTabs } }).page).pagesCloudPaneTabNames()
    ).resolves.toEqual(["Outline", "Info"]);

    const closedTabs = at({ getAttribute: async () => "false" });
    await expect(
      new WebNewDriver(pageDouble({ testIds: { "page-navigation-pane": closedTabs } }).page).pagesCloudPaneTabNames()
    ).resolves.toEqual([]);
    await expect(
      new WebNewDriver(pageDouble().page).pagesCloudPaneTabNames()
    ).resolves.toEqual([]);
  });

  it("clears the body, types a slash and reads palette titles before dismissing it", async () => {
    const editor = bodyEditor({ locator: () => at({ count: 0 }) });
    const palette = at({
      locator: () => at({ allInnerTexts: async () => [" Text ", "Heading 1", ""] }),
      waitFor: async () => undefined,
    });
    const stub = pageDouble({
      selectors: { '[aria-label="Page body"][contenteditable="true"]': editor },
      testIds: { "page-slash-palette": palette },
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesCloudSlashEntries()).resolves.toEqual(["Text", "Heading 1"]);
    expect(stub.type).toHaveBeenCalledWith("/");
    expect(stub.press).toHaveBeenCalledWith("Escape");
    expect(palette.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 30_000 });

    const stays = at({
      locator: () => at({ allInnerTexts: async () => ["Text"] }),
      waitFor: async (options?: { state?: string }) => {
        if (options?.state === "detached") throw new Error("still open");
      },
    });
    const stubborn = new WebNewDriver(
      pageDouble({
        selectors: { '[aria-label="Page body"][contenteditable="true"]': bodyEditor({ locator: () => at({ count: 0 }) }) },
        testIds: { "page-slash-palette": stays },
      }).page
    );
    await expect(stubborn.pagesCloudSlashEntries()).resolves.toEqual(["Text"]);
  });
});

describe("page desktop and negative probes", () => {
  it("reads mobile and platform from the user agent", async () => {
    const cases: Array<[string, boolean, string]> = [
      ["Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)", true, ""],
      ["Mozilla/5.0 (Windows NT 10.0; Win64; x64)", false, "Windows"],
      ["Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)", false, "MacOS"],
      ["Mozilla/5.0 (X11; Linux x86_64)", false, "Linux"],
      ["Mystery/1.0", false, "Unknown"],
    ];
    for (const [userAgent, mobile, platform] of cases) {
      vi.stubGlobal("window", { navigator: { userAgent } });
      const driver = new WebNewDriver(pageDouble().page);
      await expect(driver.pagesDeskIsMobile()).resolves.toBe(mobile);
      await expect(driver.pagesDeskPlatform()).resolves.toBe(platform);
    }
  });

  it("reads the list tabs and trims away empty labels", async () => {
    const links = at({ allInnerTexts: async () => [" Public ", "Private", "", "Archived "] });
    const tabs = at({ getByRole: () => links });
    const driver = new WebNewDriver(pageDouble({ testIds: { "pages-list-tabs": tabs } }).page);
    await expect(driver.pagesNegListTabNames()).resolves.toEqual(["Public", "Private", "Archived"]);
    expect(links.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("opens the row menu for the named title and dismisses it", async () => {
    const action = at();
    const row = at({ getByRole: () => action });
    const rowCollection = at({ filter: () => row, first: () => row });
    const menu = menuOf(["Open in new tab", "Copy link"]);
    const stub = pageDouble({ testIds: { "page-row": rowCollection }, roles: { menuitem: menu } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.pagesNegRowMenuItems("Alpha")).resolves.toEqual(["Open in new tab", "Copy link"]);

    expect(row.scrollIntoViewIfNeeded).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(action.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.press).toHaveBeenCalledWith("Escape");
  });

  it("finds a cover by copy or file input and reports none when both are absent", async () => {
    const copy = new WebNewDriver(
      pageDouble({ texts: [{ pattern: /cover image|add cover|page cover|change cover/i, value: at() }] }).page
    );
    await expect(copy.pagesNegCoverPresent()).resolves.toBe(true);

    const input = new WebNewDriver(
      pageDouble({
        texts: [{ pattern: /cover image|add cover|page cover|change cover/i, value: at({ count: 0 }) }],
        selectors: { 'input[type="file"][aria-label*="cover" i]': at() },
      }).page
    );
    await expect(input.pagesNegCoverPresent()).resolves.toBe(true);

    const absent = new WebNewDriver(
      pageDouble({
        texts: [{ pattern: /cover image|add cover|page cover|change cover/i, value: at({ count: 0 }) }],
      }).page
    );
    await expect(absent.pagesNegCoverPresent()).resolves.toBe(false);
  });

  it("reports a conflict only from dialog copy and tolerates a detached dialog", async () => {
    const conflict = new WebNewDriver(
      pageDouble({
        roles: {
          dialog: at({ nth: () => at({ innerText: async () => "Remote changes conflict with yours" }) }),
          alertdialog: at({ count: 0 }),
        },
      }).page
    );
    await expect(conflict.pagesNegConflictDialogVisible()).resolves.toBe(true);

    const detachedThenConflict = new WebNewDriver(
      pageDouble({
        roles: {
          dialog: at({
            nth: () =>
              at({
                innerText: async () => {
                  throw new Error("closed");
                },
              }),
          }),
          alertdialog: at({ nth: () => at({ innerText: async () => "MERGE CONFLICT" }) }),
        },
      }).page
    );
    await expect(detachedThenConflict.pagesNegConflictDialogVisible()).resolves.toBe(true);

    const nullCopy = new WebNewDriver(
      pageDouble({
        roles: {
          dialog: at({ nth: () => at({ innerText: async () => null as unknown as string }) }),
          alertdialog: at({ count: 0 }),
        },
      }).page
    );
    await expect(nullCopy.pagesNegConflictDialogVisible()).resolves.toBe(false);

    const none = new WebNewDriver(
      pageDouble({
        roles: {
          dialog: at({ nth: () => at({ innerText: async () => "Saved" }) }),
          alertdialog: at({ count: 0 }),
        },
      }).page
    );
    await expect(none.pagesNegConflictDialogVisible()).resolves.toBe(false);
  });

  it("reports the page mark by presence", async () => {
    await expect(
      new WebNewDriver(pageDouble({ testIds: { "page-mark": at() } }).page).pagesNegMarkPresent()
    ).resolves.toBe(true);
    await expect(new WebNewDriver(pageDouble().page).pagesNegMarkPresent()).resolves.toBe(false);
  });
});

