// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver prompt-section methods
// (NEWFRONT-597, AGT-023–027/033). These tests use a small Locator/Page
// double so selector, read, action and network-control branches are proven
// without bringing up the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => vi.restoreAllMocks());

interface FakeLocatorOptions {
  count?: number;
  visible?: boolean;
  text?: string;
  input?: string;
  enabled?: boolean;
  attributes?: Record<string, string | null>;
  items?: FakeLocator[];
  locator?: (selector: string) => FakeLocator;
  role?: (role: string, options?: { name?: string | RegExp; exact?: boolean; level?: number }) => FakeLocator;
  textLocator?: (text: string | RegExp, options?: { exact?: boolean }) => FakeLocator;
  testIdLocator?: (testId: string) => FakeLocator;
  filtered?: FakeLocator;
  /** Make reads reject, as a detached or removed element would. */
  failing?: boolean;
  /** Report the element as present while its reads reject. */
  counted?: number;
  /** Make the enabled read reject, as a detached control would. */
  isEnabledRejects?: boolean;
}

class FakeLocator {
  readonly click = vi.fn(async () => undefined);
  readonly fill = vi.fn(async (value: string) => {
    this.value = value;
  });
  readonly waitFor = vi.fn(async (_options?: unknown) => undefined);
  private value: string;

  constructor(private readonly options: FakeLocatorOptions = {}) {
    this.value = options.input ?? "";
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.items?.length ?? this.options.counted ?? this.options.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    if (this.options.failing === true && this.options.visible === undefined) {
      return Promise.reject(new Error("detached"));
    }
    return Promise.resolve(this.options.visible ?? (this.options.counted ?? this.options.count ?? 1) > 0);
  }

  innerText(): Promise<string> {
    return this.options.failing === true
      ? Promise.reject(new Error("detached"))
      : Promise.resolve(this.options.text ?? "");
  }

  getAttribute(name: string): Promise<string | null> {
    return this.options.failing === true
      ? Promise.reject(new Error("detached"))
      : Promise.resolve(this.options.attributes?.[name] ?? null);
  }

