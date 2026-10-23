// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver webhook methods (NEWFRONT-541,
// WSET-019-024/041). A small Page/Locator double stands in for Playwright:
// each test proves the address a method visits, the selector it reads, the
// branch it takes and — for the network controls — which requests it holds
// or lets through. Every method of the webhook block and every fallback in
// it is exercised here.
import type { Download, Locator, Page, Response, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";
import type { ParityBrowserCookie } from "./parity-driver";

type RoleQuery = { name?: string | RegExp; level?: number };

/** The text a `getByText` locator was built from, for dialog step markers. */
const stepMarkers = new WeakMap<object, string>();

interface FakeOptions {
  count?: number;
  checked?: boolean;
  text?: string | null;
  attribute?: string | null;
  /** Inner-text reads reject, as a detached element's would. */
  rejects?: boolean;
  items?: Locator[];
  click?: () => void | Promise<void>;
  fill?: (value: string) => void | Promise<void>;
  waitFor?: (options: unknown) => void | Promise<void>;
  filterBy?: (text: string) => Locator;
  lastBy?: () => Locator;
  by: (query: { kind: string; value: string }) => Locator;
}

function fake(options: Partial<FakeOptions> & { by?: FakeOptions["by"] } = {}): Locator {
  const self: Record<string, unknown> = {
    count: async () => options.items?.length ?? options.count ?? 1,
    isChecked: async () => options.checked ?? true,
    click: async (clickOptions?: unknown) => {
      await options.click?.();
      void clickOptions;
    },
    fill: async (value: string) => {
      await options.fill?.(value);
    },
    inputValue: async () => options.text ?? "",
    getAttribute: async () => options.attribute ?? null,
    innerText: async () => {
      if (options.rejects === true) throw new Error("detached");
      return options.text === undefined ? "" : options.text;
    },
    waitFor: async (waitOptions: unknown) => {
      await options.waitFor?.(waitOptions);
    },
    first: () => self,
    last: () => self,
    nth: (index: number) => options.items?.[index] ?? self,
  };
  const by = options.by ?? (() => gone());
  self.locator = (value: string) => by({ kind: "locator", value });
  self.getByRole = (value: string, query?: RoleQuery) =>
    by({ kind: `role:${value}:${String(query?.name)}:${String(query?.level)}`, value });
  self.getByText = (value: string) => {
    const match = by({ kind: "text", value });
    stepMarkers.set(match, value);
    return match;
  };
  self.getByLabel = (value: string) => by({ kind: "label", value });
  self.getByTestId = (value: string) => by({ kind: "testid", value });
  self.filter = (query: { hasText: Locator }) =>
    options.filterBy?.(markerText(query.hasText)) ??
    by({ kind: `filter:${markerText(query.hasText)}`, value: markerText(query.hasText) });
  self.last = () => options.lastBy?.() ?? self;
  self.waitFor = async (waitOptions: unknown) => {
    await options.waitFor?.(waitOptions);
  };
  self.nth = (index: number) => options.items?.[index] ?? self;
  return self as unknown as Locator;
}

function gone(): Locator {
  return fake({ count: 0 });
}

/** A locator present exactly when `present` is true. */
function maybe(present: boolean, options: Partial<FakeOptions> = {}): Locator {
  return present ? fake(options) : gone();
}

function response(method: string, url: string): Response {
  return { url: () => url, request: () => ({ method: () => method }) } as unknown as Response;
}

function downloadDouble(path: string | null, name = "webhook-secret-key-1.csv"): Download {
  return {
    path: async () => path,
    suggestedFilename: () => name,
  } as unknown as Download;
}

interface PageOptions {
  byRole?: (role: string, query?: RoleQuery) => Locator;
  byText?: (text: string) => Locator;
  byTestId?: (id: string) => Locator;
  byLabel?: (label: string) => Locator;
  bySelector?: (selector: string, query?: { has?: Locator }) => Locator;
  keyboard?: Page["keyboard"];
  context?: Partial<Page["context"]>;
  url?: string;
  download?: Download | "reject";
  waitForTimeout?: Page["waitForTimeout"];
}

interface Harness {
  page: Page;
  visited: string[];
  pressed: string[];
  routes: string[];
  handlers: Array<(route: Route) => Promise<void>>;
  clicks: string[];
  fills: Array<{ selector: string; value: string }>;
  granted: string[][];
  addedCookies: ParityBrowserCookie[][];
  predicates: Array<(candidate: Response) => boolean>;
  events: string[];
}

function pageOf(options: PageOptions = {}): Harness {
  const state: Harness = {
    page: null as unknown as Page,
    visited: [],
    pressed: [],
    routes: [],
    handlers: [],
    clicks: [],
    fills: [],
    granted: [],
    addedCookies: [],
    predicates: [],
    events: [],
  };
  state.page = {
    goto: async (url: string) => {
      state.visited.push(url);
      return null;
    },
    waitForLoadState: async () => undefined,
    getByRole: (role: string, query?: RoleQuery) => options.byRole?.(role, query) ?? gone(),
    getByText: (text: string) => {
      const match = options.byText?.(text) ?? gone();
      stepMarkers.set(match as unknown as object, text);
      return match;
    },
    getByTestId: (id: string) => options.byTestId?.(id) ?? gone(),
    getByLabel: (label: string) => options.byLabel?.(label) ?? gone(),
    waitForTimeout: options.waitForTimeout ?? (async () => undefined),
    locator: (selector: string, query?: { has?: Locator }) =>
      options.bySelector?.(selector, query) ??
      fake({
        click: async () => {
          state.clicks.push(selector);
        },
        fill: async (value: string) => {
          state.fills.push({ selector, value });
        },
      }),
    keyboard:
      options.keyboard ??
      ({
        press: async (key: string) => {
          state.pressed.push(key);
        },
      } as unknown as Page["keyboard"]),
    route: async (pattern: string, handler: (route: Route) => Promise<void>) => {
      state.routes.push(pattern);
      state.handlers.push(handler);
    },
    waitForResponse: async (predicate: (candidate: Response) => boolean) => {
      state.predicates.push(predicate);
      return null as unknown as Response;
    },
    waitForEvent: async (event: string) => {
      state.events.push(event);
      if (options.download === "reject") throw new Error("no download");
      return options.download as Download;
    },
    context: () => ({
      grantPermissions: async (permissions: string[]) => {
        state.granted.push(permissions);
      },
      addCookies: async (cookies: ParityBrowserCookie[]) => {
        state.addedCookies.push(cookies);
      },
      ...options.context,
    }),
    url: () => options.url ?? "https://p.test/acme/settings/webhooks",
    evaluate: async (fn: (arg?: unknown) => unknown) => fn(),
  } as unknown as Page;
  return state;
}

/** A dialog node whose paragraphs and controls answer by step. */
function dialogWith(
  controls: {
    paragraphs?: () => Locator;
    buttons?: (name: string) => Locator;
    extra?: (selector: string) => Locator;
    waitFor?: (options: unknown) => void | Promise<void>;
    filterBy?: (text: string) => Locator;
    lastBy?: () => Locator;
  } = {}
): Locator {
  return fake({
    waitFor: controls.waitFor,
    filterBy: controls.filterBy,
    lastBy: controls.lastBy,
    by: (query) => {
      if (query.kind === "locator" && query.value === "p") return controls.paragraphs?.() ?? fake({ text: "" });
      if (query.kind.startsWith("role:button")) return controls.buttons?.(query.value) ?? gone();
      if (query.kind === "locator") return controls.extra?.(query.value) ?? gone();
      return gone();
    },
  });
}

/** The step marker a `has` locator was built from, if any. */
function markerText(has: Locator | undefined): string {
  return has === undefined ? "" : (stepMarkers.get(has as unknown as object) ?? "");
}

describe("web_new webhook driver — list", () => {
  it("opens the list at its own address", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsOpenWebhooks("acme");
    expect(harness.visited).toEqual(["/acme/settings/webhooks"]);
  });

  it("reads the heading at level 2, the empty state and the denial marker", async () => {
    const harness = pageOf({
      byRole: (role, query) =>
        fake({ count: role === "heading" && query?.name === "Webhooks" && query.level === 2 ? 1 : 0 }),
      byText: (text) => fake({ count: text === "No Webhook added yet" ? 1 : 0 }),
      byTestId: (id) => fake({ count: id === "workspace-settings-not-authorized" ? 1 : 0 }),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhooksHeadingVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhooksEmptyVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhooksNotAuthorized()).resolves.toBe(true);
  });

  it("reads an absent heading, empty state and denial as false", async () => {
    const driver = new WebNewDriver(pageOf().page);
    await expect(driver.wsSettingsWebhooksHeadingVisible()).resolves.toBe(false);
    await expect(driver.wsSettingsWebhooksEmptyVisible()).resolves.toBe(false);
    await expect(driver.wsSettingsWebhooksNotAuthorized()).resolves.toBe(false);
  });

  it("finds the Add control and reads the loader by its name", async () => {
    const harness = pageOf({
      byRole: (role, query) => fake({ count: role === "status" && query?.name === "Loading webhooks" ? 1 : 0 }),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhooksLoaderVisible()).resolves.toBe(true);
    const add = pageOf({
      byRole: (role, query) => fake({ count: role === "button" && query?.name === "Add webhook" ? 1 : 0 }),
    });
    await expect(new WebNewDriver(add.page).wsSettingsWebhooksAddVisible()).resolves.toBe(true);
  });

  it("gathers only detail links that carry text", async () => {
    const rows = [
      fake({ attribute: "/acme/settings/webhooks/1", text: "https://example.com/a" }),
      fake({ attribute: "/acme/settings/webhooks", text: "Webhooks" }),
      fake({ attribute: "/acme/settings/webhooks/2", text: "  " }),
      fake({ rejects: true, text: "https://example.com/c" }),
      fake({ attribute: "/acme/settings/webhooks/4", text: "" }),
      fake({ attribute: "/acme/settings/webhooks/5", text: null }),
    ];
    const harness = pageOf({
      bySelector: (selector) => (selector === 'a[href*="/settings/webhooks/"]' ? fake({ items: rows }) : gone()),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhooksRowUrls()).resolves.toEqual([
      "https://example.com/a",
    ]);
  });

  it("holds only GETs on the list route", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsWebhooksDelayListFetch(1);
    expect(harness.routes).toEqual(["**/api/workspaces/*/webhooks/"]);
    const handler = harness.handlers[0];
    if (handler === undefined) throw new Error("list route was not registered");
    const write = { request: () => ({ method: () => "POST" }), continue: vi.fn(async () => undefined) };
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledOnce();
    const read = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledOnce();
  });

  it("tolerates a held request that was cancelled before it resumed", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsWebhooksDelayListFetch(0);
    const handler = harness.handlers[0];
    if (handler === undefined) throw new Error("list route was not registered");
    const read = {
      request: () => ({ method: () => "GET" }),
      continue: async () => Promise.reject(new Error("cancelled")),
    };
    await expect(handler(read as unknown as Route)).resolves.toBeUndefined();
  });
});

describe("web_new webhook driver — rows and creation entry", () => {
  /**
   * The list markup: a row `li` holding the row link and, beside it, the
   * active switch. `selector` is `li`; `filter` selects the row by the link
   * it holds, and the switch is read from the row itself.
   */
  function rowLinks(checked: boolean, switchPresent = true): (selector: string) => Locator {
    return (selector) => {
      if (selector === 'a[href*="/settings/webhooks/"]') return fake();
      if (selector !== "li") return gone();
      const row = fake({
        click: async () => undefined,
        by: (query) => (query.kind.startsWith("role:switch") ? (switchPresent ? fake({ checked }) : gone()) : gone()),
      });
      row.filter = () => row;
      return row;
    };
  }

  it("opens a row, reads and flips its switch, and watches the patch", async () => {
    const harness = pageOf({
      byText: (text) => fake({ count: text === "Webhook details" ? 1 : 0 }),
      bySelector: rowLinks(true),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhooksRowActiveOn("https://example.com/a")).resolves.toBe(true);
    await driver.wsSettingsWebhooksOpenRow("https://example.com/a");
    await driver.wsSettingsWebhooksRowToggleActive("https://example.com/a");
    const predicate = harness.predicates[0];
    expect(predicate?.(response("PATCH", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(true);
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(false);
    expect(predicate?.(response("PATCH", "https://api.test/api/workspaces/acme/webhooks/1"))).toBe(false);
  });

  it("reports an absent switch as off", async () => {
    const harness = pageOf({ bySelector: rowLinks(true, false) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhooksRowActiveOn("https://example.com/a")).resolves.toBe(
      false
    );
  });

  it("opens the creation dialog from the heading and from the empty state", async () => {
    const harness = pageOf({
      byRole: (role, query) => fake({ count: role === "button" && query?.name === "Add webhook" ? 1 : 0 }),
      byText: (text) => fake({ count: text === "Create webhook" ? 1 : 0 }),
    });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhooksAddClick();
    await driver.wsSettingsWebhooksEmptyAddClick();
  });
});

describe("web_new webhook driver — create dialog", () => {
  interface CreateOptions {
    dialogOpen?: boolean;
    titleOpen?: boolean;
    revealOpen?: boolean;
    urlError?: boolean;
    url?: string;
    onFill?: (value: string) => void;
    onWaitFor?: (options: unknown) => void;
    withParagraph?: boolean;
    paragraphText?: string | null;
  }

  function createHarness(options: CreateOptions = {}): Harness {
    const dialogOpen = options.dialogOpen ?? true;
    return pageOf({
      byText: (text) => {
        if (text === "URL is required") return maybe(options.urlError === true);
        if (text === "Key created") {
          return maybe(options.revealOpen ?? true, {
            waitFor: (waits) => options.onWaitFor?.(waits),
            filterBy: () => fake({ waitFor: options.onWaitFor }),
          });
        }
        return gone();
      },
      bySelector: (selector, query) => {
        if (selector.includes('role="dialog"')) {
          const open = (step: string): boolean => {
            if (!dialogOpen) return false;
            if (step === "Create webhook") return options.titleOpen ?? true;
            if (step === "Key created") return options.revealOpen ?? true;
            return false;
          };
          const step = markerText(query?.has);
          if (!dialogOpen) return gone();
          if (step !== "" && !open(step)) return gone();
          return dialogWith({
            paragraphs: () =>
              options.withParagraph === false
                ? gone()
                : fake({ text: options.paragraphText === undefined ? "Copy this key now." : options.paragraphText }),
            buttons: () => fake({ waitFor: options.onWaitFor }),
            waitFor: options.onWaitFor,
            filterBy: (text) => (open(text) ? fake({}) : fake({ count: 0 })),
          });
        }
        if (selector === "form") {
          return fake({
            by: (query) => {
              if (query.kind === "label") {
                return fake({ text: options.url ?? "", fill: options.onFill });
              }
              if (query.kind === "text") return maybe(query.value === "URL is required" && options.urlError === true);
              return gone();
            },
          });
        }
        return gone();
      },
      byLabel: (label) => (label === "Target URL" ? fake({ text: options.url ?? "", fill: options.onFill }) : gone()),
    });
  }

  it("reads whether a dialog is open and the create title", async () => {
    await expect(new WebNewDriver(createHarness().page).wsSettingsWebhookDialogOpen()).resolves.toBe(true);
    await expect(new WebNewDriver(createHarness().page).wsSettingsWebhookDialogTitle()).resolves.toBe("Create webhook");
    await expect(
      new WebNewDriver(createHarness({ dialogOpen: false }).page).wsSettingsWebhookDialogOpen()
    ).resolves.toBe(false);
    await expect(
      new WebNewDriver(createHarness({ titleOpen: false }).page).wsSettingsWebhookDialogTitle()
    ).resolves.toBeNull();
  });

  it("fills and reads the target address", async () => {
    let value = "";
    const harness = createHarness({
      url: "https://example.com/typed",
      onFill: (next) => {
        value = next;
      },
    });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookUrlFill("https://example.com/a");
    expect(value).toBe("https://example.com/a");
    await expect(driver.wsSettingsWebhookUrlValue()).resolves.toBe("https://example.com/typed");
  });

  it("reads the inline URL error and its absence", async () => {
    await expect(new WebNewDriver(createHarness({ urlError: true }).page).wsSettingsWebhookUrlError()).resolves.toBe(
      "URL is required"
    );
    await expect(new WebNewDriver(createHarness().page).wsSettingsWebhookUrlError()).resolves.toBeNull();
  });

  it("derives the event mode from the two radios", async () => {
    const cases = [
      { all: true, individual: false, expected: "all" },
      { all: true, individual: true, expected: "individual" },
      { all: false, individual: false, expected: null },
    ] as const;
    for (const entry of cases) {
      const harness = pageOf({
        bySelector: (selector) =>
          selector === "form"
            ? fake({
                by: (query) =>
                  query.value === "input#all"
                    ? fake({ checked: entry.all })
                    : query.value === "input#individual"
                      ? fake({ checked: entry.individual })
                      : gone(),
              })
            : gone(),
      });
      await expect(new WebNewDriver(harness.page).wsSettingsWebhookEventMode()).resolves.toBe(entry.expected);
    }
    const missingOne = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({ by: (query) => (query.value === "input#all" ? fake({ checked: true }) : gone()) })
          : gone(),
    });
    await expect(new WebNewDriver(missingOne.page).wsSettingsWebhookEventMode()).resolves.toBeNull();
  });

  it("picks both event modes and toggles an option", async () => {
    const harness = createHarness();
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookPickEventMode("all");
    await driver.wsSettingsWebhookPickEventMode("individual");
    await driver.wsSettingsWebhookToggleIndividual("Cycles");
  });

  it("reads the individual options, falling back to the key for a missing or unreadable label", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({
              by: (query) => {
                if (query.value === "input#module") return gone();
                if (query.value.startsWith("input#")) return fake({ checked: true });
                if (query.value === 'label[for="cycle"]') return fake({ text: "   " });
                if (query.value === 'label[for="issue"]') return fake({ rejects: true });
                if (query.value.startsWith("label[for=")) return fake({ text: "  Projects \n" });
                return gone();
              },
            })
          : gone(),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookIndividualOptions()).resolves.toEqual([
      { label: "Projects", checked: true },
      { label: "cycle", checked: true },
      { label: "issue", checked: true },
      { label: "Projects", checked: true },
    ]);
  });

  it("falls back to the key when a label reads no text", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({
              by: (query) => {
                if (query.value === "input#project") return fake({ checked: true });
                if (query.value === 'label[for="project"]') return fake({ text: null });
                return gone();
              },
            })
          : gone(),
    });
    const options = await new WebNewDriver(harness.page).wsSettingsWebhookIndividualOptions();
    expect(options[0]).toEqual({ label: "project", checked: true });
  });

  it("falls back to the key when a label is not rendered", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({
              by: (query) => {
                if (query.value === "input#project") return fake({ checked: true });
                if (query.value === 'label[for="project"]') {
                  return {
                    count: async () => 0,
                    innerText: async () => "Projects",
                    first: () => fake({}),
                  } as unknown as Locator;
                }
                return gone();
              },
            })
          : gone(),
    });
    const options = await new WebNewDriver(harness.page).wsSettingsWebhookIndividualOptions();
    expect(options[0]).toEqual({ label: "project", checked: true });
  });

  it("submits and settles on the create answer", async () => {
    const harness = createHarness();
    await new WebNewDriver(harness.page).wsSettingsWebhookSubmit();
    const predicate = harness.predicates[0];
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/"))).toBe(true);
    expect(predicate?.(response("PUT", "https://api.test/api/workspaces/acme/webhooks/"))).toBe(false);
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/x/"))).toBe(false);
  });

  it("cancels and sends Escape", async () => {
    const harness = createHarness();
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookCancel();
    await driver.wsSettingsWebhookPressEscape();
    expect(harness.pressed).toEqual(["Escape"]);
  });

  it("reads the reveal marker, its explainer, and its absence", async () => {
    const driver = new WebNewDriver(createHarness().page);
    await expect(driver.wsSettingsWebhookRevealVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookRevealText()).resolves.toBe("Copy this key now.");
    const none = new WebNewDriver(createHarness({ revealOpen: false, dialogOpen: false }).page);
    await expect(none.wsSettingsWebhookRevealVisible()).resolves.toBe(false);
    await expect(none.wsSettingsWebhookRevealText()).resolves.toBeNull();
    const bare = new WebNewDriver(createHarness({ withParagraph: false }).page);
    await expect(bare.wsSettingsWebhookRevealText()).resolves.toBeNull();
    const nullText = new WebNewDriver(createHarness({ paragraphText: null }).page);
    await expect(nullText.wsSettingsWebhookRevealText()).resolves.toBeNull();
  });

  it("closes the reveal and waits for it to detach", async () => {
    const waits: unknown[] = [];
    const harness = createHarness({ onWaitFor: (options) => waits.push(options) });
    await new WebNewDriver(harness.page).wsSettingsWebhookRevealClose();
    expect(waits).toEqual([{ state: "detached", timeout: 30_000 }]);
  });
});

