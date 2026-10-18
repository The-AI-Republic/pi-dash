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
  filtered?: FakeLocator;
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
    return Promise.resolve(this.options.items?.length ?? this.options.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    return Promise.resolve(this.options.visible ?? (this.options.count ?? 1) > 0);
  }

  innerText(): Promise<string> {
    return Promise.resolve(this.options.text ?? "");
  }

  getAttribute(name: string): Promise<string | null> {
    return Promise.resolve(this.options.attributes?.[name] ?? null);
  }

  hrefKey(): string | null {
    return this.options.attributes?.href?.replace(/^#prompt-section-/, "") ?? null;
  }

  inputValue(): Promise<string> {
    return Promise.resolve(this.value);
  }

  isEnabled(): Promise<boolean> {
    return Promise.resolve(this.options.enabled ?? true);
  }

  first(): FakeLocator {
    return this.options.items?.[0] ?? this;
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
      if (role === "button") {
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