  hrefKey(): string | null {
    return this.options.attributes?.href?.replace(/^#prompt-section-/, "") ?? null;
  }

  /** The receipt kind this link anchors, from its href. */
  receiptKey(): string | null {
    return this.options.attributes?.href?.replace(/^#prompt-receipt-/, "") ?? null;
  }

  /** The element's rendered id, e.g. `prompt-receipt-review`. */
  idValue(): string | null {
    return this.options.attributes?.id ?? null;
  }

  inputValue(): Promise<string> {
    return Promise.resolve(this.value);
  }

  isEnabled(): Promise<boolean> {
    if (this.options.isEnabledRejects === true) return Promise.reject(new Error("detached"));
    return Promise.resolve(this.options.enabled ?? true);
  }

  first(): FakeLocator {
    return this.options.items?.[0] ?? this;
  }

  last(): FakeLocator {
    return this.options.items?.at(-1) ?? this;
  }

  nth(index: number): FakeLocator {
    return this.options.items?.[index] ?? absent();
  }

  locator(selector: string): FakeLocator {
    return this.options.locator?.(selector) ?? absent();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean; level?: number }): FakeLocator {
    return this.options.role?.(role, options) ?? absent();
  }

  getByText(text: string | RegExp, options?: { exact?: boolean }): FakeLocator {
    return this.options.textLocator?.(text, options) ?? absent();
  }

  getByTestId(testId: string): FakeLocator {
    return this.options.testIdLocator?.(testId) ?? absent();
  }

  filter(_options: unknown): FakeLocator {
    return this.options.filtered ?? this;
  }

  or(other: FakeLocator): FakeLocator {
    const present = (this.options.count ?? 1) > 0 && (this.options.visible ?? true);
    return present ? this : other;
  }
}

function absent(): FakeLocator {
  return new FakeLocator({ count: 0, visible: false });
}

function routeDouble(method: string, url: string) {
  return {
    request: () => ({ method: () => method, url: () => url }),
    fulfill: vi.fn(async (_options: unknown) => undefined),
    continue: vi.fn(async () => undefined),
  };
}

function navigationPage(options?: {
  sectionsVisible?: boolean;
  receiptVisible?: boolean;
  hash?: string;
  links?: FakeLocator[];
  cards?: FakeLocator[];
  cardByKey?: Record<string, FakeLocator>;
}) {
  const heading = new FakeLocator();
  const sections = new FakeLocator({
    visible: options?.sectionsVisible ?? true,
    role: (role) => (role === "link" ? new FakeLocator({ items: options?.links ?? [] }) : absent()),
    locator: (selector) => {
      const key = selector.match(/#prompt-section-([^"\]]+)/)?.[1];
      return key === undefined ? absent() : ((options?.links ?? []).find((link) => link.hrefKey() === key) ?? absent());
    },
  });
  const receipt = new FakeLocator({ visible: options?.receiptVisible ?? false });
  const buttons = new Map<string, FakeLocator>();
  const page = {
    goto: vi.fn(async (_url: string) => undefined),
    evaluate: vi.fn(async (_callback: unknown) => options?.hash ?? "#prompt-section-alpha"),
    getByRole: vi.fn((role: string, roleOptions?: { name?: string | RegExp }) => {
      if (role === "heading" && roleOptions?.name === "Prompts") return heading;
      if (role === "navigation" && roleOptions?.name === "Prompt sections") return sections;
      if (role === "navigation" && roleOptions?.name === "Prompt receipt") return receipt;
      // The prompts tabs carry role=tab; a button-role lookup finds nothing.
      if (role === "button" || role === "tab") {
        const name = String(roleOptions?.name);
        const button = buttons.get(name) ?? new FakeLocator();
        buttons.set(name, button);
        return button;
      }
      return absent();
    }),
    getByText: vi.fn(() => absent()),
    locator: vi.fn((selector: string) => {
      if (selector === 'article[id^="prompt-section-"]') return new FakeLocator({ items: options?.cards ?? [] });
      const key = selector.match(/^article\[id="prompt-section-(.*)"\]$/)?.[1];
      if (key !== undefined) return options?.cardByKey?.[key] ?? absent();
      return absent();
    }),
  };
  return { page: page as unknown as Page, heading, sections, receipt, buttons };
}

function card(options?: {
  id?: string | null;
  title?: string;
  badges?: string[];
  buttons?: string[];
  stale?: boolean;
  body?: string;
}) {
  const badgeSpans = new FakeLocator({ items: (options?.badges ?? []).map((text) => new FakeLocator({ text })) });
  const header = new FakeLocator({ locator: (selector) => (selector === "span" ? badgeSpans : absent()) });
  const buttonLocators = (options?.buttons ?? []).map((text) => new FakeLocator({ text }));
  return new FakeLocator({
    attributes: { id: options?.id === undefined ? "prompt-section-alpha" : options.id },
    locator: (selector) => {
      if (selector === ":scope > div") return new FakeLocator({ items: [header] });
      if (selector === ":scope > pre") return new FakeLocator({ text: options?.body ?? "body\n" });
      return absent();
    },
    role: (role, roleOptions) => {
      if (role === "heading" && roleOptions?.level === 2) return new FakeLocator({ text: options?.title ?? "Alpha" });
      if (role === "button" && roleOptions === undefined) return new FakeLocator({ items: buttonLocators });
      return absent();
    },
    textLocator: (text) =>
      String(text).includes("The default changed") ? new FakeLocator({ visible: options?.stale ?? false }) : absent(),
  });
}

describe("WebNewDriver prompt navigation and cards", () => {
  it("opens the prompts route and waits for the new-app sections surface", async () => {
    const stub = navigationPage();
    const driver = new WebNewDriver(stub.page);

    await driver.promptsOpen("acme");

    expect(stub.page.goto).toHaveBeenCalledWith("/acme/prompts");
    expect(stub.heading.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.sections.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("detects both tabs and opens only a destination that is not already visible", async () => {
    const onSections = navigationPage({ sectionsVisible: true, receiptVisible: false });
    const sectionsDriver = new WebNewDriver(onSections.page);
    await expect(sectionsDriver.promptsActiveTab()).resolves.toBe("Sections");
    await sectionsDriver.promptsOpenTab("Sections");
    expect(onSections.buttons.size).toBe(0);

    const onReceipt = navigationPage({ sectionsVisible: false, receiptVisible: true });
    await expect(new WebNewDriver(onReceipt.page).promptsActiveTab()).resolves.toBe("Receipt");

    const switching = navigationPage({ sectionsVisible: false, receiptVisible: false });
    const driver = new WebNewDriver(switching.page);
    await driver.promptsOpenTab("Receipt");
    expect(switching.buttons.get("Receipt")?.click).toHaveBeenCalledTimes(1);
    expect(switching.page.getByRole).toHaveBeenCalledWith("tab", { name: "Receipt", exact: true });
    expect(switching.receipt.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("reads cards using the new article, badge and copy structure", async () => {
    const valid = card({
      badges: ["Your override", "Admin-managed", "Coding task", "Review"],
      buttons: ["Edit workspace default", "Edit my override", "Ignore me"],
      stale: true,
      body: "effective body\n",
    });
    const invalid = card({ id: null });
    const stub = navigationPage({ cards: [valid, invalid], cardByKey: { alpha: valid } });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.promptsSectionCards()).resolves.toEqual([
      {
        key: "alpha",
        title: "Alpha",
        sourceBadge: "Your override",
        badges: ["Admin-managed", "Coding task", "Review"],
        kinds: ["Coding task", "Review"],
        staleWarning: true,
        body: "effective body",
        workspaceEditLabel: "Edit workspace default",
        personalEditLabel: "Edit my override",
      },
    ]);
    await expect(driver.promptsSectionCard("alpha")).resolves.toMatchObject({ key: "alpha" });
    await expect(driver.promptsSectionCard("missing")).resolves.toBeNull();
  });

  it("reads sparse cards without inventing badges or edit controls", async () => {
    const sparse = card({ badges: ["Pi Dash default", "Scheduler"], buttons: [], stale: false, body: "" });
    const driver = new WebNewDriver(navigationPage({ cardByKey: { alpha: sparse } }).page);

    await expect(driver.promptsSectionCard("alpha")).resolves.toMatchObject({
      sourceBadge: "Pi Dash default",
      badges: ["Scheduler"],
      kinds: ["Scheduler"],
      staleWarning: false,
      workspaceEditLabel: null,
      personalEditLabel: null,
    });
  });
});

describe("WebNewDriver prompt section navigation and shell states", () => {
  function link(title: string, key: string) {
    const result = new FakeLocator({
      attributes: { href: `#prompt-section-${key}` },
      locator: (selector) =>
        selector === "span" ? new FakeLocator({ items: [new FakeLocator({ text: title })] }) : absent(),
    });
    return result;
  }

  it("reads nav entries and jumps to a card anchor", async () => {
    const alpha = link("Alpha", "alpha");
    const beta = link("Beta", "beta");
    const alphaCard = card();
    const stub = navigationPage({
      links: [alpha, beta],
      cardByKey: { alpha: alphaCard },
      hash: "#prompt-section-alpha",
    });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.promptsSectionNav()).resolves.toEqual([
      { title: "Alpha", key: "alpha" },
      { title: "Beta", key: "beta" },
    ]);
    await expect(driver.promptsSectionNavJump("alpha")).resolves.toBe("#prompt-section-alpha");
    expect(alpha.click).toHaveBeenCalledTimes(1);
    expect(alphaCard.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("reports loading, member failure and workspace warning independently", async () => {
    const visible = new Set(["Loading…", "workspace editing is unavailable"]);
    const page = {
      getByText: (text: string) => new FakeLocator({ visible: visible.has(text) }),
    } as unknown as Page;
    const driver = new WebNewDriver(page);

    await expect(driver.promptsLoadingVisible()).resolves.toBe(true);
    await expect(driver.promptsSectionsErrorVisible()).resolves.toBe(false);
    await expect(driver.promptsWorkspaceWarningVisible()).resolves.toBe(true);
  });
});

describe("WebNewDriver prompt request controls", () => {
  it("holds only the selected section-list scope and can clear the route", async () => {
    let handler!: (route: Route) => Promise<void>;
    const route = vi.fn(async (_pattern: string, next: (route: Route) => Promise<void>) => {
      handler = next;
    });
    const unroute = vi.fn(async (_pattern: string) => undefined);
    const driver = new WebNewDriver({ route, unroute } as unknown as Page);

    await driver.promptsFailSectionsStart("workspace");
    expect(route).toHaveBeenCalledWith("**/api/workspaces/*/prompt-sections*", expect.any(Function));

    const failed = routeDouble("GET", "https://test/api/workspaces/acme/prompt-sections?scope=workspace");
    await handler(failed as unknown as Route);
    expect(failed.fulfill).toHaveBeenCalledWith({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "parity sections failure" }),
    });
    expect(failed.continue).not.toHaveBeenCalled();

    const otherScope = routeDouble("GET", "https://test/api/workspaces/acme/prompt-sections?scope=user");
    await handler(otherScope as unknown as Route);
    expect(otherScope.continue).toHaveBeenCalledTimes(1);
    const write = routeDouble("PUT", "https://test/api/workspaces/acme/prompt-sections/key?scope=workspace");
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledTimes(1);

    await driver.promptsFailSectionsStop();
    expect(unroute).toHaveBeenCalledWith("**/api/workspaces/*/prompt-sections*");
  });

  it("tolerates an already-cleared failure route", async () => {
    const driver = new WebNewDriver({
      unroute: vi.fn(async () => Promise.reject(new Error("gone"))),
    } as unknown as Page);
    await expect(driver.promptsFailSectionsStop()).resolves.toBeUndefined();
  });

  it("delays the first GET only and passes every request through", async () => {
    let handler!: (route: Route) => Promise<void>;
    const waitForTimeout = vi.fn(async (_ms: number) => undefined);
    const page = {
      route: async (_pattern: string, next: (route: Route) => Promise<void>) => {
        handler = next;
      },
      waitForTimeout,
    };
    const driver = new WebNewDriver(page as unknown as Page);

    await driver.promptsDelaySectionsOnce(75);
    const post = routeDouble("POST", "https://test/prompt-sections");
    const first = routeDouble("GET", "https://test/prompt-sections");
    const second = routeDouble("GET", "https://test/prompt-sections");
    await handler(post as unknown as Route);
    await handler(first as unknown as Route);
    await handler(second as unknown as Route);
    expect(waitForTimeout).toHaveBeenCalledOnce();
    expect(waitForTimeout).toHaveBeenCalledWith(75);
    expect(post.continue).toHaveBeenCalledOnce();
    expect(first.continue).toHaveBeenCalledOnce();
    expect(second.continue).toHaveBeenCalledOnce();
  });

  it("fails the first PUT only", async () => {
    let handler!: (route: Route) => Promise<void>;
    const page = {
      route: async (_pattern: string, next: (route: Route) => Promise<void>) => {
        handler = next;
      },
    };
    const driver = new WebNewDriver(page as unknown as Page);

    await driver.promptsFailUpsertOnce();
    const get = routeDouble("GET", "https://test/prompt-sections");
    const first = routeDouble("PUT", "https://test/prompt-sections/key");
    const second = routeDouble("PUT", "https://test/prompt-sections/key");
    await handler(get as unknown as Route);
    await handler(first as unknown as Route);
    await handler(second as unknown as Route);
    expect(get.continue).toHaveBeenCalledOnce();
    expect(first.fulfill).toHaveBeenCalledWith({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "parity upsert failure" }),
    });
    expect(second.continue).toHaveBeenCalledOnce();
  });
});

function editorFixture(options?: {
  present?: boolean;
  defaultVisible?: boolean;
  errorVisible?: boolean;
  errorText?: string;
  saveEnabled?: boolean;
  revertVisible?: boolean;
}) {
  const textarea = new FakeLocator({ input: "draft" });
  const defaultPane = new FakeLocator({ visible: options?.defaultVisible ?? true, text: "registry default\n" });
  const error = new FakeLocator({ visible: options?.errorVisible ?? true, text: options?.errorText ?? "save failed" });
  const scope = new FakeLocator({ text: "Your personal override" });
  const buttons = new Map<string, FakeLocator>();
  const role = (_role: string, roleOptions?: { name?: string | RegExp }) => {
    if (_role === "alert") return error;
    if (_role !== "button") return absent();
    const name = roleOptions?.name;
    const key = name instanceof RegExp ? name.source : String(name);
    if (!buttons.has(key)) {
      const visible = key === "Revert to default" ? (options?.revertVisible ?? true) : true;
      const enabled = key === "Save" ? (options?.saveEnabled ?? true) : true;
      buttons.set(key, new FakeLocator({ visible, enabled }));
    }
    return buttons.get(key) ?? absent();
  };
  const root = new FakeLocator({
    count: options?.present === false ? 0 : 1,
    locator: (selector) => {
      if (selector === "textarea") return textarea;
      if (selector === "pre") return defaultPane;
      return absent();
    },
    role,
    textLocator: () => scope,
  });
  const cards = new Map<string, FakeLocator>();
  const page = {
    locator: vi.fn((selector: string, locatorOptions?: unknown) => {
      if (selector === "textarea") return absent();
      if (selector === 'article[id^="prompt-section-"]' && locatorOptions !== undefined) return root;
      const key = selector.match(/^article\[id="prompt-section-(.*)"\]$/)?.[1];
      if (key !== undefined) {
        if (!cards.has(key)) cards.set(key, root);
        return cards.get(key) ?? absent();
      }
      return absent();
    }),
  };
  return { page: page as unknown as Page, root, textarea, defaultPane, error, scope, buttons };
}

describe("WebNewDriver prompt editor", () => {
  it("opens both workspace and personal editors and waits for the textarea", async () => {
    const fixture = editorFixture();
    const driver = new WebNewDriver(fixture.page);

    await driver.promptsOpenSectionEditor("alpha", "workspace");
    await driver.promptsOpenSectionEditor("alpha", "user");

    expect(fixture.buttons.get("^(Edit workspace default|Customize for workspace)$")?.click).toHaveBeenCalledOnce();
    expect(fixture.buttons.get("^(Edit my override|Customize for me)$")?.click).toHaveBeenCalledOnce();
    expect(fixture.textarea.waitFor).toHaveBeenCalledTimes(2);
  });

  it("returns null when no editor is open", async () => {
    const fixture = editorFixture({ present: false });
    await expect(new WebNewDriver(fixture.page).promptsEditorState()).resolves.toBeNull();
  });

  it("reads the complete open-editor state", async () => {
    const fixture = editorFixture();
    await expect(new WebNewDriver(fixture.page).promptsEditorState()).resolves.toEqual({
      scopeLabel: "Your personal override",
      draft: "draft",
      saveEnabled: true,
      defaultVisible: true,
      defaultBody: "registry default",
      revertVisible: true,
      error: "save failed",
    });
  });

  it("reads hidden optional state and normalizes an empty visible error", async () => {
    const hidden = editorFixture({
      defaultVisible: false,
      errorVisible: false,
      saveEnabled: false,
      revertVisible: false,
    });
    await expect(new WebNewDriver(hidden.page).promptsEditorState()).resolves.toMatchObject({
      saveEnabled: false,
      defaultVisible: false,
      defaultBody: null,
      revertVisible: false,
      error: null,
    });

    const emptyError = editorFixture({ errorText: "" });
    await expect(new WebNewDriver(emptyError.page).promptsEditorState()).resolves.toMatchObject({ error: null });
  });

  it("fills, saves, cancels and toggles compare using new-app labels", async () => {
    const fixture = editorFixture();
    const driver = new WebNewDriver(fixture.page);

    await driver.promptsEditorFill("changed");
    await driver.promptsEditorSave();
    await driver.promptsEditorToggleCompare();
    await driver.promptsEditorCancel();

    expect(fixture.textarea.fill).toHaveBeenCalledWith("changed");
    expect(fixture.buttons.get("Save")?.click).toHaveBeenCalledOnce();
    expect(fixture.buttons.get("^(Show default|Hide default)$")?.click).toHaveBeenCalledOnce();
    expect(fixture.buttons.get("Cancel")?.click).toHaveBeenCalledOnce();
    expect(fixture.textarea.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 30_000 });
  });
});

describe("WebNewDriver prompt revert dialog", () => {
  function dialogFixture(visible: boolean) {
    const heading = new FakeLocator({ visible, text: "Revert to default?" });
    const body = new FakeLocator({ text: "Your override will be discarded and this can't be undone." });
    const buttons = new Map<string, FakeLocator>();
    const dialog = new FakeLocator({
      locator: (selector) => (selector === "p" ? new FakeLocator({ items: [body] }) : absent()),
      role: (role, options) => {
        if (role !== "button") return absent();
        const key = options?.name instanceof RegExp ? options.name.source : String(options?.name);
        if (!buttons.has(key)) {
          buttons.set(key, new FakeLocator({ text: key === "^Revert" ? "Reverting…" : key }));
        }
        return buttons.get(key) ?? absent();
      },
    });
    const editor = editorFixture();
    const page = {
      locator: (editor.page as unknown as { locator: (...args: unknown[]) => unknown }).locator,
      getByRole: (role: string, _options?: { name?: string | RegExp }) => {
        if (role === "heading") return heading;
        if (role === "dialog") return new FakeLocator({ filtered: dialog });
        return absent();
      },
    };
    return { page: page as unknown as Page, editor, heading, dialog, body, buttons };
  }

  it("opens the dialog from the editor", async () => {
    const fixture = dialogFixture(true);
    const driver = new WebNewDriver(fixture.page);

    await driver.promptsEditorRevertOpen();

    expect(fixture.editor.buttons.get("Revert to default")?.click).toHaveBeenCalledOnce();
    expect(fixture.heading.waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("returns null when the dialog is absent and reads it when visible", async () => {
    const hidden = dialogFixture(false);
    await expect(new WebNewDriver(hidden.page).promptsRevertDialog()).resolves.toBeNull();

    const visible = dialogFixture(true);
    await expect(new WebNewDriver(visible.page).promptsRevertDialog()).resolves.toEqual({
      title: "Revert to default?",
      body: "Your override will be discarded and this can't be undone.",
      confirmLabel: "Reverting…",
    });
  });

  it.each(["rejected", "nullish"] as const)("normalizes %s dialog text reads", async (failure) => {
    const fixture = dialogFixture(true);
    const textRead = vi.spyOn(FakeLocator.prototype, "innerText");
    if (failure === "rejected") textRead.mockRejectedValue(new Error("detached dialog"));
    else textRead.mockResolvedValue(null as unknown as string);
    await expect(new WebNewDriver(fixture.page).promptsRevertDialog()).resolves.toEqual({
      title: "",
      body: "",
      confirmLabel: "",
    });
  });

  it("confirms and cancels through the scoped dialog", async () => {
    const fixture = dialogFixture(true);
    const driver = new WebNewDriver(fixture.page);

    await driver.promptsRevertConfirm();
    await driver.promptsRevertCancel();

    expect(fixture.buttons.get("Revert")?.click).toHaveBeenCalledOnce();
    expect(fixture.buttons.get("Cancel")?.click).toHaveBeenCalledOnce();
    expect(fixture.heading.waitFor).toHaveBeenCalledWith({ state: "detached", timeout: 30_000 });
  });
});

describe("WebNewDriver prompt read fallbacks", () => {
  it.each(["rejected", "nullish"] as const)("normalizes %s card, nav and editor reads", async (failure) => {
    const textRead = vi.spyOn(FakeLocator.prototype, "innerText");
    const inputRead = vi.spyOn(FakeLocator.prototype, "inputValue");
    const enabledRead = vi.spyOn(FakeLocator.prototype, "isEnabled");
    const attributeRead = vi.spyOn(FakeLocator.prototype, "getAttribute");
    if (failure === "rejected") {
      textRead.mockRejectedValue(new Error("detached text"));
      inputRead.mockRejectedValue(new Error("detached input"));
      enabledRead.mockRejectedValue(new Error("detached button"));
    } else {
      textRead.mockResolvedValue(null as unknown as string);
      inputRead.mockResolvedValue(null as unknown as string);
    }
    const sparse = card({ badges: ["unknown"], buttons: ["unknown"] });
    const driver = new WebNewDriver(navigationPage({ cardByKey: { alpha: sparse } }).page);
    await expect(driver.promptsSectionCard("alpha")).resolves.toMatchObject({
      title: "",
      body: "",
      sourceBadge: "",
      badges: [],
      kinds: [],
      workspaceEditLabel: null,
      personalEditLabel: null,
    });
    // A disappearing card has no usable id; unrelated ids are ignored too.
    attributeRead.mockResolvedValueOnce("unrelated");
    await expect(driver.promptsSectionCard("alpha")).resolves.toBeNull();
    attributeRead.mockRejectedValueOnce(new Error("detached card"));
    await expect(driver.promptsSectionCard("alpha")).resolves.toBeNull();

    const nav = navigationPage({ links: [new FakeLocator()] });
    if (failure === "rejected") attributeRead.mockRejectedValueOnce(new Error("detached link"));
    await expect(new WebNewDriver(nav.page).promptsSectionNav()).resolves.toEqual([{ title: "", key: "" }]);
    const editor = editorFixture();
    await expect(new WebNewDriver(editor.page).promptsEditorState()).resolves.toMatchObject({
      scopeLabel: "",
      draft: "",
      defaultBody: "",
      error: null,
      ...(failure === "rejected" ? { saveEnabled: false } : {}),
    });
  });
});

// --- Receipt composition and previews (NEWFRONT-599, AGT-028–031) ---

interface ReceiptCardOptions {
  kind: string;
  count?: string;
  expanded?: boolean;
  template?: string | null;
  automatic?: string | null;
  items?: { num: string; title: string; key: string; badge: string }[];
  previewHeading?: boolean;
  previewEnabled?: boolean;
  submitFails?: boolean;
  previewError?: string | null;
  previewResult?: string | null;
  /** Expanded, but the template block has not rendered yet. */
  hiddenTemplate?: boolean;
}

interface ReceiptCardDouble {
  kind: string;
  card: FakeLocator;
  header: FakeLocator;
}

/** A receipt card double addressable by its `id="prompt-receipt-<kind>"`. */
function receiptCard(options: ReceiptCardOptions): ReceiptCardDouble {
  const header = new FakeLocator({
    attributes: { "aria-expanded": options.expanded === true ? "true" : "false" },
    text: options.expanded === true ? "Hide" : "Show",
  });
  const items = (options.items ?? []).map(
    (item) =>
      new FakeLocator({
        locator: (selector) => {
          if (selector === "span") {
            return new FakeLocator({
              items: [
                new FakeLocator({ text: item.num }),
                new FakeLocator({ text: item.badge }),
                new FakeLocator({ text: item.badge }),
              ],
            });
          }
          if (selector === "span > span") {
            return new FakeLocator({
              items: [new FakeLocator({ text: item.title }), new FakeLocator({ text: item.key })],
            });
          }
          return new FakeLocator({ text: item.badge });
        },
      })
  );
  const textbox = new FakeLocator();
  const submit = new FakeLocator({
    enabled: options.previewEnabled ?? false,
    isEnabledRejects: options.submitFails ?? false,
  });
  const testIds = new Map<string, FakeLocator>();
  const addTestId = (id: string, text: string | null | undefined) => {
    if (text !== null && text !== undefined) testIds.set(id, new FakeLocator({ text }));
  };
  addTestId("receipt-count", options.count);
  addTestId(`receipt-template-${options.kind}`, options.template);
  addTestId(`receipt-automatic-${options.kind}`, options.automatic);
  addTestId(`receipt-preview-${options.kind}-error`, options.previewError);
  addTestId(`receipt-preview-${options.kind}-result`, options.previewResult);
  const card = new FakeLocator({
    attributes: { id: `prompt-receipt-${options.kind}` },
    testIdLocator: (testId) => {
      if (options.hiddenTemplate === true && testId === `receipt-template-${options.kind}`) {
        return new FakeLocator({ visible: false });
      }
      return testIds.get(testId) ?? absent();
    },
    locator: (selector) => {
      if (selector === "ol > li") return new FakeLocator({ items });
      if (selector === "form") {
        return new FakeLocator({ role: (role) => (role === "textbox" ? textbox : absent()) });
      }
      return absent();
    },
    role: (role, roleOptions) => {
      if (role === "button" && roleOptions?.name instanceof RegExp) return header;
      if (role === "button" && roleOptions?.name === "Preview") return submit;
      if (role === "heading" && roleOptions?.name === "Preview") {
        return new FakeLocator({ visible: options.previewHeading ?? false });
      }
      if (role === "textbox") return textbox;
      return absent();
    },
  });
  return { kind: options.kind, card, header };
}

function receiptPage(options: { cards: ReceiptCardDouble[]; links?: FakeLocator[]; hash?: string }): {
  page: Page;
  nav: FakeLocator;
} {
  const doubles = options.cards;
  // The reads themselves may reject, so key the lookup by the fixture's kind
  // rather than reading the id back off the double.
  const byId = new Map(doubles.map((double) => [double.kind, double.card]));
  const nav = new FakeLocator({
    role: (role) => (role === "link" ? new FakeLocator({ items: options.links ?? [] }) : absent()),
    locator: (selector) => {
      const kind = selector.match(/^a\[href="#prompt-receipt-(.*)"\]$/)?.[1];
      return (
        (kind !== undefined ? (options.links ?? []).find((link) => link.receiptKey() === kind) : undefined) ?? absent()
      );
    },
  });
  const page = {
    evaluate: vi.fn(async (_callback: unknown) => options.hash ?? "#prompt-receipt-review"),
    getByRole: vi.fn((role: string, roleOptions?: { name?: string | RegExp }) =>
      role === "navigation" && roleOptions?.name === "Prompt receipt" ? nav : absent()
    ),
    locator: vi.fn((selector: string) => {
      if (selector === 'article[id^="prompt-receipt-"]') {
        return new FakeLocator({ items: doubles.map((double) => double.card) });
      }
      const kind = selector.match(/^article\[id="prompt-receipt-(.*)"\]$/)?.[1];
      return (kind !== undefined ? byId.get(kind) : undefined) ?? absent();
    }),
  };
  return { page: page as unknown as Page, nav };
}

function receiptLink(kind: string, count: string): FakeLocator {
  return new FakeLocator({ attributes: { href: `#prompt-receipt-${kind}` }, text: `${kind}\n${count}\n` });
}

describe("WebNewDriver receipt composition", () => {
  it("reads each card's kind, rendered count and numbered composition", async () => {
    const coding = receiptCard({
      kind: "coding-task",
      count: "2 sections",
      items: [
        { num: "01", title: "Intro", key: "intro", badge: "Pi Dash default" },
        { num: "02", title: "Autonomy", key: "autonomy", badge: "Your override" },
      ],
    });
    const unrelated = {
      kind: "",
      card: new FakeLocator({ attributes: { id: "unrelated" } }),
      header: new FakeLocator(),
    };
    const { page } = receiptPage({ cards: [coding, unrelated] });

    await expect(new WebNewDriver(page).promptsReceiptCards()).resolves.toEqual([
      {
        kind: "coding-task",
        countBadge: "2 sections",
        sections: [
          { num: "01", title: "Intro", key: "intro", sourceBadge: "Pi Dash default" },
          { num: "02", title: "Autonomy", key: "autonomy", sourceBadge: "Your override" },
        ],
      },
    ]);
  });

  it("trims copy-pasted counts and drops unrelated roots", async () => {
    const review = receiptCard({ kind: "review", count: " 1 section\n" });
    const { page } = receiptPage({ cards: [review] });
    const cards = await new WebNewDriver(page).promptsReceiptCards();
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({ kind: "review", countBadge: "1 section", sections: [] });
  });

  it("reads the receipt nav and follows an anchor", async () => {
    const link = receiptLink("review", "1 section");
    const review = receiptCard({ kind: "review", count: "1 section" });
    const { page } = receiptPage({ cards: [review], links: [link], hash: "#prompt-receipt-review" });
    const driver = new WebNewDriver(page);

    await expect(driver.promptsReceiptNav()).resolves.toEqual([{ kind: "review", count: "1 section" }]);
    await expect(driver.promptsReceiptNavJump("review")).resolves.toBe("#prompt-receipt-review");
    expect(link.click).toHaveBeenCalledTimes(1);
    expect(review.card.waitFor).toHaveBeenCalledWith({ state: "visible", timeout: 30_000 });
  });

  it("toggles expansion and reads only a shown template", async () => {
    const open = receiptCard({ kind: "review", expanded: true, template: "assembled\n\n" });
    const openDriver = new WebNewDriver(receiptPage({ cards: [open] }).page);
    await expect(openDriver.promptsReceiptExpanded("review")).resolves.toBe(true);
    await expect(openDriver.promptsReceiptTemplate("review")).resolves.toBe("assembled");
    await openDriver.promptsReceiptToggle("review");
    expect(open.header.click).toHaveBeenCalledTimes(1);

    const collapsed = receiptCard({ kind: "review", expanded: false, template: "assembled" });
    const collapsedDriver = new WebNewDriver(receiptPage({ cards: [collapsed] }).page);
    await expect(collapsedDriver.promptsReceiptExpanded("review")).resolves.toBe(false);
    await expect(collapsedDriver.promptsReceiptTemplate("review")).resolves.toBeNull();
    await expect(collapsedDriver.promptsReceiptAutomatic("review")).resolves.toBeNull();
  });

  it("reads the automatic-run variant only when present", async () => {
    const withVariant = receiptCard({ kind: "review", expanded: true, template: "personal", automatic: "auto\n" });
    const withoutVariant = receiptCard({ kind: "review", expanded: true, template: "personal" });
    await expect(
      new WebNewDriver(receiptPage({ cards: [withVariant] }).page).promptsReceiptAutomatic("review")
    ).resolves.toBe("auto");
    await expect(
      new WebNewDriver(receiptPage({ cards: [withoutVariant] }).page).promptsReceiptAutomatic("review")
    ).resolves.toBeNull();
  });

  it("reports an expanded card whose template has not rendered yet as null", async () => {
    const pending = receiptCard({ kind: "review", expanded: true, template: "assembled", hiddenTemplate: true });
    await expect(
      new WebNewDriver(receiptPage({ cards: [pending] }).page).promptsReceiptTemplate("review")
    ).resolves.toBeNull();
  });
});

describe("WebNewDriver saved preview", () => {
  it("reports a detached submit as disarmed", async () => {
    const admin = receiptCard({
      kind: "coding-task",
      expanded: true,
      template: "t",
      previewHeading: true,
      previewEnabled: true,
      submitFails: true,
    });
    await expect(
      new WebNewDriver(receiptPage({ cards: [admin] }).page).promptsSavedPreviewSubmitEnabled("coding-task")
    ).resolves.toBe(false);
  });

  it("reports the admin form's presence and disabled empty submit", async () => {
    const admin = receiptCard({ kind: "coding-task", expanded: true, template: "t", previewHeading: true });
    const member = receiptCard({ kind: "coding-task", expanded: true, template: "t" });
    await expect(
      new WebNewDriver(receiptPage({ cards: [admin] }).page).promptsSavedPreviewVisible("coding-task")
    ).resolves.toBe(true);
    await expect(
      new WebNewDriver(receiptPage({ cards: [admin] }).page).promptsSavedPreviewSubmitEnabled("coding-task")
    ).resolves.toBe(false);
    await expect(
      new WebNewDriver(receiptPage({ cards: [member] }).page).promptsSavedPreviewVisible("coding-task")
    ).resolves.toBe(false);
  });

  it("fills the scheduler target, submits, and reads the rendered prompt", async () => {
    const card = receiptCard({ kind: "scheduler", expanded: true, previewResult: "rendered prompt\n" });
    const driver = new WebNewDriver(receiptPage({ cards: [card] }).page);
    await driver.promptsSavedPreviewSubmit("scheduler", "binding-1");
    await expect(driver.promptsSavedPreviewResult("scheduler")).resolves.toEqual({
      prompt: "rendered prompt",
      error: null,
    });
  });

  it("reads the inline error and reports no prompt", async () => {
    const card = receiptCard({ kind: "review", expanded: true, previewError: "Target not found.\n" });
    const driver = new WebNewDriver(receiptPage({ cards: [card] }).page);
    await expect(driver.promptsSavedPreviewResult("review")).resolves.toEqual({
      prompt: null,
      error: "Target not found.",
    });
  });
});

describe("WebNewDriver draft preview", () => {
  function editorPage(options: {
    switches?: string[];
    submitEnabled?: boolean;
    prompt?: string | null;
    error?: string | null;
  }) {
    const textbox = new FakeLocator();
    const submit = new FakeLocator({ enabled: options.submitEnabled ?? false });
    const testIds = new Map<string, FakeLocator>();
    for (const kind of options.switches ?? []) {
      testIds.set(`draft-preview-kind-${kind}`, new FakeLocator());
    }
    if (options.prompt !== null && options.prompt !== undefined) {
      testIds.set("draft-preview-result", new FakeLocator({ text: options.prompt }));
    }
    if (options.error !== null && options.error !== undefined) {
      testIds.set("draft-preview-error", new FakeLocator({ text: options.error }));
    }
    const textarea = new FakeLocator();
    const root = new FakeLocator({
      locator: (selector) => {
        if (selector === "textarea") return textarea;
        if (selector === "form") {
          return new FakeLocator({ role: (role) => (role === "textbox" ? textbox : absent()) });
        }
        return absent();
      },
      testIdLocator: (testId) => testIds.get(testId) ?? absent(),
      role: (role, roleOptions) => {
        if (role === "textbox") return textbox;
        if (role === "button" && roleOptions?.name === "Preview draft") return submit;
        return absent();
      },
    });
    const page = {
      locator: vi.fn((selector: string) =>
        selector === 'article[id^="prompt-section-"]' && options.switches !== undefined
          ? new FakeLocator({ items: [root] })
          : absent()
      ),
    };
    return { page: page as unknown as Page, root, textbox, submit };
  }

  it("reports no switcher for a single-kind section", async () => {
    const fixture = editorPage({ switches: [] });
    await expect(new WebNewDriver(fixture.page).promptsDraftPreviewKinds()).resolves.toEqual([]);
  });

  it("reads and selects the multi-kind switcher labels", async () => {
    const fixture = editorPage({ switches: ["coding-task", "review"] });
    const driver = new WebNewDriver(fixture.page);
    await expect(driver.promptsDraftPreviewKinds()).resolves.toEqual(["coding-task", "review"]);
    await driver.promptsDraftPreviewSelectKind("review");
    expect(fixture.root.getByTestId("draft-preview-kind-review").click).toHaveBeenCalledTimes(1);
  });

  it("reports the submit's enabled state and posts the target", async () => {
    const fixture = editorPage({ switches: [], submitEnabled: true });
    const driver = new WebNewDriver(fixture.page);
    await expect(driver.promptsDraftPreviewSubmitEnabled()).resolves.toBe(true);
    await driver.promptsDraftPreviewSubmit("issue-1");
    expect(fixture.textbox.fill).toHaveBeenCalledWith("issue-1");
    expect(fixture.submit.click).toHaveBeenCalledTimes(1);
  });

  it("reads the rendered draft prompt", async () => {
    const fixture = editorPage({ switches: [], prompt: "draft rendered\n" });
    await expect(new WebNewDriver(fixture.page).promptsDraftPreviewResult()).resolves.toEqual({
      prompt: "draft rendered",
      error: null,
    });
  });

  it("reads an inline draft failure with no prompt", async () => {
    const fixture = editorPage({ switches: [], error: "Draft render failed.\n" });
    await expect(new WebNewDriver(fixture.page).promptsDraftPreviewResult()).resolves.toEqual({
      prompt: null,
      error: "Draft render failed.",
    });
  });
});

describe("WebNewDriver receipt and preview read fallbacks", () => {
  it("normalizes rejected and nullish receipt reads", async () => {
    const attributes = { id: "prompt-receipt-review" };
    const failingCard = new FakeLocator({
      attributes,
      counted: 1,
      failing: true,
      testIdLocator: (testId) =>
        testId === "receipt-count" ? new FakeLocator({ counted: 1, failing: true }) : new FakeLocator({ count: 0 }),
      locator: (selector) =>
        selector === "ol > li"
          ? new FakeLocator({
              items: [
                new FakeLocator({
                  failing: true,
                  locator: (inner) =>
                    inner === "span"
                      ? new FakeLocator({ failing: true })
                      : new FakeLocator({ items: [], failing: true }),
                }),
              ],
            })
          : absent(),
      role: () => new FakeLocator({ failing: true }),
    });
    // The kind check reads the id once; only later reads are detached.
    vi.spyOn(failingCard, "getAttribute")
      .mockResolvedValueOnce("prompt-receipt-review")
      .mockRejectedValue(new Error("detached"));
    const doubles = { kind: "review", header: new FakeLocator({ failing: true }), card: failingCard };
    const { page } = receiptPage({ cards: [doubles] });
    const driver = new WebNewDriver(page);
    await expect(driver.promptsReceiptCards()).resolves.toEqual([
      {
        kind: "review",
        countBadge: "",
        sections: [{ num: "", title: "", key: "", sourceBadge: "" }],
      },
    ]);
    await expect(driver.promptsReceiptExpanded("review")).resolves.toBe(false);
    await expect(driver.promptsReceiptTemplate("review")).resolves.toBeNull();
    await expect(driver.promptsReceiptAutomatic("review")).resolves.toBeNull();
    await expect(driver.promptsSavedPreviewResult("review")).resolves.toEqual({ prompt: null, error: null });
  });

  it("normalizes rejected nav reads and empty counts", async () => {
    // The kind comes from the href; a failing text read empties the count.
    const link = new FakeLocator({ attributes: { href: "#prompt-receipt-review" }, counted: 1, failing: true });
    vi.spyOn(link, "getAttribute").mockResolvedValue("#prompt-receipt-review");
    const { page: textFailing } = receiptPage({ cards: [], links: [link] });
    await expect(new WebNewDriver(textFailing).promptsReceiptNav()).resolves.toEqual([{ kind: "review", count: "" }]);
    // A null href read must degrade to an empty kind, not throw.
    const nullHref = new FakeLocator({ attributes: { href: "#prompt-receipt-review" }, counted: 1 });
    vi.spyOn(nullHref, "getAttribute").mockResolvedValue(null);
    const { page } = receiptPage({ cards: [], links: [nullHref] });
    await expect(new WebNewDriver(page).promptsReceiptNav()).resolves.toEqual([{ kind: "", count: "" }]);
  });

  it("normalizes an href read that rejects", async () => {
    const link = new FakeLocator({ attributes: { href: "#prompt-receipt-review" }, counted: 1 });
    vi.spyOn(link, "getAttribute").mockRejectedValue(new Error("detached"));
    const { page } = receiptPage({ cards: [], links: [link] });
    await expect(new WebNewDriver(page).promptsReceiptNav()).resolves.toEqual([{ kind: "", count: "" }]);
  });

  it("reads a blank inline error and result as absent", async () => {
    const card = receiptCard({
      kind: "review",
      expanded: true,
      previewError: "   \n",
      previewResult: "  \n",
    });
    await expect(
      new WebNewDriver(receiptPage({ cards: [card] }).page).promptsSavedPreviewResult("review")
    ).resolves.toEqual({ prompt: null, error: null });
  });

  it("normalizes rejected draft-preview reads", async () => {
    const root = new FakeLocator({
      locator: (selector) => (selector === "textarea" ? new FakeLocator() : absent()),
      testIdLocator: (testId) =>
        testId === "draft-preview-kind-coding-task" ? new FakeLocator() : new FakeLocator({ count: 0, visible: false }),
      role: (role) =>
        role === "button" ? new FakeLocator({ enabled: undefined, isEnabledRejects: true }) : new FakeLocator(),
    });
    const page = {
      locator: vi.fn((selector: string) =>
        selector === 'article[id^="prompt-section-"]' ? new FakeLocator({ items: [root] }) : absent()
      ),
    };
    const driver = new WebNewDriver(page as unknown as Page);
    await expect(driver.promptsDraftPreviewKinds()).resolves.toEqual(["coding-task"]);
    await expect(driver.promptsDraftPreviewSubmitEnabled()).resolves.toBe(false);
    await expect(driver.promptsDraftPreviewResult()).resolves.toEqual({ prompt: null, error: null });
  });
});