describe("web_new webhook driver — downloads", () => {
  it("captures the create backup with its file name and contents", async () => {
    const harness = pageOf({ download: downloadDouble("/tmp/backup.csv") });
    const driver = new WebNewDriver(harness.page);
    const reads: string[] = [];
    (driver as unknown as { readDownload: (path: string) => string }).readDownload = (path) => {
      reads.push(path);
      return "id,url\n1,https://example.com/a";
    };
    await expect(driver.wsSettingsWebhookSubmitAndCaptureDownload()).resolves.toEqual({
      fileName: "webhook-secret-key-1.csv",
      content: "id,url\n1,https://example.com/a",
    });
    expect(harness.events).toEqual(["download"]);
    expect(reads).toEqual(["/tmp/backup.csv"]);
  });

  it("reads the downloaded file through the filesystem by default", async () => {
    const { mkdtempSync, writeFileSync } = await import("node:fs");
    const { tmpdir } = await import("node:os");
    const { join } = await import("node:path");
    const directory = mkdtempSync(join(tmpdir(), "webhook-541-"));
    const path = join(directory, "backup.csv");
    writeFileSync(path, "id,secret_key\n1,whsec_fixture_default");
    const harness = pageOf({ download: downloadDouble(path) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookSubmitAndCaptureDownload()).resolves.toEqual({
      fileName: "webhook-secret-key-1.csv",
      content: "id,secret_key\n1,whsec_fixture_default",
    });
  });

  it("rejects a create backup that arrived without a path", async () => {
    const harness = pageOf({ download: downloadDouble(null) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookSubmitAndCaptureDownload()).rejects.toThrow(
      "landed without a path"
    );
  });

  it("returns null when a regenerate produces no download", async () => {
    const harness = pageOf({ download: "reject" });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookRegenerateAndCaptureDownload()).resolves.toBeNull();
  });

  it("captures the regenerated backup with its new key", async () => {
    const harness = pageOf({ download: downloadDouble("/tmp/rotated.csv", "webhook-secret-key-2.csv") });
    const driver = new WebNewDriver(harness.page);
    (driver as unknown as { readDownload: (path: string) => string }).readDownload = () =>
      "secret_key\nwhsec_fixture_rotated";
    await expect(driver.wsSettingsWebhookRegenerateAndCaptureDownload()).resolves.toEqual({
      fileName: "webhook-secret-key-2.csv",
      content: "secret_key\nwhsec_fixture_rotated",
    });
  });

  it("rejects a regenerated backup that arrived without a path", async () => {
    const harness = pageOf({ download: downloadDouble(null) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookRegenerateAndCaptureDownload()).rejects.toThrow(
      "landed without a path"
    );
  });
});

describe("web_new webhook driver — secret key", () => {
  it("reads the shown key, its actions and the masked state", async () => {
    const shown = pageOf({
      bySelector: (selector) => (selector === "[data-secret-value]" ? fake({ text: "  whsec_fixture_key  " }) : gone()),
      byRole: (role) => fake({ count: role === "button" ? 1 : 0 }),
    });
    const driver = new WebNewDriver(shown.page);
    await expect(driver.wsSettingsWebhookSecretShown()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookSecretValue()).resolves.toBe("whsec_fixture_key");
    await expect(driver.wsSettingsWebhookSecretActionsVisible()).resolves.toBe(true);

    const nullKey = pageOf({
      bySelector: (selector) => (selector === "[data-secret-value]" ? fake({ text: null }) : gone()),
    });
    await expect(new WebNewDriver(nullKey.page).wsSettingsWebhookSecretShown()).resolves.toBe(true);
    await expect(new WebNewDriver(nullKey.page).wsSettingsWebhookSecretValue()).resolves.toBeNull();
    const masked = pageOf({ bySelector: () => gone() });
    const hidden = new WebNewDriver(masked.page);
    await expect(hidden.wsSettingsWebhookSecretShown()).resolves.toBe(false);
    await expect(hidden.wsSettingsWebhookSecretValue()).resolves.toBeNull();
  });

  it("reports a revealed but unreadable key as present with no value", async () => {
    const harness = pageOf({
      bySelector: (selector) => (selector === "[data-secret-value]" ? fake({ rejects: true }) : gone()),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhookSecretShown()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookSecretValue()).resolves.toBeNull();
  });

  it("reports an unreadable or blank key value as null", async () => {
    const unreadable = pageOf({
      bySelector: (selector) => (selector === "[data-secret-value]" ? fake({ rejects: true }) : gone()),
    });
    await expect(new WebNewDriver(unreadable.page).wsSettingsWebhookSecretValue()).resolves.toBeNull();
    const blank = pageOf({
      bySelector: (selector) => (selector === "[data-secret-value]" ? fake({ text: "   " }) : gone()),
    });
    await expect(new WebNewDriver(blank.page).wsSettingsWebhookSecretValue()).resolves.toBeNull();
  });

  it("reports missing secret actions", async () => {
    await expect(new WebNewDriver(pageOf().page).wsSettingsWebhookSecretActionsVisible()).resolves.toBe(false);
  });

  it("toggles the view and copies through the granted clipboard", async () => {
    const harness = pageOf({ byRole: () => fake({}) });
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookSecretToggleView();
    await driver.wsSettingsWebhookSecretCopy();
    expect(harness.granted).toEqual([["clipboard-read", "clipboard-write"]]);
  });
});

describe("web_new webhook driver — detail", () => {
  function detailForm(
    detail: {
      switchPresent?: boolean;
      checked?: boolean;
      urlError?: boolean;
      updateLabel?: string;
    } = {}
  ): Locator {
    return fake({
      by: (query) => {
        if (query.kind.startsWith("role:switch"))
          return detail.switchPresent === false ? gone() : fake({ checked: detail.checked ?? true });
        if (query.kind.startsWith("role:button") && query.kind.includes("/^(Update|Updating)$/")) {
          return detail.updateLabel === undefined ? gone() : fake({ text: detail.updateLabel });
        }
        if (query.kind === "text" && query.value === "URL is required") return maybe(detail.urlError === true);
        return gone();
      },
    });
  }

  it("opens an unauthenticated detail and waits for any settled state", async () => {
    const harness = pageOf({
      byText: (text) => fake({ count: text === "Webhook details" ? 1 : 0 }),
      byTestId: () => gone(),
    });
    await new WebNewDriver(harness.page).wsSettingsOpenWebhookDetail("acme", "wh-1");
    expect(harness.visited).toEqual(["/acme/settings/webhooks/wh-1"]);
    expect(harness.addedCookies).toEqual([]);
  });

  it("opens a cookie-authenticated detail and stops on the form", async () => {
    const cookies: ParityBrowserCookie[] = [{ name: "session", value: "s", domain: "p.test", path: "/" }];
    const harness = pageOf({
      byTestId: () => gone(),
      byText: (text) => fake({ count: text === "Webhook details" ? 1 : 0 }),
    });
    await new WebNewDriver(harness.page).wsSettingsOpenWebhookDetail("acme", "wh-1", cookies);
    expect(harness.addedCookies).toEqual([cookies]);
    expect(harness.visited).toEqual(["/acme/settings/webhooks/wh-1"]);
  });

  it("keeps polling a cookie-authenticated detail and stops on the denial", async () => {
    let polls = 0;
    const harness = pageOf({
      byTestId: (id) => {
        if (id !== "workspace-settings-not-authorized") return gone();
        polls += 1;
        return fake({ count: polls > 1 ? 1 : 0 });
      },
    });
    await new WebNewDriver(harness.page).wsSettingsOpenWebhookDetail("acme", "wh-1", [
      { name: "session", value: "s", domain: "p.test", path: "/" },
    ]);
    expect(polls).toBeGreaterThan(1);
  });

  it("stops polling a cookie-authenticated detail at the wait budget", async () => {
    const harness = pageOf({
      byTestId: () => gone(),
      byText: () => gone(),
      waitForTimeout: async () => undefined,
    });
    const now = vi.spyOn(Date, "now");
    const started = 1_000_000;
    now
      .mockReturnValueOnce(started)
      .mockReturnValueOnce(started)
      .mockReturnValueOnce(started + 30_001);
    await expect(
      new WebNewDriver(harness.page).wsSettingsOpenWebhookDetail("acme", "wh-slow", [
        { name: "session", value: "s", domain: "p.test", path: "/" },
      ])
    ).resolves.toBeUndefined();
    expect(harness.visited).toEqual(["/acme/settings/webhooks/wh-slow"]);
    now.mockRestore();
  });

  it("reads the detail state from the form marker", async () => {
    const harness = pageOf({
      byText: (text) => fake({ count: text === "Webhook details" ? 1 : 0 }),
      byTestId: () => gone(),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookDetailState()).resolves.toBe("form");
    const failed = pageOf({
      byText: (text) => fake({ count: text === "Couldn't load the webhooks." ? 1 : 0 }),
      byTestId: () => gone(),
    });
    await expect(new WebNewDriver(failed.page).wsSettingsWebhookDetailState()).resolves.toBe("form");
    await expect(new WebNewDriver(pageOf().page).wsSettingsWebhookDetailState()).resolves.toBe("loading");
  });

  it("reads the denial as a view or a paragraph", async () => {
    const view = pageOf({ byTestId: (id) => fake({ count: id === "workspace-settings-not-authorized" ? 1 : 0 }) });
    const viewDriver = new WebNewDriver(view.page);
    await expect(viewDriver.wsSettingsWebhookDetailNotAuthorized()).resolves.toBe(true);
    await expect(viewDriver.wsSettingsWebhookDetailDenialKind()).resolves.toBe("view");
    const paragraph = pageOf({
      byText: (text) => fake({ count: text === "You are not authorized to access this page." ? 1 : 0 }),
    });
    const paragraphDriver = new WebNewDriver(paragraph.page);
    await expect(paragraphDriver.wsSettingsWebhookDetailNotAuthorized()).resolves.toBe(true);
    await expect(paragraphDriver.wsSettingsWebhookDetailDenialKind()).resolves.toBe("paragraph");
    const open = new WebNewDriver(pageOf().page);
    await expect(open.wsSettingsWebhookDetailNotAuthorized()).resolves.toBe(false);
    await expect(open.wsSettingsWebhookDetailDenialKind()).resolves.toBeNull();
  });

  it("shows the loader only before the form or a denial appears", async () => {
    const loading = pageOf({
      byRole: (role, query) => fake({ count: role === "status" && query?.name === "Loading webhook details" ? 1 : 0 }),
    });
    await expect(new WebNewDriver(loading.page).wsSettingsWebhookDetailLoaderVisible()).resolves.toBe(true);
    const form = pageOf({
      byText: (text) => fake({ count: text === "Webhook details" ? 1 : 0 }),
      byRole: (role) => fake({ count: role === "status" ? 1 : 0 }),
    });
    await expect(new WebNewDriver(form.page).wsSettingsWebhookDetailLoaderVisible()).resolves.toBe(false);
    const denied = pageOf({
      byTestId: (id) => fake({ count: id === "workspace-settings-not-authorized" ? 1 : 0 }),
      byRole: (role) => fake({ count: role === "status" ? 1 : 0 }),
    });
    await expect(new WebNewDriver(denied.page).wsSettingsWebhookDetailLoaderVisible()).resolves.toBe(false);
  });

  it("holds only GETs on the detail route", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsWebhookDetailDelayFetch(1);
    expect(harness.routes).toEqual(["**/api/workspaces/*/webhooks/*/"]);
    const handler = harness.handlers[0];
    if (handler === undefined) throw new Error("detail route was not registered");
    const write = { request: () => ({ method: () => "PATCH" }), continue: vi.fn(async () => undefined) };
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledOnce();
    const read = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledOnce();
    const cancelled = {
      request: () => ({ method: () => "GET" }),
      continue: async () => Promise.reject(new Error("cancelled")),
    };
    await expect(handler(cancelled as unknown as Route)).resolves.toBeUndefined();
  });

  it("reads and toggles the form's active switch", async () => {
    const harness = pageOf({
      bySelector: (selector) => (selector === "form" ? detailForm({ checked: false }) : gone()),
    });
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhookDetailActiveOn()).resolves.toBe(false);
    await driver.wsSettingsWebhookDetailToggleActive();
    const absent = new WebNewDriver(pageOf().page);
    await expect(absent.wsSettingsWebhookDetailActiveOn()).resolves.toBe(false);
  });

  it("saves the form and watches the patch or the inline error", async () => {
    const harness = pageOf({ bySelector: (selector) => (selector === "form" ? detailForm() : gone()) });
    await new WebNewDriver(harness.page).wsSettingsWebhookDetailUpdate();
    const predicate = harness.predicates[0];
    expect(predicate?.(response("PATCH", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(true);
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(false);
    expect(predicate?.(response("PATCH", "https://api.test/api/workspaces/acme/webhooks/1"))).toBe(false);
  });

  it("reads a null update label as absent", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({ by: (query) => (query.kind.startsWith("role:button") ? fake({ text: null }) : gone()) })
          : gone(),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookDetailUpdateLabel()).resolves.toBeNull();
  });

  it("reads a null reveal explainer as absent", async () => {
    const harness = pageOf({
      byText: (text) => fake({ count: text === "Key created" ? 1 : 0 }),
      bySelector: () => fake({ by: (query) => (query.value === "p" ? fake({ text: null }) : fake({ text: "" })) }),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookRevealText()).resolves.toBeNull();
  });

  it("reads a null regenerate label as absent", async () => {
    const harness = pageOf({ byRole: () => fake({ text: null }) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookRegenerateLabel()).resolves.toBeNull();
  });

  it("reads the update label, and null when the control is gone", async () => {
    const harness = pageOf({
      bySelector: (selector) => (selector === "form" ? detailForm({ updateLabel: " Updating " }) : gone()),
    });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookDetailUpdateLabel()).resolves.toBe("Updating");
    await expect(new WebNewDriver(pageOf().page).wsSettingsWebhookDetailUpdateLabel()).resolves.toBeNull();
    const blank = pageOf({
      bySelector: (selector) => (selector === "form" ? detailForm({ updateLabel: "  " }) : gone()),
    });
    await expect(new WebNewDriver(blank.page).wsSettingsWebhookDetailUpdateLabel()).resolves.toBeNull();
    const unreadable = pageOf({
      bySelector: (selector) =>
        selector === "form"
          ? fake({
              by: (query) => (query.kind.startsWith("role:button") ? fake({ rejects: true }) : gone()),
            })
          : gone(),
    });
    await expect(new WebNewDriver(unreadable.page).wsSettingsWebhookDetailUpdateLabel()).resolves.toBeNull();
  });

  it("holds only PATCHes while an update is delayed", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsWebhookDetailDelayUpdate(1);
    const handler = harness.handlers[0];
    if (handler === undefined) throw new Error("update route was not registered");
    const read = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledOnce();
    const write = { request: () => ({ method: () => "PATCH" }), continue: vi.fn(async () => undefined) };
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledOnce();
  });

  it("finds the secret block by its title", async () => {
    const harness = pageOf({ byText: (text) => fake({ count: text === "Secret key" ? 1 : 0 }) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookSecretBlockVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(pageOf().page).wsSettingsWebhookSecretBlockVisible()).resolves.toBe(false);
  });

  it("regenerates and watches the regenerate answer", async () => {
    const harness = pageOf({ byRole: () => fake({}) });
    await new WebNewDriver(harness.page).wsSettingsWebhookRegenerate();
    const predicate = harness.predicates[0];
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/1/regenerate/"))).toBe(true);
    expect(predicate?.(response("GET", "https://api.test/api/workspaces/acme/webhooks/1/regenerate/"))).toBe(false);
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(false);
  });

  it("reads the regenerate label, and null when the control is gone or blank", async () => {
    const harness = pageOf({ byRole: () => fake({ text: " Re-generating… " }) });
    await expect(new WebNewDriver(harness.page).wsSettingsWebhookRegenerateLabel()).resolves.toBe("Re-generating…");
    await expect(new WebNewDriver(pageOf().page).wsSettingsWebhookRegenerateLabel()).resolves.toBeNull();
    const blank = pageOf({ byRole: () => fake({ text: "" }) });
    await expect(new WebNewDriver(blank.page).wsSettingsWebhookRegenerateLabel()).resolves.toBeNull();
    const unreadable = pageOf({ byRole: () => fake({ rejects: true }) });
    await expect(new WebNewDriver(unreadable.page).wsSettingsWebhookRegenerateLabel()).resolves.toBeNull();
  });

  it("holds only regenerate POSTs while a rotation is delayed", async () => {
    const harness = pageOf();
    await new WebNewDriver(harness.page).wsSettingsWebhookDelayRegenerate(1);
    const handler = harness.handlers[0];
    if (handler === undefined) throw new Error("regenerate route was not registered");
    const read = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledOnce();
    const write = { request: () => ({ method: () => "POST" }), continue: vi.fn(async () => undefined) };
    await handler(write as unknown as Route);
    expect(write.continue).toHaveBeenCalledOnce();
  });
});

describe("web_new webhook driver — danger zone", () => {
  function deletePage(
    options: {
      panel?: boolean;
      dialogOpen?: boolean;
      withParagraph?: boolean;
      paragraphText?: string | null;
    } = {}
  ): Harness {
    return pageOf({
      byRole: (role, query) =>
        query?.name === "Danger zone" && role === "button"
          ? fake({})
          : query?.name === "Delete webhook" && role === "button"
            ? maybe(options.panel ?? true)
            : gone(),
      byText: (text) =>
        fake({ count: text.startsWith("Once a webhook is deleted") && (options.panel ?? true) ? 1 : 0 }),
      bySelector: (selector) =>
        selector.includes('role="dialog"')
          ? options.dialogOpen === false
            ? gone()
            : dialogWith({
                paragraphs: () =>
                  options.withParagraph === false
                    ? gone()
                    : fake({
                        text:
                          options.paragraphText === undefined
                            ? "  This action cannot be undone.  "
                            : options.paragraphText,
                      }),
                buttons: () => fake({}),
              })
          : gone(),
    });
  }

  it("reports the section, expands it and reads its panel", async () => {
    const harness = deletePage();
    const driver = new WebNewDriver(harness.page);
    await expect(driver.wsSettingsWebhookDeleteSectionVisible()).resolves.toBe(true);
    await driver.wsSettingsWebhookDeleteSectionExpand();
    await expect(driver.wsSettingsWebhookDeletePanelVisible()).resolves.toBe(true);
    const sectionLess = pageOf({ byRole: () => gone(), byText: () => gone(), bySelector: () => gone() });
    await expect(new WebNewDriver(sectionLess.page).wsSettingsWebhookDeleteSectionVisible()).resolves.toBe(false);
    await expect(
      new WebNewDriver(deletePage({ panel: false }).page).wsSettingsWebhookDeletePanelVisible()
    ).resolves.toBe(false);
  });

  it("opens the dialog and reads its text", async () => {
    const harness = deletePage();
    const driver = new WebNewDriver(harness.page);
    await driver.wsSettingsWebhookDeleteOpen();
    await expect(driver.wsSettingsWebhookDeleteDialogOpen()).resolves.toBe(true);
    await expect(driver.wsSettingsWebhookDeleteDialogText()).resolves.toBe("This action cannot be undone.");
    await expect(
      new WebNewDriver(deletePage({ dialogOpen: false }).page).wsSettingsWebhookDeleteDialogOpen()
    ).resolves.toBe(false);
    await expect(
      new WebNewDriver(deletePage({ dialogOpen: false }).page).wsSettingsWebhookDeleteDialogText()
    ).resolves.toBeNull();
    await expect(
      new WebNewDriver(deletePage({ withParagraph: false }).page).wsSettingsWebhookDeleteDialogText()
    ).resolves.toBeNull();
    await expect(
      new WebNewDriver(deletePage({ paragraphText: null }).page).wsSettingsWebhookDeleteDialogText()
    ).resolves.toBeNull();
  });

  it("confirms and watches the delete answer", async () => {
    const harness = deletePage();
    await new WebNewDriver(harness.page).wsSettingsWebhookDeleteConfirm();
    const predicate = harness.predicates[0];
    expect(predicate?.(response("DELETE", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(true);
    expect(predicate?.(response("POST", "https://api.test/api/workspaces/acme/webhooks/1/"))).toBe(false);
    expect(predicate?.(response("DELETE", "https://api.test/api/workspaces/acme/webhooks/1"))).toBe(false);
  });

  it("cancels and waits for the dialog to detach", async () => {
    const waits: unknown[] = [];
    const harness = pageOf({
      bySelector: (selector, query) => {
        return selector.includes('role="dialog"') && markerText(query?.has) === "Delete webhook"
          ? dialogWith({
              buttons: () => fake({}),
              waitFor: (options) => waits.push(options),
            })
          : gone();
      },
    });
    await new WebNewDriver(harness.page).wsSettingsWebhookDeleteCancel();
    expect(waits).toContainEqual({ state: "detached", timeout: 30_000 });
  });
});

describe("web_new webhook driver — notices", () => {
  it("reads the newest toast before any status or alert", async () => {
    const harness = pageOf({
      bySelector: (selector) =>
        selector.includes('aria-label="Notifications"') ? fake({ text: "Webhook created" }) : gone(),
    });
    await expect(new WebNewDriver(harness.page).lastToast()).resolves.toBe("Webhook created");
  });

  it("falls back past a blank alert-region toast to the alert itself", async () => {
    const harness = pageOf({ bySelector: () => gone(), byRole: () => gone() });
    const page = harness.page;
    (page as unknown as { getByRole: Page["getByRole"] }).getByRole = ((role: string) => {
      if (role === "alert") return fake({ text: "Webhook deleted" });
      return gone();
    }) as Page["getByRole"];
    await expect(new WebNewDriver(page).lastToast()).resolves.toBe("Webhook deleted");
  });

  it("treats null toast, status and alert text as absent", async () => {
    const nullText = {
      count: async () => 1,
      innerText: async () => null,
      first: () => nullText as unknown as Locator,
    } as unknown as Locator;
    for (const role of ["status", "alert"] as const) {
      const harness = pageOf({ bySelector: () => gone() });
      const page = harness.page;
      (page as unknown as { getByRole: Page["getByRole"] }).getByRole = ((asked: string) => {
        if (asked === role) return nullText;
        return gone();
      }) as Page["getByRole"];
      await expect(new WebNewDriver(page).lastToast()).resolves.toBeNull();
    }
    const harness = pageOf({ byRole: () => gone() });
    (harness.page as unknown as { locator: Page["locator"] }).locator = ((selector: string) =>
      selector.includes('aria-label="Notifications"') ? nullText : gone()) as Page["locator"];
    await expect(new WebNewDriver(harness.page).lastToast()).resolves.toBeNull();
  });

  it("falls back past an empty toast to a status region", async () => {
    const harness = pageOf({
      bySelector: (selector) => (selector.includes('aria-label="Notifications"') ? fake({ text: "  " }) : gone()),
      byRole: (role) => (role === "status" ? fake({ text: "Loading webhooks" }) : gone()),
    });
    await expect(new WebNewDriver(harness.page).lastToast()).resolves.toBe("Loading webhooks");
  });

  it("falls back past an unreadable toast and an empty status to an alert", async () => {
    const harness = pageOf({
      bySelector: (selector) => (selector.includes('aria-label="Notifications"') ? fake({ rejects: true }) : gone()),
      byRole: (role) => (role === "status" ? fake({ rejects: true }) : gone()),
      byTestId: () => gone(),
    });
    const page = harness.page;
    const original = page.getByRole.bind(page);
    (page as unknown as { getByRole: Page["getByRole"] }).getByRole = ((role: string, query?: RoleQuery) => {
      if (role === "alert") return fake({ text: "Webhook deleted" });
      return original(role, query);
    }) as Page["getByRole"];
    await expect(new WebNewDriver(page).lastToast()).resolves.toBe("Webhook deleted");
  });

  it("reads a blank alert as no toast", async () => {
    const harness = pageOf({ bySelector: () => gone(), byRole: () => gone() });
    const page = harness.page;
    const original = page.getByRole.bind(page);
    (page as unknown as { getByRole: Page["getByRole"] }).getByRole = ((role: string, query?: RoleQuery) => {
      if (role === "alert") return fake({ text: "  " });
      return original(role, query);
    }) as Page["getByRole"];
    await expect(new WebNewDriver(page).lastToast()).resolves.toBeNull();
  });

  it("reads no toast when neither region nor alert is present", async () => {
    await expect(new WebNewDriver(pageOf().page).lastToast()).resolves.toBeNull();
  });
});
