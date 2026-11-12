// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver page-body probes (NEWFRONT-316,
// PAGE-037-044): the block editor, its slash palette, drag handle, mentions,
// emoji, links, the pane's assets/info tabs and the header's options. A Page
// double answers the same addresses the driver uses — the body shell, its
// editor, and the data-testid hooks beside it — so every method runs without
// a browser or the parity stack.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface LocatorOptions {
  count?: number | (() => number);
  visible?: boolean;
  innerText?: () => Promise<string>;
  innerHTML?: () => Promise<string>;
  textContent?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  waitFor?: (options?: { state?: string; timeout?: number }) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  inputValue?: () => Promise<string>;
  isVisible?: () => Promise<boolean>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  evaluate?: (fn: (node: unknown, arg?: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  dispatchEvent?: (type: string, init?: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: () => Promise<void>;
  locator?: (selector: string, options?: unknown) => Locator;
  getByRole?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
  last?: () => Locator;
}

const CANVAS_SELECTOR = "#page-content-container";
const EDITOR_SELECTOR = `${CANVAS_SELECTOR} .editor-container [contenteditable="true"]`;
const PANE_SELECTOR = '[data-testid="page-navigation-pane"][data-open="true"]';
const BUBBLE_SELECTOR = '[data-testid="page-selection-bubble"]';
const SLASH_SELECTOR = '[data-testid="page-slash-palette"]';
const HANDLE_SELECTOR = '[data-testid="page-block-handle"]';
const EMOJI_SELECTOR = '[data-testid="page-emoji-picker"] [data-testid="page-emoji-option"]';
const TOAST_SELECTOR = '[data-testid="toast-notification"]';
const MENTION_ITEM_PREFIX = '[id^="mention-item-"]';

class FakeLocator {
  readonly waitFor: ReturnType<typeof vi.fn>;
  readonly click: ReturnType<typeof vi.fn>;
  readonly hover: ReturnType<typeof vi.fn>;
  readonly fill: ReturnType<typeof vi.fn>;
  readonly scrollIntoViewIfNeeded: ReturnType<typeof vi.fn>;
  readonly evaluate: ReturnType<typeof vi.fn>;
  readonly dispatchEvent: ReturnType<typeof vi.fn>;
  readonly isVisible: ReturnType<typeof vi.fn>;

  constructor(private readonly options: LocatorOptions = {}) {
    this.waitFor = vi.fn(this.options.waitFor ?? (async () => undefined));
    this.click = vi.fn(this.options.click ?? (async () => undefined));
    this.hover = vi.fn(this.options.hover ?? (async () => undefined));
    this.fill = vi.fn(this.options.fill ?? (async () => undefined));
    this.scrollIntoViewIfNeeded = vi.fn(this.options.scrollIntoViewIfNeeded ?? (async () => undefined));
    // The default answers null instead of running the callback: the editor's
    // evaluators touch DOM globals (MutationObserver, Selection) that do not
    // exist here, and every test that depends on the answer supplies it.
    this.evaluate = vi.fn(this.options.evaluate ?? (async () => null));
    this.dispatchEvent = vi.fn(this.options.dispatchEvent ?? (async () => undefined));
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

  innerHTML(): Promise<string> {
    return (this.options.innerHTML ?? (async () => ""))();
  }

  textContent(): Promise<string> {
    return (this.options.textContent ?? this.options.innerText ?? (async () => ""))();
  }

  allTextContents(): Promise<string[]> {
    return (this.options.allTextContents ?? (async () => []))();
  }

  getAttribute(name: string): Promise<string | null> {
    return (this.options.getAttribute ?? (async () => null))(name);
  }

  inputValue(): Promise<string> {
    return (this.options.inputValue ?? (async () => ""))();
  }

  boundingBox(): Promise<{ x: number; y: number; width: number; height: number } | null> {
    return (this.options.boundingBox ?? (async () => null))();
  }

  locator(selector: string, options?: unknown): Locator {
    return this.options.locator?.(selector, options) ?? empty();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): Locator {
    return this.options.getByRole?.(role, options) ?? empty();
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
  /** Every page-level address: selectors plus the accessible-name reads. */
  selectors?: Record<string, Locator>;
  roles?: Record<string, Locator>;
  testIds?: Record<string, Locator>;
  evaluate?: (fn: (arg?: unknown) => unknown, arg?: unknown) => Promise<unknown>;
  fileChooser?: { setFiles: (files: unknown) => Promise<void> };
  clipboard?: string;
}

interface PageDouble {
  page: Page;
  goto: ReturnType<typeof vi.fn>;
  reload: ReturnType<typeof vi.fn>;
  type: ReturnType<typeof vi.fn>;
  press: ReturnType<typeof vi.fn>;
  evaluate: ReturnType<typeof vi.fn>;
  grantPermissions: ReturnType<typeof vi.fn>;
  waitForTimeout: ReturnType<typeof vi.fn>;
  waitForEvent: ReturnType<typeof vi.fn>;
  writeText: ReturnType<typeof vi.fn>;
  readText: ReturnType<typeof vi.fn>;
  mouseDown: ReturnType<typeof vi.fn>;
  mouseUp: ReturnType<typeof vi.fn>;
}

/**
 * A Page double. `evaluate` answers null by default: the driver's page-level
 * probes (focus, clipboard, paste watch) are answered per test, and running
 * them here would touch DOM globals that do not exist in this environment.
 */
function pageDouble(options: PageOptions = {}): PageDouble {
  let stored = options.clipboard ?? "";
  const goto = vi.fn(async () => undefined);
  const reload = vi.fn(async () => undefined);
  const type = vi.fn(async () => undefined);
  const press = vi.fn(async () => undefined);
  const evaluate = vi.fn(options.evaluate ?? (async () => null));
  const grantPermissions = vi.fn(async () => undefined);
  const waitForTimeout = vi.fn(async () => undefined);
  const waitForEvent = vi.fn(async () => options.fileChooser ?? { setFiles: vi.fn(async () => undefined) });
  const writeText = vi.fn(async (value: string) => {
    stored = value;
  });
  const readText = vi.fn(async () => stored);
  const mouseDown = vi.fn(async () => undefined);
  const mouseUp = vi.fn(async () => undefined);
  const page = {
    goto,
    reload,
    evaluate,
    mouse: { move: vi.fn(async () => undefined), down: mouseDown, up: mouseUp },
    url: () => options.url ?? "https://parity.test/acme/projects/p1/pages/pg1",
    keyboard: { type, press },
    context: () => ({ grantPermissions }),
    clipboard: { writeText, readText },
    waitForTimeout,
    waitForEvent,
    getByTestId: vi.fn((id: string) => options.testIds?.[id] ?? empty()),
    getByRole: vi.fn((role: string, roleOptions?: { name?: string }) => {
      const key = roleOptions?.name === undefined ? role : `${role}:${roleOptions.name}`;
      return options.roles?.[key] ?? options.roles?.[role] ?? empty();
    }),
    locator: vi.fn((selector: string) => options.selectors?.[selector] ?? empty()),
  } as unknown as Page;
  return {
    page,
    goto,
    reload,
    type,
    press,
    evaluate,
    grantPermissions,
    waitForTimeout,
    waitForEvent,
    writeText,
    readText,
    mouseDown,
    mouseUp,
  };
}

const CANVAS = at();

interface StageOptions {
  /** The live editor, wired to its own children by selector. */
  editor?: Locator;
  /** Page-level addresses beside the body, by selector or role key. */
  selectors?: Record<string, Locator>;
  roles?: Record<string, Locator>;
  testIds?: Record<string, Locator>;
  page?: Omit<PageOptions, "selectors" | "roles" | "testIds">;
}

interface Stage {
  driver: WebNewDriver;
  stub: PageDouble;
  editor: Locator;
  page: Page;
}

/** A detail whose canvas and editor are mounted, as a settled body would be. */
function stage(options: StageOptions = {}): Stage {
  const editor = options.editor ?? at();
  const stub = pageDouble({
    ...options.page,
    selectors: {
      [CANVAS_SELECTOR]: CANVAS,
      [EDITOR_SELECTOR]: editor,
      ...(options.selectors ?? {}),
    },
    roles: options.roles,
    testIds: options.testIds,
  });
  return { driver: new WebNewDriver(stub.page), stub, editor, page: stub.page };
}

/** A live editor whose children answer by selector. */
function editorOf(children: Record<string, Locator> = {}, options: LocatorOptions = {}): Locator {
  return at({
    ...options,
    locator: (selector: string) => children[selector] ?? empty(),
  });
}

/** A pane that is open, with its own children resolved by selector. */
function paneOf(children: Record<string, Locator> = {}, options: LocatorOptions = {}): Locator {
  return at({ ...options, locator: (selector: string) => children[selector] ?? empty() });
}

describe("detail navigation", () => {
  it("opens a page detail and waits for the editable body", async () => {
    const editor = fake();
    const { driver, stub } = stage({ editor: editor.asLocator() });

    await driver.pagesBlocksOpenDetail("acme", "project-1", "page-1");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1", { waitUntil: "domcontentloaded" });
    expect(editor.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("carries a query string on the detail address", async () => {
    const { driver, stub } = stage();

    await driver.pagesBlocksOpenDetailQuery("acme", "project-1", "page-1", "paneTab=info");

    expect(stub.goto).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1?paneTab=info", {
      waitUntil: "domcontentloaded",
    });
  });

  it("reloads and re-settles the editor", async () => {
    const editor = fake();
    const { driver, stub } = stage({ editor: editor.asLocator() });

    await driver.pagesBlocksReload();

    expect(stub.reload).toHaveBeenCalledWith({ waitUntil: "domcontentloaded" });
    expect(editor.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("reports the refusal panel instead of waiting out the editor", async () => {
    // The body never paints while the panel is up, so the editor's own wait
    // stays pending and the refusal is what settles first.
    const editor = fake({ waitFor: () => new Promise<void>(() => undefined) });
    const { driver } = stage({
      editor: editor.asLocator(),
      selectors: { '[data-testid="page-detail-panel"]': fake().asLocator() },
    });

    await expect(driver.pagesBlocksOpenDetail("acme", "p", "g")).rejects.toThrow("refusal panel");
  });

  it("reports a body that never mounted, naming the live loader when it is up", async () => {
    const editor = fake({
      waitFor: async () => {
        throw new Error("no editor");
      },
    });
    const loading = stage({
      editor: editor.asLocator(),
      selectors: { '[data-testid="page-live-loader"]': fake().asLocator() },
    });
    await expect(loading.driver.pagesBlocksOpenDetail("acme", "p", "g")).rejects.toThrow("still on the live loader");

    const bare = stage({ editor: editor.asLocator() });
    await expect(bare.driver.pagesBlocksOpenDetail("acme", "p", "g")).rejects.toThrow(/never mounted\.$/);
  });
});

describe("editor reads", () => {
  it("reads trimmed body text and markup", async () => {
    const editor = at({ innerText: async () => "  Body  ", innerHTML: async () => "  <p>Body</p>  " });
    const { driver } = stage({ editor });

    await expect(driver.pagesBlocksEditorText()).resolves.toBe("Body");
    await expect(driver.pagesBlocksEditorHtml()).resolves.toBe("<p>Body</p>");
  });

  it("reads an empty string when the editor is absent, detached, or null", async () => {
    const absent = at({ locator: () => empty() });
    const missing = stage({ editor: absent });
    (missing.page as unknown as { locator: () => Locator }).locator = (selector: string) =>
      selector === CANVAS_SELECTOR ? CANVAS : empty();
    await expect(missing.driver.pagesBlocksEditorText()).resolves.toBe("");
    await expect(missing.driver.pagesBlocksEditorHtml()).resolves.toBe("");

    const detached = at({
      innerText: async () => {
        throw new Error("detached");
      },
      innerHTML: async () => {
        throw new Error("detached");
      },
    });
    const broken = stage({ editor: detached });
    await expect(broken.driver.pagesBlocksEditorText()).resolves.toBe("");
    await expect(broken.driver.pagesBlocksEditorHtml()).resolves.toBe("");

    const nullish = at({
      innerText: async () => null as unknown as string,
      innerHTML: async () => null as unknown as string,
    });
    const nulled = stage({ editor: nullish });
    await expect(nulled.driver.pagesBlocksEditorText()).resolves.toBe("");
    await expect(nulled.driver.pagesBlocksEditorHtml()).resolves.toBe("");
  });
});

describe("caret placement", () => {
  it("clicks the last painted block past its text, presses End, and stops at the end", async () => {
    const paragraph = fake({ boundingBox: async () => ({ x: 10, y: 40, width: 200, height: 20 }) });
    const paragraphs = at({ count: 2, nth: () => paragraph.asLocator() });
    const editor = editorOf({ p: paragraphs }, { evaluate: async () => true });
    const { driver, stub } = stage({ editor });

    await driver.pagesBlocksEditorClickEnd();

    expect(paragraph.click).toHaveBeenCalledWith({ position: { x: 192, y: 10 }, timeout: 30_000 });
    expect(stub.press.mock.calls).toEqual([["End"]]);
  });

  it("skips measured-empty blocks and falls back to a body click", async () => {
    const short = fake({ boundingBox: async () => ({ x: 0, y: 0, width: 100, height: 2 }) });
    const unmeasured = fake({ boundingBox: async () => null });
    const paragraphs = at({
      count: 2,
      nth: (index) => (index === 1 ? short.asLocator() : unmeasured.asLocator()),
    });
    const editor = fake({ evaluate: async () => true, locator: () => paragraphs.asLocator() });
    const { driver } = stage({ editor: editor.asLocator() });

    await driver.pagesBlocksEditorClickEnd();

    expect(short.click).not.toHaveBeenCalled();
    expect(editor.click).toHaveBeenCalledWith({ position: { x: 24, y: 16 }, timeout: 30_000 });
  });

  it("retries the placement while the caret refuses to land at the end", async () => {
    const paragraph = fake({ boundingBox: async () => ({ x: 0, y: 0, width: 100, height: 20 }) });
    const paragraphs = at({ count: 1, nth: () => paragraph.asLocator() });
    const editor = editorOf({ p: paragraphs }, { evaluate: async () => false });
    const { driver, stub } = stage({ editor });

    await driver.pagesBlocksEditorClickEnd();

    expect(paragraph.click).toHaveBeenCalledTimes(4);
    expect(stub.press).toHaveBeenCalledTimes(4);
  });

  it("answers caret-at-end from the editor's selection probe, and false when it throws", async () => {
    const { driver } = stage({ editor: at({ evaluate: async () => true }) });
    const failing = stage({
      editor: at({
        evaluate: async () => {
          throw new Error("gone");
        },
      }),
    });
    const caretAtEnd = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksCaretAtEnd(): Promise<boolean> }).pagesBlocksCaretAtEnd();

    await expect(caretAtEnd(driver)).resolves.toBe(true);
    await expect(caretAtEnd(failing.driver)).resolves.toBe(false);
  });
});

describe("keyboard and clipboard writes", () => {
  it("types after focusing the editor, without re-placing the caret", async () => {
    const { driver, stub } = stage({ page: { evaluate: async () => true } });

    await driver.pagesBlocksEditorType("hello");

    expect(stub.type).toHaveBeenCalledWith("hello");
    expect(stub.press).not.toHaveBeenCalled();
  });

  it("places the caret at the end first when the editor refuses focus", async () => {
    const editor = editorOf({ p: at({ count: 0 }) }, { evaluate: async () => true });
    const { driver, stub } = stage({ editor, page: { evaluate: async () => false } });

    await driver.pagesBlocksEditorType("hello");

    expect(stub.type).toHaveBeenCalledWith("hello");
    expect(stub.press).toHaveBeenCalledWith("End");
  });

  it("forwards keys, Enter and Backspace", async () => {
    const { driver, stub } = stage();

    await driver.pagesBlocksEditorPressKey("ControlOrMeta+z");
    await driver.pagesBlocksEditorPressEnter();
    await driver.pagesBlocksEditorPressBackspace();

    expect(stub.press.mock.calls).toEqual([["ControlOrMeta+z"], ["Enter"], ["Backspace"]]);
  });

  it("pastes markup through a synthetic clipboard event at the end of the body", async () => {
    const seen: unknown[] = [];
    const editor = editorOf(
      { p: at({ count: 0 }) },
      {
        evaluate: async (_fn: unknown, arg?: unknown) => {
          seen.push(arg);
          return arg === undefined ? true : null;
        },
      }
    );
    const { driver } = stage({ editor });

    await driver.pagesBlocksEditorPasteHtml("<h2>Heading</h2>");

    expect(seen).toContain("<h2>Heading</h2>");
  });

  it("writes the clipboard, waits for its read-back, and lets the paste land", async () => {
    let stored = "";
    const { driver, stub } = stage({
      page: {
        evaluate: async (fn: (arg?: unknown) => unknown, arg?: unknown) => {
          const source = String(fn);
          if (source.includes("__pagesBlocksPasteState")) return true;
          if (source.includes("writeText")) {
            stored = String(arg);
            return undefined;
          }
          if (source.includes("readText")) return stored;
          return null;
        },
      },
    });

    await driver.pagesBlocksEditorPasteText("pasted words");

    expect(stub.grantPermissions).toHaveBeenCalledWith(["clipboard-read", "clipboard-write"]);
    expect(stored).toBe("pasted words");
    expect(stub.press).toHaveBeenCalledWith("ControlOrMeta+v");
  });

  it("drives select-all, copy and paste through the keyboard", async () => {
    const { driver, stub } = stage({
      page: {
        evaluate: async (fn: (arg?: unknown) => unknown) =>
          String(fn).includes("__pagesBlocksPasteState") ? true : null,
      },
    });

    await driver.pagesBlocksEditorSelectAll();
    await driver.pagesBlocksEditorCopy();
    await driver.pagesBlocksEditorPaste();

    expect(stub.press.mock.calls).toEqual([["ControlOrMeta+a"], ["ControlOrMeta+c"], ["ControlOrMeta+v"]]);
  });

  it("selects a paragraph by its text with a triple click", async () => {
    const paragraph = fake();
    const seen: unknown[] = [];
    const editor = at({
      locator: (selector: string, options?: unknown) => {
        if (selector !== "p") return empty();
        seen.push(options);
        return paragraph.asLocator();
      },
    });
    const { driver } = stage({ editor });

    await driver.pagesBlocksParagraphSelectText("Intro");

    expect(seen).toEqual([{ hasText: "Intro" }]);
    expect(paragraph.click).toHaveBeenCalledWith({ clickCount: 3, timeout: 30_000 });
  });
});

describe("slash palette", () => {
  it("reports the palette open only while its root exists", async () => {
    const open = stage({ selectors: { [SLASH_SELECTOR]: at() } });
    const shut = stage();

    await expect(open.driver.pagesBlocksSlashOpen()).resolves.toBe(true);
    await expect(shut.driver.pagesBlocksSlashOpen()).resolves.toBe(false);
  });

  it("reads the entry labels then dismisses the palette", async () => {
    let dismissed = false;
    const root = fake({
      count: () => (dismissed ? 0 : 1),
      locator: (selector: string) =>
        selector === "button p" ? at({ allTextContents: async () => [" Text ", "", "Table "] }) : empty(),
    });
    const { driver, stub } = stage({ selectors: { [SLASH_SELECTOR]: root.asLocator() } });
    stub.press.mockImplementation(async (key: string) => {
      if (key === "Escape") dismissed = true;
    });

    await expect(driver.pagesBlocksSlashItems()).resolves.toEqual(["Text", "Table"]);
    expect(dismissed).toBe(true);
  });

  it("chooses an exact entry, waits for the palette to close, and restores focus", async () => {
    let open = true;
    let focused = false;
    const entry = fake({
      click: async () => {
        open = false;
      },
    });
    const root = fake({
      count: () => (open ? 1 : 0),
      getByRole: () => at({ nth: () => entry.asLocator() }),
    });
    const { driver } = stage({
      selectors: { [SLASH_SELECTOR]: root.asLocator() },
      page: {
        evaluate: async (fn: (arg?: unknown) => unknown) => {
          if (!String(fn).includes("document.activeElement")) return null;
          focused = true;
          return true;
        },
      },
    });

    await driver.pagesBlocksSlashChoose("Heading 2", 1);

    expect(entry.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(open).toBe(false);
    expect(focused).toBe(true);
  });

  it("keeps polling the editor focus until it lands", async () => {
    let open = true;
    let reads = 0;
    const entry = fake({ click: async () => (open = false) });
    const root = fake({ count: () => (open ? 1 : 0), getByRole: () => at({ nth: () => entry.asLocator() }) });
    const { driver } = stage({
      selectors: { [SLASH_SELECTOR]: root.asLocator() },
      page: {
        evaluate: async (fn: (arg?: unknown) => unknown) => {
          if (!String(fn).includes("document.activeElement")) return null;
          reads += 1;
          return reads > 1;
        },
      },
    });

    await driver.pagesBlocksSlashChoose("Table");

    expect(reads).toBeGreaterThan(1);
  });

  it("dismisses an open palette with Escape", async () => {
    let open = true;
    const root = fake({ count: () => (open ? 1 : 0) });
    const { driver, stub } = stage({ selectors: { [SLASH_SELECTOR]: root.asLocator() } });
    stub.press.mockImplementation(async (key: string) => {
      if (key === "Escape") open = false;
    });

    await driver.pagesBlocksSlashDismiss();

    expect(open).toBe(false);
    expect(stub.press).toHaveBeenCalledTimes(1);
  });

  it("leaves a closed palette alone", async () => {
    const { driver, stub } = stage();

    await driver.pagesBlocksSlashDismiss();

    expect(stub.press).not.toHaveBeenCalled();
  });

  it("opens the emoji picker from the palette entry", async () => {
    const emojiEntry = fake();
    let pickerOpen = false;
    const root = fake({ getByRole: () => emojiEntry.asLocator() });
    const { driver } = stage({
      selectors: {
        [SLASH_SELECTOR]: root.asLocator(),
        [EMOJI_SELECTOR]: at({ visible: false, count: () => (pickerOpen ? 1 : 0) }),
      },
    });
    emojiEntry.click.mockImplementation(async () => {
      pickerOpen = true;
    });

    await driver.pagesBlocksEmojiOpen();

    expect(emojiEntry.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(pickerOpen).toBe(true);
  });

  it("reads the picker's glyphs, dropping empty rows", async () => {
    const { driver } = stage({
      selectors: { [EMOJI_SELECTOR]: at({ allTextContents: async () => [" 😀 ", "", "🎉"] }) },
    });

    await expect(driver.pagesBlocksEmojiItems()).resolves.toEqual(["😀", "🎉"]);
  });

  it("picks an emoji and waits for the body to change", async () => {
    let text = "before";
    const option = fake({
      click: async () => {
        text = "before 😀";
      },
    });
    const { driver } = stage({
      editor: editorOf({}, { innerText: async () => text }),
      selectors: { [EMOJI_SELECTOR]: at({ nth: () => option.asLocator() }) },
    });

    await driver.pagesBlocksEmojiChoose(0);

    expect(option.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(text).toContain("😀");
  });
});

describe("drag handle and block menu", () => {
  it("hovers the last block that carries text, skipping blank paragraphs", async () => {
    const blank = fake({ evaluate: async () => "p", innerText: async () => "   " });
    const filled = fake({ evaluate: async () => "h2", innerText: async () => "Title" });
    const blocks = at({ count: 2, nth: (index) => (index === 0 ? filled.asLocator() : blank.asLocator()) });
    const { driver } = stage({ editor: editorOf({ "xpath=./*": blocks }), selectors: { [HANDLE_SELECTOR]: at() } });

    await driver.pagesBlocksDragHandleVisible();

    expect(filled.evaluate).toHaveBeenCalled();
    expect(filled.hover).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(blank.hover).not.toHaveBeenCalled();
  });

  it("falls back to the last block when every block is blank", async () => {
    const blank = fake({ evaluate: async () => "p", innerText: async () => "" });
    const blocks = at({ count: 1, nth: () => blank.asLocator(), last: () => blank.asLocator() });
    const { driver } = stage({ editor: editorOf({ "xpath=./*": blocks }), selectors: { [HANDLE_SELECTOR]: at() } });

    await driver.pagesBlocksDragHandleVisible();

    expect(blank.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("hovers the body itself when it has no blocks", async () => {
    const editor = fake({ locator: () => at({ count: 0 }) });
    const { driver } = stage({ editor: editor.asLocator(), selectors: { [HANDLE_SELECTOR]: at() } });

    await driver.pagesBlocksDragHandleVisible();

    expect(editor.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reports the handle hidden when it is missing or painted away", async () => {
    const missing = stage();
    (missing.page as unknown as { locator: () => Locator }).locator = () => empty();
    await expect(missing.driver.pagesBlocksDragHandleVisible()).resolves.toBe(false);

    const hidden = stage({
      editor: editorOf({ "xpath=./*": at({ count: 0 }) }),
      selectors: { [HANDLE_SELECTOR]: at({ isVisible: async () => false }) },
    });
    await expect(hidden.driver.pagesBlocksDragHandleVisible()).resolves.toBe(false);
  });

  /**
   * A hovered handle whose parent menu reports `itemCount` open entries.
   * The driver resolves the menu as handle → `xpath=..` → `[role="menu"]`.
   */
  function dragStage(
    box: { x: number; y: number; width: number; height: number } | null,
    itemCount: number
  ): { driver: WebNewDriver; handle: FakeLocator } {
    const menu = at({
      locator: (selector: string) => (selector === '[role="menuitem"]' ? at({ count: itemCount }) : empty()),
    });
    const handle = fake({
      boundingBox: async () => box,
      locator: (selector: string) =>
        selector === "xpath=.."
          ? at({ locator: (child: string) => (child === '[role="menu"]' ? menu : empty()) })
          : empty(),
    });
    const built = stage({
      editor: editorOf({ "xpath=./*": at({ count: 0 }) }),
      selectors: { [HANDLE_SELECTOR]: handle.asLocator() },
    });
    return { driver: built.driver, handle };
  }

  it("clicks the handle at its own coordinates so the gutter stays clickable", async () => {
    const { driver, handle } = dragStage({ x: 40, y: 100, width: 24, height: 24 }, 4);

    await driver.pagesBlocksDragOpenMenu();

    expect(handle.dispatchEvent).toHaveBeenCalledWith("click", { clientX: 52, clientY: 112 });
    expect(handle.click).not.toHaveBeenCalled();
  });

  it("falls back to a plain click when the handle reports no box", async () => {
    const { driver, handle } = dragStage(null, 1);

    await driver.pagesBlocksDragOpenMenu();

    expect(handle.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(handle.dispatchEvent).not.toHaveBeenCalled();
  });

  it("reads the menu labels trimmed and chooses one exactly", async () => {
    let menuOpen = true;
    const item = fake({
      click: async () => {
        menuOpen = false;
      },
    });
    const menu = at({
      count: () => (menuOpen ? 1 : 0),
      locator: (selector: string) =>
        selector === '[role="menuitem"]' ? at({ allTextContents: async () => [" Duplicate ", ""] }) : empty(),
      getByRole: () => at({ first: () => item.asLocator() }),
    });
    const handle = at({
      locator: (selector: string) => (selector === "xpath=.." ? at({ locator: () => menu }) : empty()),
    });
    const { driver } = stage({ selectors: { [HANDLE_SELECTOR]: handle } });

    await expect(driver.pagesBlocksDragMenuItems()).resolves.toEqual(["Duplicate"]);
    await driver.pagesBlocksDragMenuChoose("Duplicate");

    expect(item.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(menuOpen).toBe(false);
  });
});

describe("images", () => {
  it("opens the chooser from the palette and waits for the inserted image", async () => {
    const setFiles = vi.fn(async () => undefined);
    let open = true;
    const imageEntry = fake({
      click: async () => {
        open = false;
      },
    });
    const root = fake({ count: () => (open ? 1 : 0), getByRole: () => imageEntry.asLocator() });
    const image = fake();
    const { driver, stub } = stage({
      editor: editorOf({ img: at({ first: () => image.asLocator() }) }),
      selectors: { [SLASH_SELECTOR]: root.asLocator() },
      page: { fileChooser: { setFiles } },
    });

    await driver.pagesBlocksUploadImage({ name: "catalog.png", mime: "image/png", bytes: Buffer.from("png") });

    expect(stub.waitForEvent).toHaveBeenCalledWith("filechooser", { timeout: 30_000 });
    expect(setFiles).toHaveBeenCalledWith([{ name: "catalog.png", mimeType: "image/png", buffer: Buffer.from("png") }]);
    expect(image.waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
  });

  it("reads each inline image's source and alt text", async () => {
    const first = fake({ getAttribute: async (name: string) => (name === "src" ? "/a.png" : "A") });
    const second = fake({ getAttribute: async (name: string) => (name === "src" ? "/b.png" : null) });
    const images = at({ count: 2, nth: (index) => (index === 0 ? first.asLocator() : second.asLocator()) });
    const { driver } = stage({ editor: editorOf({ img: images }) });

    await expect(driver.pagesBlocksInlineImages()).resolves.toEqual([
      { src: "/a.png", alt: "A" },
      { src: "/b.png", alt: null },
    ]);
  });
});

describe("pane tabs", () => {
  function paneStage(pane: FakeLocator, url = "https://parity.test/acme/projects/p1/pages/pg1"): Stage {
    return stage({ selectors: { [PANE_SELECTOR]: pane.asLocator() }, page: { url } });
  }

  it("opens the pane on a tab by its address key", async () => {
    const pane = fake();
    const { driver, stub } = paneStage(pane, "https://parity.test/acme/projects/p1/pages/pg1?x=1");

    await driver.pagesBlocksAssetsTabOpen();

    expect(stub.goto).toHaveBeenCalledWith("https://parity.test/acme/projects/p1/pages/pg1?x=1&paneTab=assets", {
      waitUntil: "domcontentloaded",
    });
    expect(pane.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("reads asset rows by name and skips nameless or detached ones", async () => {
    const named = fake({ locator: () => at({ innerText: async () => " catalog.png " }) });
    const nameless = fake({ locator: () => at({ innerText: async () => "  " }) });
    const detached = fake({ locator: () => at({ innerText: async () => Promise.reject(new Error("gone")) }) });
    const rows = at({
      count: 3,
      nth: (index) => [named, nameless, detached][index]?.asLocator() ?? empty(),
    });
    const { driver } = paneStage(fake({ locator: () => rows.asLocator() }));

    await expect(driver.pagesBlocksAssetsTabRows()).resolves.toEqual([{ name: "catalog.png", kind: null }]);
  });

  it("reports the empty state only when it is in the pane and visible", async () => {
    const absent = paneStage(fake());
    await expect(absent.driver.pagesBlocksAssetsTabEmptyVisible()).resolves.toBe(false);

    const hidden = paneStage(fake({ locator: () => at({ isVisible: async () => false }) }));
    await expect(hidden.driver.pagesBlocksAssetsTabEmptyVisible()).resolves.toBe(false);

    const shown = paneStage(fake({ locator: () => at({ isVisible: async () => true }) }));
    await expect(shown.driver.pagesBlocksAssetsTabEmptyVisible()).resolves.toBe(true);
  });

  it("reports the info tab's word and character counts", async () => {
    const cards: Record<string, Locator> = {
      '[data-stat="words"] h6': at({ innerText: async () => "1,204 words" }),
      '[data-stat="characters"] h6': at({ innerText: async () => "6,002 characters" }),
    };
    const { driver } = paneStage(paneOf(cards));

    await expect(driver.pagesBlocksInfoStats()).resolves.toEqual({ words: 1204, characters: 6002 });
  });

  it("answers null for a card that is absent or unreadable", async () => {
    const { driver } = paneStage(paneOf({ '[data-stat="words"] h6': at({ innerText: async () => "lots" }) }));

    await expect(driver.pagesBlocksInfoStats(undefined, true)).resolves.toEqual({ words: null, characters: null });
  });

  it("polls the character card until it reports the expected count", async () => {
    let reads = 0;
    const characters = at({
      innerText: async () => {
        reads += 1;
        return reads === 1 ? "0 characters" : "42 characters";
      },
    });
    const { driver } = paneStage(paneOf({ '[data-stat="characters"] h6': characters }));

    await expect(driver.pagesBlocksInfoStats(42, true)).resolves.toEqual({ words: null, characters: 42 });
    expect(reads).toBeGreaterThan(1);
  });

  it("navigates to the pane tab unless the caller already did", async () => {
    const { driver, stub } = paneStage(fake());

    await driver.pagesBlocksInfoStats();

    expect(stub.goto).toHaveBeenCalledWith(expect.stringContaining("paneTab=info"), { waitUntil: "domcontentloaded" });
    expect(stub.goto).toHaveBeenCalledTimes(1);
  });
});

describe("embeds", () => {
  it("reports the upgrade card only when an embed carries it", async () => {
    const upgraded = editorOf({ "issue-embed-component": at({ locator: () => at() }) });
    const plain = editorOf({ "issue-embed-component": at({ locator: () => at({ count: 0 }) }) });
    const none = editorOf({ "issue-embed-component": at({ count: 0 }) });

    await expect(stage({ editor: upgraded }).driver.pagesBlocksEmbedUpgradeVisible()).resolves.toBe(true);
    await expect(stage({ editor: plain }).driver.pagesBlocksEmbedUpgradeVisible()).resolves.toBe(false);
    await expect(stage({ editor: none }).driver.pagesBlocksEmbedUpgradeVisible()).resolves.toBe(false);
  });

  it("reads the upgrade card's address, or null when there is none", async () => {
    const link = at({ getAttribute: async () => "/acme/browse/ENG-7" });
    const withLink = editorOf({
      "issue-embed-component": at({ locator: () => at({ first: () => link }) }),
    });
    const withoutLink = editorOf({
      "issue-embed-component": at({ locator: () => at({ count: 0 }) }),
    });
    const noEmbed = editorOf({ "issue-embed-component": at({ count: 0 }) });

    await expect(stage({ editor: withLink }).driver.pagesBlocksEmbedUpgradeHref()).resolves.toBe("/acme/browse/ENG-7");
    await expect(stage({ editor: withoutLink }).driver.pagesBlocksEmbedUpgradeHref()).resolves.toBeNull();
    await expect(stage({ editor: noEmbed }).driver.pagesBlocksEmbedUpgradeHref()).resolves.toBeNull();
  });
});

describe("mentions", () => {
  it("suggests mention rows after the query, normalising their text", async () => {
    const items = at({ allTextContents: async () => ["Ada\n  Lovelace", "", "Grace Hopper"] });
    const { driver, stub } = stage({ selectors: { [MENTION_ITEM_PREFIX]: items } });

    await expect(driver.pagesBlocksMentionSuggest("ad")).resolves.toEqual(["Ada Lovelace", "Grace Hopper"]);
    expect(stub.type).toHaveBeenCalledWith("@ad");
  });

  it("chooses a mention by display name and waits for its chip", async () => {
    const item = fake();
    const chip = fake();
    const { driver } = stage({
      editor: editorOf({ "mention-component": at({ first: () => chip.asLocator() }) }),
      selectors: { [MENTION_ITEM_PREFIX]: at({ first: () => item.asLocator() }) },
    });

    await driver.pagesBlocksMentionChoose("Ada Lovelace");

    expect(item.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(chip.waitFor).toHaveBeenCalledWith({ state: "attached", timeout: 30_000 });
  });

  it("reads chip text and their link addresses", async () => {
    const firstLink = at({ getAttribute: async () => "/acme/members/1" });
    const secondLink = at({ getAttribute: async () => null });
    const links = at({ count: 2, nth: (index) => (index === 0 ? firstLink : secondLink) });
    const { driver } = stage({
      editor: editorOf({
        "mention-component": at({ allTextContents: async () => [" Ada\nLovelace ", ""] }),
        "mention-component a": links,
      }),
    });

    await expect(driver.pagesBlocksMentionChips()).resolves.toEqual(["Ada Lovelace"]);
    await expect(driver.pagesBlocksMentionChipHrefs()).resolves.toEqual(["/acme/members/1", ""]);
  });

  it("reads the suggestion sections, or none when the popup is closed", async () => {
    const closed = stage();
    await expect(closed.driver.pagesBlocksMentionSections()).resolves.toEqual([]);

    const popup = at({ locator: () => at({ allTextContents: async () => [" People ", ""] }) });
    const open = stage({ selectors: { '[data-testid="page-mention-suggestions"]': popup } });
    await expect(open.driver.pagesBlocksMentionSections()).resolves.toEqual(["People"]);
  });
});

describe("links and the selection bubble", () => {
  it("drags across a link and waits for its Link control", async () => {
    const link = at({ boundingBox: async () => ({ x: 100, y: 50, width: 60, height: 20 }) });
    const linkButton = fake();
    // The bubble hook is a zero-size positioning shell, so the driver waits
    // on the Link control it carries rather than on the shell (PAGE-043).
    const bubble = fake({ getByRole: () => at({ first: () => linkButton.asLocator() }) });
    const { driver, stub } = stage({
      editor: editorOf({ a: at({ first: () => link }) }),
      selectors: { [BUBBLE_SELECTOR]: bubble.asLocator() },
    });

    await driver.pagesBlocksLinkSelect();

    expect(stub.mouseDown).toHaveBeenCalled();
    expect(stub.mouseUp).toHaveBeenCalled();
    expect(linkButton.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
    expect(bubble.waitFor).not.toHaveBeenCalled();
  });

  it("refuses to select a link that has no painted box", async () => {
    const { driver } = stage({ editor: editorOf({ a: at({ first: () => at({ boundingBox: async () => null }) }) }) });

    await expect(driver.pagesBlocksLinkSelect()).rejects.toThrow("no painted box");
  });

  it("reports the bubble visible only with its Link control painted", async () => {
    const linkButton = fake({ isVisible: async () => true });
    const shown = stage({
      selectors: { [BUBBLE_SELECTOR]: at({ getByRole: () => at({ first: () => linkButton.asLocator() }) }) },
    });
    await expect(shown.driver.pagesBlocksLinkBubbleVisible()).resolves.toBe(true);

    await expect(stage().driver.pagesBlocksLinkBubbleVisible()).resolves.toBe(false);
  });

  it("reads a link row's address, or null when the body has no link", async () => {
    const link = at({ getAttribute: async () => "https://example.test" });
    await expect(
      stage({ editor: editorOf({ a: at({ first: () => link }) }) }).driver.pagesBlocksLinkHref()
    ).resolves.toBe("https://example.test");

    await expect(stage({ editor: editorOf({ a: at({ count: 0 }) }) }).driver.pagesBlocksLinkHref()).resolves.toBeNull();
  });

  it("reads the pre-filled link without reopening fields that are already open", async () => {
    const input = fake({ inputValue: async () => "https://example.test/old" });
    const linkButton = fake();
    const { driver } = stage({
      selectors: {
        [BUBBLE_SELECTOR]: at({ getByRole: () => at({ first: () => linkButton.asLocator() }) }),
        '[data-testid="page-link-bubble"] input[type="url"]': at({ count: 1, first: () => input.asLocator() }),
      },
    });

    await expect(driver.pagesBlocksLinkPrefill()).resolves.toBe("https://example.test/old");
    expect(linkButton.click).not.toHaveBeenCalled();
  });

  it("clicks the Link control when the fields are shut", async () => {
    let open = false;
    const input = fake({
      count: () => (open ? 1 : 0),
      inputValue: async () => "https://example.test",
    });
    const linkButton = fake({
      click: async () => {
        open = true;
      },
    });
    const { driver } = stage({
      selectors: {
        [BUBBLE_SELECTOR]: at({ getByRole: () => at({ first: () => linkButton.asLocator() }) }),
        '[data-testid="page-link-bubble"] input[type="url"]': input.asLocator(),
      },
    });

    await expect(driver.pagesBlocksLinkPrefill()).resolves.toBe("https://example.test");
    expect(linkButton.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("edits a link and waits for the fields to close", async () => {
    let open = true;
    const input = fake({ count: () => (open ? 1 : 0) });
    const { driver, stub } = stage({
      selectors: {
        [BUBBLE_SELECTOR]: at({ getByRole: () => at({ first: () => at() }) }),
        '[data-testid="page-link-bubble"] input[type="url"]': input.asLocator(),
      },
    });
    stub.press.mockImplementation(async () => {
      open = false;
    });

    await driver.pagesBlocksLinkEdit("https://example.test/new");

    expect(input.fill).toHaveBeenCalledWith("https://example.test/new");
    expect(open).toBe(false);
  });

  it("removes a link through the bubble's own control", async () => {
    const remove = fake();
    const { driver, stub } = stage({
      selectors: {
        '[data-testid="page-link-bubble"]': at({ getByRole: () => remove.asLocator() }),
        '[data-testid="page-link-bubble"] input[type="url"]': at({ count: 1 }),
      },
    });

    await driver.pagesBlocksLinkRemove();

    expect(remove.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.press).not.toHaveBeenCalled();
  });

  it("runs a bubble format command by its index among the labelled controls", async () => {
    const button = fake();
    const { driver } = stage({
      selectors: {
        [BUBBLE_SELECTOR]: at({
          locator: (selector: string) =>
            selector === "button[aria-label]" ? at({ nth: () => button.asLocator() }) : empty(),
        }),
      },
    });

    await driver.pagesBlocksBubbleFormat(2);

    expect(button.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("header options", () => {
  it("opens the options menu when it is shut", async () => {
    let open = false;
    const trigger = fake({
      click: async () => {
        open = true;
      },
    });
    const items = at({ count: () => (open ? 3 : 0) });
    const { driver, stub } = stage({
      roles: { "button:Page options": at({ first: () => trigger.asLocator() }), menuitem: items },
    });

    await driver.pagesBlocksOptionsOpen();

    expect(trigger.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.waitForTimeout).toHaveBeenCalled();
  });

  it("leaves an already-open menu alone", async () => {
    const trigger = fake();
    const { driver } = stage({
      roles: { "button:Page options": at({ first: () => trigger.asLocator() }), menuitem: at({ count: 2 }) },
    });

    await driver.pagesBlocksOptionsOpen();

    expect(trigger.click).not.toHaveBeenCalled();
  });

  it("reads the item labels deduplicated, then closes the menu", async () => {
    const items = at({ count: 2, allTextContents: async () => [" Copy\nmarkdown ", "Copy markdown", ""] });
    const { driver, stub } = stage({
      roles: { "button:Page options": at({ first: () => at() }), menuitem: items },
    });

    await expect(driver.pagesBlocksOptionsItems()).resolves.toEqual(["Copy markdown"]);
    expect(stub.press).toHaveBeenCalledWith("Escape");
  });

  it("copies markdown and waits for its notice", async () => {
    const copy = fake();
    const card = fake({ innerText: async () => "Copied\nMarkdown copied to clipboard." });
    const { driver } = stage({
      roles: {
        "button:Page options": at({ first: () => at() }),
        menuitem: at({ count: 1 }),
        "menuitem:Copy markdown": copy.asLocator(),
      },
      selectors: { [TOAST_SELECTOR]: at({ count: 1, nth: () => card.asLocator() }) },
    });

    await driver.pagesBlocksOptionsCopyMarkdown();

    expect(copy.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads the toast text, or null when there is no stack or nothing on it", async () => {
    const shown = at({ textContent: async () => "  Saved\n the page  " });
    const blank = at({ textContent: async () => "   " });
    const stack = (value: Locator) =>
      stage({ selectors: { '[data-testid="toast-stack"]': at({ first: () => value }) } });

    await expect(stack(shown).driver.pagesBlocksToastText()).resolves.toBe("Saved the page");
    await expect(stack(blank).driver.pagesBlocksToastText()).resolves.toBeNull();
    await expect(stage().driver.pagesBlocksToastText()).resolves.toBeNull();
  });

  it("reads the clipboard", async () => {
    const { driver } = stage({ page: { evaluate: async () => "# Copied" } });

    await expect(driver.pagesBlocksClipboardText()).resolves.toBe("# Copied");
  });

  it("cycles the header lock and waits for its notice to clear", async () => {
    const lock = fake();
    const locked = fake();
    const unlocked = fake();
    let toastReads = 0;
    const { driver } = stage({
      roles: {
        "button:Locked": at({ first: () => locked.asLocator() }),
        "button:Lock": at({ first: () => unlocked.asLocator() }),
      },
      selectors: {
        '[data-testid="page-detail-lock"]': lock.asLocator(),
        '[data-testid="toast-stack"]': at({
          first: () => {
            toastReads += 1;
            return toastReads === 1 ? at({ textContent: async () => "Page locked" }) : empty();
          },
        }),
      },
    });

    await driver.pagesBlocksHeaderLockCycle();

    expect(lock.scrollIntoViewIfNeeded).toHaveBeenCalled();
    expect(lock.click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(locked.click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

// The probes below run the driver's own browser-side callbacks against real
// jsdom nodes, which is the only way to exercise their branches: the waits,
// the caret probe, the paste watch, the clipboard and the block hovers.
describe("browser-side callbacks", () => {
  afterEach(() => {
    // The paste watcher keeps its state on the window between drives.
    delete (window as unknown as Record<string, unknown>).__pagesBlocksPasteState;
  });

  /** A locator whose evaluate runs the callback against a real element. */
  function domLocator(element: Element, options: LocatorOptions = {}): Locator {
    return new FakeLocator({
      ...options,
      evaluate: async (fn: (node: unknown, arg?: unknown) => unknown, arg?: unknown) => fn(element, arg),
    }).asLocator();
  }

  /** A body whose editor is a real element under the canvas. */
  function domStage(): { driver: WebNewDriver; stub: PageDouble; editor: HTMLElement; canvas: HTMLElement } {
    document.body.innerHTML = "";
    const canvas = document.createElement("div");
    canvas.id = "page-content-container";
    const container = document.createElement("div");
    container.className = "editor-container";
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "true");
    const paragraph = document.createElement("p");
    paragraph.textContent = "Body";
    editor.append(paragraph);
    container.append(editor);
    canvas.append(container);
    document.body.append(canvas);

    const stub = pageDouble({
      evaluate: async (fn: (arg?: unknown) => unknown, arg?: unknown) => fn(arg),
    });
    (stub.page as unknown as { locator: (selector: string) => Locator }).locator = (selector: string) => {
      if (selector === CANVAS_SELECTOR) return domLocator(canvas);
      if (selector === EDITOR_SELECTOR) return domLocator(editor);
      return empty();
    };
    return { driver: new WebNewDriver(stub.page), stub, editor, canvas };
  }

  it("settles once the body stops mutating, and swallows a detached body", async () => {
    const { driver } = domStage();
    vi.useFakeTimers();
    try {
      const settle = (driver as unknown as { pagesBlocksWaitSettled(): Promise<void> }).pagesBlocksWaitSettled();
      const before = await Promise.race([settle.then(() => "settled" as const), Promise.resolve("pending" as const)]);
      expect(before).toBe("pending");
      await vi.advanceTimersByTimeAsync(699);
      expect(await Promise.race([settle.then(() => "settled" as const), Promise.resolve("pending" as const)])).toBe(
        "pending"
      );
      await vi.advanceTimersByTimeAsync(1);
      await expect(settle).resolves.toBeUndefined();
    } finally {
      vi.useRealTimers();
    }

    // A body that throws on evaluate is not an error: the wait is best effort.
    const stub = pageDouble();
    (stub.page as unknown as { locator: () => Locator }).locator = () =>
      new FakeLocator({ evaluate: async () => Promise.reject(new Error("gone")) }).asLocator();
    await expect(
      (new WebNewDriver(stub.page) as unknown as { pagesBlocksWaitSettled(): Promise<void> }).pagesBlocksWaitSettled()
    ).resolves.toBeUndefined();
  });

  it("focuses the editor when the focus sits outside it, and reports no editor without one", async () => {
    const { driver, editor } = domStage();

    expect(await (driver as unknown as { pagesBlocksFocusEditor(): Promise<boolean> }).pagesBlocksFocusEditor()).toBe(
      true
    );
    expect(document.activeElement).toBe(editor);

    // Already focused: no second focus call, just a true answer.
    expect(await (driver as unknown as { pagesBlocksFocusEditor(): Promise<boolean> }).pagesBlocksFocusEditor()).toBe(
      true
    );

    // With no editor in the document the probe refuses rather than focusing
    // something else.
    document.body.innerHTML = "";
    expect(await (driver as unknown as { pagesBlocksFocusEditor(): Promise<boolean> }).pagesBlocksFocusEditor()).toBe(
      false
    );
  });

  it("answers the caret probe from a real selection at the end of the last block", async () => {
    const { driver, editor } = domStage();
    const caret = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksCaretAtEnd(): Promise<boolean> }).pagesBlocksCaretAtEnd();
    const paragraph = editor.querySelector("p") as HTMLElement;
    const select = (node: Node, offset: number): void => {
      const range = document.createRange();
      range.setStart(node, offset);
      range.collapse(true);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    };

    const text = paragraph.firstChild as Text;
    select(text, text.length);
    await expect(caret(driver)).resolves.toBe(true);

    // Mid-text is not the end.
    select(text, 1);
    await expect(caret(driver)).resolves.toBe(false);

    // A spread selection is not a caret.
    const wide = document.createRange();
    wide.setStart(text, 0);
    wide.setEnd(text, 2);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(wide);
    await expect(caret(driver)).resolves.toBe(false);

    // An empty selection list is not a caret either.
    window.getSelection()?.removeAllRanges();
    await expect(caret(driver)).resolves.toBe(false);

    // A block ending in an element (not text) puts the caret on the block.
    paragraph.textContent = "";
    const nested = document.createElement("span");
    nested.textContent = "nested";
    paragraph.append(nested);
    select(paragraph, 1);
    await expect(caret(driver)).resolves.toBe(true);

    // A body with no blocks has no end.
    editor.innerHTML = "";
    select(editor, 0);
    await expect(caret(driver)).resolves.toBe(false);
  });

  it("dispatches a real paste event carrying the caller's markup", async () => {
    const { driver, editor } = domStage();
    // jsdom ships no DataTransfer or ClipboardEvent constructors of its own.
    vi.stubGlobal(
      "DataTransfer",
      class {
        private readonly data = new Map<string, string>();
        setData(type: string, value: string): void {
          this.data.set(type, value);
        }
        getData(type: string): string {
          return this.data.get(type) ?? "";
        }
      }
    );
    vi.stubGlobal(
      "ClipboardEvent",
      class extends Event {
        readonly clipboardData: unknown;
        constructor(type: string, init?: { clipboardData?: unknown }) {
          super(type, init);
          this.clipboardData = init?.clipboardData;
        }
      }
    );
    const seen: string[] = [];
    editor.addEventListener("paste", (event) => {
      seen.push((event as ClipboardEvent).clipboardData?.getData("text/html") ?? "");
    });

    vi.useFakeTimers();
    try {
      const pasting = driver.pagesBlocksEditorPasteHtml("<h2>Heading</h2>");
      await vi.runAllTimersAsync();
      await pasting;
    } finally {
      vi.useRealTimers();
    }

    expect(seen).toEqual(["<h2>Heading</h2>"]);
    vi.unstubAllGlobals();
  });

  it("arms the paste watch, records the paste, then the mutation", async () => {
    const { driver, stub, editor } = domStage();
    const arm = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksArmPasteWatch(): Promise<void> }).pagesBlocksArmPasteWatch();
    const landed = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksWaitPasteLanded(): Promise<void> }).pagesBlocksWaitPasteLanded();

    await arm(driver);
    expect((window as unknown as { __pagesBlocksPasteState?: { seen: boolean } }).__pagesBlocksPasteState?.seen).toBe(
      false
    );

    const arriving = landed(driver);
    document.dispatchEvent(new Event("paste", { bubbles: true }));
    editor.append(document.createElement("p"));
    await expect(arriving).resolves.toBeUndefined();
    expect(
      (window as unknown as { __pagesBlocksPasteState?: { mutated: boolean } }).__pagesBlocksPasteState?.mutated
    ).toBe(true);

    // Arming with no editor leaves the watcher without an observer, which is
    // not an error: the paste is still recorded.
    (stub.page as unknown as { locator: () => Locator }).locator = () => empty();
    await arm(driver);
    document.dispatchEvent(new Event("paste", { bubbles: true }));
    expect((window as unknown as { __pagesBlocksPasteState?: { seen: boolean } }).__pagesBlocksPasteState?.seen).toBe(
      true
    );
  });

  it("lets a paste that never lands fall through instead of failing", async () => {
    const { driver, stub } = domStage();
    (stub.page as unknown as { locator: () => Locator }).locator = () => empty();
    delete (window as unknown as Record<string, unknown>).__pagesBlocksPasteState;
    const landed = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksWaitPasteLanded(): Promise<void> }).pagesBlocksWaitPasteLanded();

    // Nothing armed, nothing landed: the wait polls until its budget runs out
    // and then returns instead of failing. The budget is virtual here.
    vi.useFakeTimers();
    try {
      const wait = landed(driver);
      const outcome = (): Promise<"settled" | "pending"> =>
        Promise.race([wait.then(() => "settled" as const), Promise.resolve("pending" as const)]);
      expect(await outcome()).toBe("pending");
      await vi.advanceTimersByTimeAsync(10_000);
      await expect(wait).resolves.toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
    expect((window as unknown as { __pagesBlocksPasteState?: unknown }).__pagesBlocksPasteState).toBeUndefined();
  });

  it("reads the clipboard through the browser", async () => {
    const { driver } = domStage();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText: async () => "# Copied", writeText: async () => undefined },
    });

    await expect(driver.pagesBlocksClipboardText()).resolves.toBe("# Copied");
  });

  it("skips a block whose tag or text cannot be read", async () => {
    const unreadableTag = new FakeLocator({
      count: 1,
      evaluate: async () => Promise.reject(new Error("detached")),
      innerText: async () => "Text",
    });
    const blocks = new FakeLocator({ count: 1, nth: () => unreadableTag.asLocator() });
    const { driver } = stage({ editor: editorOf({ "xpath=./*": blocks.asLocator() }) });

    await (driver as unknown as { pagesBlocksHoverLastBlock(): Promise<void> }).pagesBlocksHoverLastBlock();

    expect(unreadableTag.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("treats a throwing visibility read as hidden", async () => {
    const stageOne = stage({
      editor: editorOf({ "xpath=./*": at({ count: 0 }) }),
      selectors: {
        [HANDLE_SELECTOR]: at({
          isVisible: async () => Promise.reject(new Error("detached")),
        }),
      },
    });

    await expect(stageOne.driver.pagesBlocksDragHandleVisible()).resolves.toBe(false);
  });

  it("treats a throwing empty-state and bubble read as hidden", async () => {
    const throwing = { isVisible: async () => Promise.reject(new Error("detached")) };
    const pane = stage({
      selectors: { [PANE_SELECTOR]: paneOf({ '[data-testid="page-assets-empty"]': at({ ...throwing }) }).first() },
    });
    await expect(pane.driver.pagesBlocksAssetsTabEmptyVisible()).resolves.toBe(false);

    const bubble = stage({
      selectors: {
        [BUBBLE_SELECTOR]: at({ getByRole: () => at({ first: () => at({ ...throwing }) }) }),
      },
    });
    await expect(bubble.driver.pagesBlocksLinkBubbleVisible()).resolves.toBe(false);
  });

  it("counts an unparseable stats card as null", async () => {
    const pane = fake({ locator: () => at({ innerText: async () => "no digits here" }) });
    const { driver } = stage({ selectors: { [PANE_SELECTOR]: pane.asLocator() } });

    await expect(driver.pagesBlocksInfoStats(undefined, true)).resolves.toEqual({ words: null, characters: null });
  });
  it("re-arms the settle window on each mutation before it quiets", async () => {
    const { driver, editor } = domStage();
    const settle = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksWaitSettled(): Promise<void> }).pagesBlocksWaitSettled();
    // A late application re-arms the timer, so the quiet window restarts.
    vi.useFakeTimers();
    try {
      const waiting = settle(driver);
      await vi.advanceTimersByTimeAsync(300);
      editor.append(document.createElement("p"));
      await vi.advanceTimersByTimeAsync(699);
      expect(await Promise.race([waiting.then(() => "settled" as const), Promise.resolve("pending" as const)])).toBe(
        "pending"
      );
      await vi.advanceTimersByTimeAsync(1);
      await waiting;
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips a paragraph whose box cannot be measured", async () => {
    const throws = fake({ boundingBox: async () => Promise.reject(new Error("detached")) });
    const measured = fake({ boundingBox: async () => ({ x: 0, y: 0, width: 100, height: 20 }) });
    const paragraphs = at({ count: 2, nth: (index) => (index === 1 ? throws.asLocator() : measured.asLocator()) });
    const pointers = at({ count: 0, nth: () => throws.asLocator() });
    const editor = editorOf({ p: paragraphs }, { evaluate: async () => true });
    const { driver } = stage({ editor });

    await driver.pagesBlocksEditorClickEnd();

    expect(throws.click).not.toHaveBeenCalled();
    expect(measured.click).toHaveBeenCalled();
    void pointers;
  });

  it("arms the paste watch with no editor in the document", async () => {
    const { driver } = domStage();
    document.body.innerHTML = "";
    const arm = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksArmPasteWatch(): Promise<void> }).pagesBlocksArmPasteWatch();

    await arm(driver);
    document.dispatchEvent(new Event("paste", { bubbles: true }));

    expect(
      (window as unknown as { __pagesBlocksPasteState?: { seen: boolean; mutated: boolean } }).__pagesBlocksPasteState
    ).toEqual({ seen: true, mutated: false });
  });

  it("runs the clipboard write and read through the browser's own clipboard", async () => {
    const { driver, stub } = domStage();
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (value: string) => {
          written.push(value);
        },
        readText: async () => written.at(-1) ?? "",
      },
    });
    const landed = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksWaitPasteLanded(): Promise<void> }).pagesBlocksWaitPasteLanded();
    const settle = (d: WebNewDriver) =>
      (d as unknown as { pagesBlocksWaitSettled(): Promise<void> }).pagesBlocksWaitSettled();
    (driver as unknown as { pagesBlocksWaitPasteLanded(): Promise<void> }).pagesBlocksWaitPasteLanded = async () =>
      undefined;
    (driver as unknown as { pagesBlocksWaitSettled(): Promise<void> }).pagesBlocksWaitSettled = async () => undefined;

    await driver.pagesBlocksEditorPasteText("fresh text");

    expect(written).toEqual(["fresh text"]);
    expect(stub.press).toHaveBeenCalledWith("ControlOrMeta+v");
    void landed;
    void settle;
  });

  it("hovers a real block after centring it, reading its tag through the DOM", async () => {
    const { driver, editor } = domStage();
    const paragraph = editor.querySelector("p") as HTMLElement;
    const scrolled: string[] = [];
    paragraph.scrollIntoView = ((options?: ScrollIntoViewOptions) => {
      scrolled.push(String(options?.block));
    }) as HTMLElement["scrollIntoView"];
    const block = new FakeLocator({
      count: 1,
      nth: () => domLocator(paragraph, { innerText: async () => "Body" }),
    });
    const hoverEditor = at({ locator: (selector: string) => (selector === "xpath=./*" ? block.asLocator() : empty()) });
    (driver as unknown as { pagesBlocksEditor(): Locator }).pagesBlocksEditor = () => hoverEditor;

    await (driver as unknown as { pagesBlocksHoverLastBlock(): Promise<void> }).pagesBlocksHoverLastBlock();

    expect(scrolled).toEqual(["center"]);
  });

  it("leaves the caret probe alone when the last block ends in text", async () => {
    const { driver, editor } = domStage();
    const paragraph = editor.querySelector("p") as HTMLElement;
    const text = paragraph.firstChild as Text;
    const range = document.createRange();
    range.setStart(text, 0);
    range.collapse(true);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);

    await expect(
      (driver as unknown as { pagesBlocksCaretAtEnd(): Promise<boolean> }).pagesBlocksCaretAtEnd()
    ).resolves.toBe(false);
  });

  it("treats a null inner text as empty when naming assets and reading stats", async () => {
    const pane = fake({
      locator: (selector: string) => {
        if (selector === '[data-testid="page-asset-row"]') {
          return at({
            nth: () => at({ locator: () => at({ innerText: async () => null as unknown as string }) }),
          });
        }
        return at({ innerText: async () => Promise.reject(new Error("detached")) });
      },
    });
    const { driver } = stage({ selectors: { [PANE_SELECTOR]: pane.asLocator() } });

    await expect(driver.pagesBlocksAssetsTabRows()).resolves.toEqual([]);
    await expect(driver.pagesBlocksInfoStats(undefined, true)).resolves.toEqual({ words: null, characters: null });
  });

  it("reports an image with no source as an empty string", async () => {
    const image = fake({ getAttribute: async (name: string) => (name === "alt" ? "Alt" : null) });
    const { driver } = stage({ editor: editorOf({ img: at({ count: 1, nth: () => image.asLocator() }) }) });

    await expect(driver.pagesBlocksInlineImages()).resolves.toEqual([{ src: "", alt: "Alt" }]);
  });

  it("polls past an empty toast before the copy notice arrives", async () => {
    const copy = fake();
    let reads = 0;
    const card = fake({
      isVisible: async () => true,
      innerText: async () => {
        reads += 1;
        return reads === 1 ? "" : "Copied\nMarkdown copied to clipboard.";
      },
    });
    const { driver } = stage({
      roles: {
        "button:Page options": at({ first: () => at() }),
        menuitem: at({ count: 1 }),
        "menuitem:Copy markdown": copy.asLocator(),
      },
      selectors: { [TOAST_SELECTOR]: at({ count: 1, nth: () => card.asLocator() }) },
    });

    await driver.pagesBlocksOptionsCopyMarkdown();

    expect(reads).toBeGreaterThan(1);
  });
  it("reports a missing body when the refusal panel never paints either", async () => {
    const editor = fake({ waitFor: () => new Promise<void>(() => undefined) });
    const refusal = fake({
      waitFor: async () => {
        throw new Error("never painted");
      },
    });
    const { driver } = stage({
      editor: editor.asLocator(),
      selectors: { '[data-testid="page-detail-panel"]': refusal.asLocator() },
    });

    await expect(driver.pagesBlocksOpenDetail("acme", "p", "g")).rejects.toThrow("never mounted");
  });

  it("falls back to the last block when its text probe answers null", async () => {
    const block = fake({
      evaluate: async () => "p",
      innerText: async () => null as unknown as string,
    });
    const blocks = at({ count: 1, nth: () => block.asLocator(), last: () => block.asLocator() });
    const { driver } = stage({ editor: editorOf({ "xpath=./*": blocks }) });

    await (driver as unknown as { pagesBlocksHoverLastBlock(): Promise<void> }).pagesBlocksHoverLastBlock();

    expect(block.hover).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(block.scrollIntoViewIfNeeded).not.toHaveBeenCalled();
  });

  it("reads a block whose text probe throws as empty", async () => {
    const block = fake({
      evaluate: async () => "h2",
      innerText: async () => Promise.reject(new Error("detached")),
    });
    const blocks = at({ count: 1, nth: () => block.asLocator() });
    const { driver } = stage({ editor: editorOf({ "xpath:./*": at() }) });
    (driver as unknown as { pagesBlocksEditor(): Locator }).pagesBlocksEditor = () =>
      at({ locator: () => blocks.asLocator() });

    await (driver as unknown as { pagesBlocksHoverLastBlock(): Promise<void> }).pagesBlocksHoverLastBlock();

    expect(block.hover).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("counts an asset row and stats card whose text probe throws as empty", async () => {
    const pane = fake({
      locator: (selector: string) => {
        if (selector === '[data-testid="page-asset-row"]') {
          return at({
            locator: () => at({ innerText: async () => Promise.reject(new Error("detached")) }),
          });
        }
        return at({ innerText: async () => Promise.reject(new Error("detached")) });
      },
    });
    const { driver } = stage({ selectors: { [PANE_SELECTOR]: pane.asLocator() } });

    await expect(driver.pagesBlocksAssetsTabRows()).resolves.toEqual([]);
    await expect(driver.pagesBlocksInfoStats(undefined, true)).resolves.toEqual({ words: null, characters: null });
  });

  it("treats a last block with no children as having no trailing text", async () => {
    const { driver } = domStage();
    const editor = document.querySelector(EDITOR_SELECTOR) as HTMLElement;
    editor.innerHTML = "<p></p>";
    const block = editor.querySelector("p") as HTMLElement;
    const range = document.createRange();
    range.setStart(block, 0);
    range.collapse(true);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);

    await expect(
      (driver as unknown as { pagesBlocksCaretAtEnd(): Promise<boolean> }).pagesBlocksCaretAtEnd()
    ).resolves.toBe(false);
  });
  it("reads an undefined asset name or stats value as empty", async () => {
    const pane = fake({
      locator: (selector: string) => {
        const undefinedText = async () => undefined as unknown as string;
        if (selector === '[data-testid="page-asset-row"]') {
          return at({ nth: () => at({ locator: () => at({ innerText: undefinedText }) }) });
        }
        return at({ innerText: undefinedText });
      },
    });
    const { driver } = stage({ selectors: { [PANE_SELECTOR]: pane.asLocator() } });

    await expect(driver.pagesBlocksAssetsTabRows()).resolves.toEqual([]);
    await expect(driver.pagesBlocksInfoStats(undefined, true)).resolves.toEqual({ words: null, characters: null });
  });
});
