// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver project feature-flag and automations
// methods (NEWFRONT-582, PSET-043–048, 053, 054). A small Locator/Page
// double proves the selectors, read/action branches and the project-write
// waiters without bringing up the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SettingsGeneralField } from "./parity-driver";
import { WebNewDriver } from "./web-new";

afterEach(() => vi.restoreAllMocks());

interface FakeOptions {
  count?: number;
  visible?: boolean;
  text?: string;
  input?: string;
  checked?: boolean;
  disabled?: boolean;
  waitForRejects?: boolean;
  items?: FakeLocator[];
  role?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => FakeLocator;
  textLocator?: (text: string | RegExp, options?: { exact?: boolean }) => FakeLocator;
  testId?: (testId: string) => FakeLocator;
  locator?: (selector: string) => FakeLocator;
  filtered?: FakeLocator;
  derived?: FakeLocator;
}

class FakeLocator {
  readonly click = vi.fn(async () => undefined);
  readonly fill = vi.fn(async (_value: string) => undefined);
  readonly waitFor = vi.fn(async (_options?: unknown) => {
    if (this.options.waitForRejects === true) {
      // Playwright spends the whole timeout on an absent locator before it
      // rejects; a raced visible locator therefore wins. Keep that order.
      await new Promise((resolve) => setTimeout(resolve, 5));
      throw new Error("timed out");
    }
  });
  private readonly value: string;

  constructor(private readonly options: FakeOptions = {}) {
    this.value = options.input ?? "";
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.items?.length ?? this.options.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    return Promise.resolve(this.options.visible ?? true);
  }

  isChecked(): Promise<boolean> {
    return Promise.resolve(this.options.checked ?? false);
  }

  isDisabled(): Promise<boolean> {
    return Promise.resolve(this.options.disabled ?? false);
  }

  innerText(): Promise<string> {
    return Promise.resolve(this.options.text ?? "");
  }

  inputValue(): Promise<string> {
    return Promise.resolve(this.value);
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

  filter(_options: unknown): FakeLocator {
    return this.options.filtered ?? this;
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): FakeLocator {
    return this.options.role?.(role, options) ?? absent();
  }

  getByText(text: string | RegExp, options?: { exact?: boolean }): FakeLocator {
    return this.options.textLocator?.(text, options) ?? absent();
  }

  getByTestId(testId: string): FakeLocator {
    return this.options.testId?.(testId) ?? absent();
  }

  getByPlaceholder(_placeholder: string): FakeLocator {
    return this.options.derived ?? absent();
  }

  locator(selector: string): FakeLocator {
    return this.options.locator?.(selector) ?? absent();
  }
}

function absent(): FakeLocator {
  return new FakeLocator({ count: 0, visible: false, waitForRejects: true });
}

interface PageOptions {
  switchLabel?: string;
  switchOn?: boolean;
  switchDisabled?: boolean;
  closeOn?: boolean;
  denied?: boolean;
  headings?: string[];
  archiveVisible?: boolean;
  closeVisible?: boolean;
  delayText?: string;
  triggers?: number;
  optionLabels?: string[];
  monthOpen?: boolean;
  monthInput?: string;
  monthError?: string;
  closeStateText?: string;
  closeStateDisabled?: boolean;
  loaderVisible?: boolean;
  navVisible?: boolean;
  createDialog?: boolean;
  createStep?: boolean;
  writeRejects?: boolean;
}

function pageDouble(options: PageOptions = {}) {
  const monthInput = new FakeLocator({
    count: options.monthOpen === false ? 0 : 1,
    input: options.monthInput ?? "",
    waitForRejects: options.monthOpen === false,
  });
  const createInput = new FakeLocator({ count: options.createDialog === false ? 0 : 1 });
  const navLink = new FakeLocator({
    count: 1,
    visible: options.navVisible ?? true,
    waitForRejects: options.navVisible === false,
  });
  const route: Route = {
    request: () => ({ method: () => "GET" }),
    continue: vi.fn(async () => undefined),
  } as unknown as Route;

  const page = {
    goto: vi.fn(async (_url: string) => undefined),
    waitForLoadState: vi.fn(async (_state: string) => undefined),
    waitForTimeout: vi.fn(async (_ms: number) => undefined),
    route: vi.fn(async (_pattern: string, _handler: (route: Route) => Promise<void>) => undefined),
    waitForResponse: vi.fn(async (_predicate: unknown, _options?: unknown) => {
      if (options.writeRejects === true) throw new Error("write never came");
      return undefined;
    }),
    on: vi.fn((_event: string, _handler: (socket: unknown) => void) => undefined),
    off: vi.fn((_event: string, _handler: (socket: unknown) => void) => undefined),
    evaluate: vi.fn(async () => "shell"),
    getByPlaceholder: vi.fn((placeholder: string) => (placeholder === "Project name" ? createInput : monthInput)),
    getByTestId: vi.fn((testId: string) => {
      if (testId === "project-settings-denied") return options.denied === true ? new FakeLocator() : absent();
      if (testId === "automations-not-authorized") return options.denied === true ? new FakeLocator() : absent();
      return absent();
    }),
    getByText: vi.fn((text: string | RegExp) =>
      String(text).includes("Select a month") && options.monthError !== undefined
        ? new FakeLocator({ text: options.monthError })
        : absent()
    ),
    getByRole: vi.fn((role: string, roleOptions?: { name?: string | RegExp; exact?: boolean }) => {
      if (role === "switch") {
        if (typeof roleOptions?.name === "string" && roleOptions.name.startsWith("Auto-")) {
          const title = roleOptions.name;
          const visible = title.startsWith("Auto-archive")
            ? options.archiveVisible !== false
            : options.closeVisible === true;
          return visible
            ? new FakeLocator({
                checked: title.startsWith("Auto-archive") ? (options.switchOn ?? false) : (options.closeOn ?? false),
              })
            : absent();
        }
        if (options.switchLabel === undefined) return absent();
        const name = String(roleOptions?.name ?? "");
        if (name !== "" && !options.switchLabel.includes(name)) return absent();
        return new FakeLocator({ checked: options.switchOn ?? false, disabled: options.switchDisabled ?? false });
      }
      if (role === "heading") {
        const name = String(roleOptions?.name ?? "");
        const headings = options.headings ?? [];
        if (name === "Auto-archive closed work items") {
          return options.archiveVisible === false ? absent() : new FakeLocator();
        }
        const known = headings.includes(name) || name === "Automations";
        if (!known) return absent();
        if (name === "Auto-archive closed work items" && options.archiveVisible === false) return absent();
        return new FakeLocator();
      }
      if (role === "button" && roleOptions?.name instanceof RegExp) {
        return new FakeLocator({
          items: Array.from(
            { length: options.triggers ?? 0 },
            () => new FakeLocator({ text: options.delayText ?? "" })
          ),
        });
      }
      if (role === "button" && roleOptions?.name === "Customize time range") return new FakeLocator();
      if (role === "button" && roleOptions?.name === "Submit") return new FakeLocator();
      if (role === "option") {
        return new FakeLocator({ items: (options.optionLabels ?? []).map((text) => new FakeLocator({ text })) });
      }
      if (role === "dialog") {
        const dialog = new FakeLocator({
          count: options.createDialog === false ? 0 : 1,
          role: (innerRole) => (innerRole === "link" && options.createStep === true ? new FakeLocator() : absent()),
        });
        (dialog as unknown as { has: unknown }).has = vi.fn();
        return dialog;
      }
      return absent();
    }),
    locator: vi.fn((selector: string) => {
      if (selector.startsWith(`a[href^=`)) return navLink;
      if (selector === '[role="status"]') return new FakeLocator({ count: options.loaderVisible === true ? 1 : 0 });
      if (selector === 'button[aria-haspopup="listbox"]') {
        return new FakeLocator({
          items: [
            new FakeLocator(),
            new FakeLocator({ text: options.closeStateText ?? "", disabled: options.closeStateDisabled ?? false }),
          ],
        });
      }
      if (selector === "[data-automation-row]") {
        return new FakeLocator({
          filtered: new FakeLocator({
            locator: () =>
              new FakeLocator({
                items: [
                  new FakeLocator(),
                  new FakeLocator({
                    text: options.closeStateText ?? "",
                    disabled: options.closeStateDisabled ?? false,
                  }),
                ],
              }),
          }),
          items: [
            ...(options.archiveVisible === false ? [] : [new FakeLocator({ text: "Auto-archive closed work items" })]),
            ...(options.closeVisible === true ? [new FakeLocator({ text: "Auto-close work items" })] : []),
          ],
        });
      }
      return absent();
    }),
  };
  return { page: page as unknown as Page, pageObject: page, monthInput, route };
}

describe("WebNewDriver feature tabs", () => {
  it("opens the tab and settles on its named switch", async () => {
    const stub = pageDouble({ switchLabel: "Enable views" });
    const driver = new WebNewDriver(stub.page);
    await driver.projectFeaturesOpenTab("acme", "p1", "views");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1/features/views");
  });

  it("settles on the denial view when the tab refuses the caller", async () => {
    const stub = pageDouble({ denied: true });
    const driver = new WebNewDriver(stub.page);
    await driver.projectFeaturesOpenTab("acme", "p1", "pages");
    await expect(driver.projectFeaturesNotAuthorized()).resolves.toBe(true);
  });

  it("throws when neither the switch nor the denial view settles", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectFeaturesOpenTab("acme", "p1", "cycles")).rejects.toThrow(
      "settled on neither the switch nor the denial view"
    );
  });

  it("reads the switch state, disabled flag and toggles while waiting for the write", async () => {
    const stub = pageDouble({ switchLabel: "Enable intake", switchOn: true, switchDisabled: true });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectFeaturesSwitchOn()).resolves.toBe(true);
    await expect(driver.projectFeaturesSwitchDisabled()).resolves.toBe(true);
    await driver.projectFeaturesToggle();
    expect(stub.pageObject.waitForResponse).toHaveBeenCalled();
  });

  it("reads an absent switch as off and enabled", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectFeaturesSwitchOn()).resolves.toBe(false);
    await expect(driver.projectFeaturesSwitchDisabled()).resolves.toBe(false);
  });

  it("reports the area nav link by polling for it on the project", async () => {
    const shown = pageDouble({ navVisible: true });
    await expect(new WebNewDriver(shown.page).projectFeaturesNavLinkVisible("acme", "p1", "modules")).resolves.toBe(
      true
    );
    expect(shown.pageObject.goto).toHaveBeenCalledWith("/acme/projects/p1/issues");

    const hidden = pageDouble({ navVisible: false });
    await expect(new WebNewDriver(hidden.page).projectFeaturesNavLinkVisible("acme", "p1", "modules")).resolves.toBe(
      false
    );
  });
});

describe("WebNewDriver automations", () => {
  it("opens the tab and settles on the archive row", async () => {
    const stub = pageDouble({ archiveVisible: true });
    await new WebNewDriver(stub.page).projectAutomationsOpenTab("acme", "p1");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1/automations");
  });

  it("settles on the denial view for a refused caller", async () => {
    const stub = pageDouble({ archiveVisible: false, denied: true });
    const driver = new WebNewDriver(stub.page);
    await driver.projectAutomationsOpenTab("acme", "p1");
    await expect(driver.projectAutomationsNotAuthorized()).resolves.toBe(true);
  });

  it("throws when neither rows nor the denial view settle", async () => {
    const stub = pageDouble({ archiveVisible: false });
    await expect(new WebNewDriver(stub.page).projectAutomationsOpenTab("acme", "p1")).rejects.toThrow(
      "settled on neither the rows nor the denial view"
    );
  });

  it("reads both switches and toggles each under its own project write", async () => {
    const stub = pageDouble({ archiveVisible: true, closeVisible: true, switchOn: true, closeOn: false });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectAutomationsArchiveOn()).resolves.toBe(true);
    await expect(driver.projectAutomationsCloseOn()).resolves.toBe(false);
    await driver.projectAutomationsToggleArchive();
    await driver.projectAutomationsToggleClose();
    expect(stub.pageObject.waitForResponse).toHaveBeenCalledTimes(2);
  });

  it("reads an absent row as off", async () => {
    const stub = pageDouble({ archiveVisible: false, closeVisible: false });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectAutomationsArchiveOn()).resolves.toBe(false);
    await expect(driver.projectAutomationsCloseOn()).resolves.toBe(false);
  });

  it("reports and holds a loader window", async () => {
    const stub = pageDouble({ loaderVisible: true });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.projectAutomationsLoaderVisible()).resolves.toBe(true);
    await driver.projectAutomationsDelayDetailsFetch(25);
    expect(stub.pageObject.route).toHaveBeenCalledWith("**/api/workspaces/*/projects/*/", expect.any(Function));
    const handler = stub.pageObject.route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    await handler(stub.route);
    expect((stub.route as unknown as { continue: ReturnType<typeof vi.fn> }).continue).toHaveBeenCalled();
  });

  it("lets a write through the held-read proxy without delaying it", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.projectAutomationsDelayDetailsFetch(0);
    const handler = stub.pageObject.route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    const writeRoute = {
      request: () => ({ method: () => "PATCH" }),
      continue: vi.fn(async () => undefined),
    } as unknown as Route;
    await handler(writeRoute);
    expect((writeRoute as unknown as { continue: ReturnType<typeof vi.fn> }).continue).toHaveBeenCalled();
  });

  it("reads the single delay trigger text and refuses ambiguity", async () => {
    const one = pageDouble({ triggers: 1, delayText: "3 months" });
    await expect(new WebNewDriver(one.page).projectAutomationsDelayTriggerText()).resolves.toBe("3 months");
    const none = pageDouble({ triggers: 0 });
    await expect(new WebNewDriver(none.page).projectAutomationsDelayTriggerText()).resolves.toBeNull();
    const two = pageDouble({ triggers: 2, delayText: "1 month" });
    await expect(new WebNewDriver(two.page).projectAutomationsDelayTriggerText()).resolves.toBeNull();
  });

  it("opens the delay options and lists their labels", async () => {
    const stub = pageDouble({ triggers: 1, delayText: "1 month", optionLabels: ["1 month", "3 months", "6 months"] });
    const driver = new WebNewDriver(stub.page);
    await driver.projectAutomationsOpenDelayOptions();
    await expect(driver.projectAutomationsDelayOptionLabels()).resolves.toEqual(["1 month", "3 months", "6 months"]);
  });

  it("picks a preset under the project write", async () => {
    const stub = pageDouble({ optionLabels: ["6 months"] });
    await new WebNewDriver(stub.page).projectAutomationsPickDelayOption("6 months");
    expect(stub.pageObject.waitForResponse).toHaveBeenCalled();
  });

  it("opens the custom dialog and reports its open state and error", async () => {
    const open = pageDouble({ monthOpen: true, monthError: "Select a month between 1 and 12." });
    const driver = new WebNewDriver(open.page);
    await driver.projectAutomationsOpenCustomize();
    await expect(driver.projectAutomationsMonthDialogOpen()).resolves.toBe(true);
    await expect(driver.projectAutomationsMonthDialogError()).resolves.toBe("Select a month between 1 and 12.");

    const closed = pageDouble({ monthOpen: false });
    const closedDriver = new WebNewDriver(closed.page);
    await expect(closedDriver.projectAutomationsMonthDialogOpen()).resolves.toBe(false);
    await expect(closedDriver.projectAutomationsMonthDialogError()).resolves.toBeNull();
  });

  it("submits a custom month and waits for the write or the refusal", async () => {
    const stub = pageDouble({ monthOpen: true });
    const driver = new WebNewDriver(stub.page);
    await driver.projectAutomationsMonthDialogSubmit("6");
    expect(stub.monthInput.fill).toHaveBeenCalledWith("6");
    expect(stub.pageObject.waitForResponse).toHaveBeenCalled();
  });

  it("reads the close state trigger text and disabled flag", async () => {
    const open = pageDouble({ closeVisible: true, closeStateText: "Cancelled", closeStateDisabled: true });
    const driver = new WebNewDriver(open.page);
    await expect(driver.projectAutomationsCloseStateText()).resolves.toBe("Cancelled");
    await expect(driver.projectAutomationsCloseStateDisabled()).resolves.toBe(true);

    const hidden = pageDouble({ closeVisible: false });
    const hiddenDriver = new WebNewDriver(hidden.page);
    await expect(hiddenDriver.projectAutomationsCloseStateText()).resolves.toBeNull();
    await expect(hiddenDriver.projectAutomationsCloseStateDisabled()).resolves.toBe(false);
  });
});

describe("WebNewDriver settings shell sockets", () => {
  function socketDouble(url: string) {
    return { url: () => url };
  }

  it("ignores vite HMR sockets and counts app sockets", async () => {
    const stub = pageDouble();
    let visits = 0;
    stub.pageObject.goto.mockImplementation(async () => {
      const calls = stub.pageObject.on.mock.calls;
      if (calls.length === 0 || calls[0] === undefined) throw new Error("websocket handler not registered");
      const handler = calls[0][1] as (socket: unknown) => void;
      visits += 1;
      handler(socketDouble("ws://localhost:3016/?token=hmr"));
      if (visits === 1) {
        handler(socketDouble("ws://localhost:18006/ws/realtime/"));
        handler(socketDouble("ws://localhost:18006/ws/live/?token=app"));
      }
    });
    const count = await new WebNewDriver(stub.page).settingsShellCountSockets(["", "/members"], "acme", "p1");
    // Two HMR sockets ignored; both app sockets counted even though one
    // carries its own token query (only the root-path HMR shape filters).
    expect(count).toBe(2);
    expect(stub.pageObject.goto).toHaveBeenCalledTimes(2);
    expect(stub.pageObject.off).toHaveBeenCalledTimes(1);
  });
});

describe("WebNewDriver create feature step", () => {
  it("reports no step once creation closed the dialog", async () => {
    const stub = pageDouble({ createDialog: false });
    await expect(new WebNewDriver(stub.page).isCreateFeatureStepVisible()).resolves.toBe(false);
  });

  it("reports no step on the plain open create form", async () => {
    const stub = pageDouble({ createDialog: true, createStep: false });
    await expect(new WebNewDriver(stub.page).isCreateFeatureStepVisible()).resolves.toBe(false);
  });

  it("reports a step when the dialog offers its open-project exit", async () => {
    const stub = pageDouble({ createDialog: true, createStep: true });
    await expect(new WebNewDriver(stub.page).isCreateFeatureStepVisible()).resolves.toBe(true);
  });
});

interface GeneralFakeState {
  count?: number;
  visible?: boolean;
  text?: string;
  input?: string;
  checked?: boolean;
  disabled?: boolean;
  attr?: Record<string, string | null>;
  items?: GeneralFakeLocator[];
  waitRejects?: boolean;
}

type GeneralLocatorQuery = (selector: string) => GeneralFakeLocator;
type GeneralTestIdQuery = (testId: string) => GeneralFakeLocator;
type GeneralRoleQuery = (role: string, options?: { name?: string | RegExp; exact?: boolean }) => GeneralFakeLocator;
type GeneralLabelQuery = (label: string) => GeneralFakeLocator;
type GeneralPlaceholderQuery = (text: string) => GeneralFakeLocator;

class GeneralFakeLocator {
  readonly click = vi.fn(async () => undefined);
  readonly fill = vi.fn(async (_value: string) => undefined);
  readonly press = vi.fn(async (_key: string) => undefined);
  readonly pressSequentially = vi.fn(async (_value: string) => undefined);
  readonly setChecked = vi.fn(async (_checked: boolean) => undefined);
  readonly hover = vi.fn(async () => undefined);
  readonly scrollIntoViewIfNeeded = vi.fn(async () => undefined);
  readonly dragTo = vi.fn(async (_target: unknown, _options?: unknown) => undefined);
  readonly evaluate = vi.fn(async (_fn: unknown) => undefined);
  readonly boundingBox = vi.fn(async () => null);
  readonly waitFor = vi.fn(async (_options?: unknown) => {
    if (this.state.waitRejects === true) throw new Error("timed out");
  });
  readonly state: GeneralFakeState;
  private readonly queries: {
    locator?: GeneralLocatorQuery;
    testId?: GeneralTestIdQuery;
    role?: GeneralRoleQuery;
    label?: GeneralLabelQuery;
    placeholder?: GeneralPlaceholderQuery;
  };

  constructor(
    state: GeneralFakeState = {},
    queries: {
      locator?: GeneralLocatorQuery;
      testId?: GeneralTestIdQuery;
      role?: GeneralRoleQuery;
      label?: GeneralLabelQuery;
      placeholder?: GeneralPlaceholderQuery;
    } = {}
  ) {
    this.state = { attr: {}, ...state };
    this.queries = queries;
  }

  count(): Promise<number> {
    return Promise.resolve(this.state.items?.length ?? this.state.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    return Promise.resolve(this.state.visible ?? true);
  }

  isChecked(): Promise<boolean> {
    return Promise.resolve(this.state.checked ?? false);
  }

  isDisabled(): Promise<boolean> {
    return Promise.resolve(this.state.disabled ?? false);
  }

  isEnabled(): Promise<boolean> {
    return Promise.resolve(!(this.state.disabled ?? false));
  }

  innerText(): Promise<string> {
    return Promise.resolve(this.state.text ?? "");
  }

  textContent(): Promise<string | null> {
    return Promise.resolve(this.state.text ?? null);
  }

  inputValue(): Promise<string> {
    return Promise.resolve(this.state.input ?? "");
  }

  getAttribute(name: string): Promise<string | null> {
    return Promise.resolve(this.state.attr?.[name] ?? null);
  }

  first(): GeneralFakeLocator {
    return this.state.items?.[0] ?? this;
  }

  last(): GeneralFakeLocator {
    return this.state.items?.at(-1) ?? this;
  }

  nth(index: number): GeneralFakeLocator {
    return this.state.items?.[index] ?? generalAbsent();
  }

  locator(selector: string): GeneralFakeLocator {
    return this.queries.locator?.(selector) ?? generalAbsent();
  }

  getByTestId(testId: string): GeneralFakeLocator {
    return this.queries.testId?.(testId) ?? generalAbsent();
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean }): GeneralFakeLocator {
    return this.queries.role?.(role, options) ?? generalAbsent();
  }

  getByLabel(label: string): GeneralFakeLocator {
    return this.queries.label?.(label) ?? generalAbsent();
  }

  getByPlaceholder(text: string): GeneralFakeLocator {
    return this.queries.placeholder?.(text) ?? generalAbsent();
  }
}

function generalAbsent(): GeneralFakeLocator {
  return new GeneralFakeLocator({ count: 0, visible: false, waitRejects: true });
}

interface GeneralPageHooks {
  locator?: GeneralLocatorQuery;
  testId?: GeneralTestIdQuery;
  role?: GeneralRoleQuery;
  text?: (text: string) => GeneralFakeLocator;
  response?: "resolve" | "reject";
  responseStatus?: number;
}

function generalSettingsPageDouble(hooks: GeneralPageHooks = {}) {
  const calls = { locators: [] as string[], testIds: [] as string[], pressed: [] as string[] };
  const routes: Array<{ pattern: unknown; action: "route" | "unroute" }> = [];
  const page = {
    goto: vi.fn(async (_url: string) => undefined),
    waitForLoadState: vi.fn(async (_state: string) => undefined),
    waitForTimeout: vi.fn(async (_ms: number) => undefined),
    waitForEvent: vi.fn(async (_event: string) => undefined),
    waitForResponse: vi.fn(async () => {
      if (hooks.response === "reject") throw new Error("no request fired");
      return { status: () => hooks.responseStatus ?? 200, url: () => "https://parity.test/repository/bind/" };
    }),
    route: vi.fn(async (pattern: unknown, _handler?: unknown) => {
      routes.push({ pattern, action: "route" });
    }),
    unroute: vi.fn(async (pattern: unknown) => {
      routes.push({ pattern, action: "unroute" });
    }),
    keyboard: { press: vi.fn(async (key: string) => calls.pressed.push(key)) },
    locator: vi.fn((selector: string) => {
      calls.locators.push(selector);
      return hooks.locator?.(selector) ?? generalAbsent();
    }),
    getByTestId: vi.fn((testId: string) => {
      calls.testIds.push(testId);
      return hooks.testId?.(testId) ?? generalAbsent();
    }),
    getByRole: vi.fn((role: string, options?: { name?: string | RegExp; exact?: boolean }) => {
      return hooks.role?.(role, options) ?? generalAbsent();
    }),
    getByText: vi.fn((text: string) => {
      return hooks.text?.(text) ?? generalAbsent();
    }),
    evaluate: vi.fn(async () => "none"),
    mouse: {
      move: vi.fn(async (_x: number, _y: number) => undefined),
      down: vi.fn(async () => undefined),
      up: vi.fn(async () => undefined),
    },
  };
  return { page: page as unknown as Page, pageObject: page, calls, routes };
}

describe("WebNewDriver general tab open and state", () => {
  it("opens the tab address and settles on the name field", async () => {
    const stub = generalSettingsPageDouble({ locator: () => new GeneralFakeLocator() });
    await new WebNewDriver(stub.page).settingsGeneralOpen("acme", "p1");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1");
  });

  it("reads the form, skeleton and danger zone presence", async () => {
    const present = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator(),
      testId: () => new GeneralFakeLocator(),
    });
    const driver = new WebNewDriver(present.page);
    await expect(driver.settingsGeneralFormVisible()).resolves.toBe(true);
    await expect(driver.settingsGeneralSkeletonVisible()).resolves.toBe(true);
    await expect(driver.settingsGeneralDangerVisible()).resolves.toBe(true);

    const missing = generalSettingsPageDouble();
    const missingDriver = new WebNewDriver(missing.page);
    await expect(missingDriver.settingsGeneralFormVisible()).resolves.toBe(false);
    await expect(missingDriver.settingsGeneralSkeletonVisible()).resolves.toBe(false);
    await expect(missingDriver.settingsGeneralDangerVisible()).resolves.toBe(false);
  });

  it("reads the dimmed marker off the form class", async () => {
    const dimmed = generalSettingsPageDouble({
      locator: (selector) =>
        selector.startsWith("form") ? new GeneralFakeLocator({ attr: { class: "flex opacity-60" } }) : generalAbsent(),
    });
    await expect(new WebNewDriver(dimmed.page).settingsGeneralFormDimmed()).resolves.toBe(true);

    const bright = generalSettingsPageDouble({
      locator: (selector) =>
        selector.startsWith("form") ? new GeneralFakeLocator({ attr: { class: "flex" } }) : generalAbsent(),
    });
    await expect(new WebNewDriver(bright.page).settingsGeneralFormDimmed()).resolves.toBe(false);

    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralFormDimmed()).resolves.toBe(false);
  });
});

describe("WebNewDriver general tab fields", () => {
  const FIELD_IDS: Record<SettingsGeneralField, string> = {
    name: "name",
    description: "description",
    identifier: "identifier",
    repoUrl: "repoUrl",
    baseBranch: "baseBranch",
    budget: "agent_default_max_ticks",
  };

  it("addresses every field by its input id", async () => {
    for (const [field, id] of Object.entries(FIELD_IDS) as Array<[SettingsGeneralField, string]>) {
      const stub = generalSettingsPageDouble({ locator: () => new GeneralFakeLocator({ input: "v", disabled: true }) });
      const driver = new WebNewDriver(stub.page);
      await expect(driver.settingsGeneralFieldValue(field)).resolves.toBe("v");
      await expect(driver.settingsGeneralFieldDisabled(field)).resolves.toBe(true);
      await driver.settingsGeneralFill(field, "w");
      expect(stub.calls.locators).toContain(`#${id}`);
    }
  });

  it("reads the inline error or empty when the slot is generalAbsent", async () => {
    const failing = generalSettingsPageDouble({ locator: () => new GeneralFakeLocator({ text: "  Required  " }) });
    await expect(new WebNewDriver(failing.page).settingsGeneralFieldError("name")).resolves.toBe("Required");
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralFieldError("name")).resolves.toBe(
      ""
    );
  });
});

describe("WebNewDriver general tab header reads", () => {
  it("reads the banner, network, timezone and creation stamp", async () => {
    const texts: Record<string, string> = {
      banner: "Forge  FRG Private",
      network: "Private",
      timezone: "UTC",
      created: "Created on Oct 1, 2026",
    };
    const stub = generalSettingsPageDouble({
      locator: (selector) =>
        selector === '[data-testid="general-banner"]'
          ? new GeneralFakeLocator({ text: texts.banner })
          : generalAbsent(),
      testId: (testId) => {
        if (testId === "general-network") return new GeneralFakeLocator({ text: texts.network });
        if (testId === "general-timezone") return new GeneralFakeLocator({ text: texts.timezone });
        if (testId === "general-created") return new GeneralFakeLocator({ text: texts.created });
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsGeneralHeaderText()).resolves.toBe(texts.banner);
    await expect(driver.settingsGeneralNetworkText()).resolves.toBe("Private");
    await expect(driver.settingsGeneralTimezoneText()).resolves.toBe("UTC");
    await expect(driver.settingsGeneralCreatedText()).resolves.toBe("Created on Oct 1, 2026");
  });

  it("reports no cover when the banner carries only the mark", async () => {
    const banner = new GeneralFakeLocator(
      {},
      {
        role: () => generalAbsent(),
        locator: (selector) =>
          selector === 'img:not([data-testid="project-mark-image"])'
            ? new GeneralFakeLocator({ count: 0 })
            : generalAbsent(),
      }
    );
    const clean = generalSettingsPageDouble({
      locator: (selector) => (selector === '[data-testid="general-banner"]' ? banner : generalAbsent()),
    });
    await expect(new WebNewDriver(clean.page).settingsGeneralCoverVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralCoverVisible()).resolves.toBe(false);
  });

  it("reports a cover when a control or a non-mark image renders", async () => {
    const withControl = new GeneralFakeLocator({}, { role: () => new GeneralFakeLocator() });
    const controlStub = generalSettingsPageDouble({
      locator: (selector) => (selector === '[data-testid="general-banner"]' ? withControl : generalAbsent()),
    });
    await expect(new WebNewDriver(controlStub.page).settingsGeneralCoverVisible()).resolves.toBe(true);

    const withImage = new GeneralFakeLocator(
      {},
      { locator: () => new GeneralFakeLocator(), role: () => generalAbsent() }
    );
    const imageStub = generalSettingsPageDouble({
      locator: (selector) => (selector === '[data-testid="general-banner"]' ? withImage : generalAbsent()),
    });
    await expect(new WebNewDriver(imageStub.page).settingsGeneralCoverVisible()).resolves.toBe(true);
  });
});

describe("WebNewDriver general tab execution", () => {
  it("maps the executor label back to its kind", async () => {
    for (const [kind, label] of [
      ["local_runner", "Local runner"],
      ["cloud_agent", "Cloud agent"],
      ["managed_runner", "Desktop agent"],
    ]) {
      const stub = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator({ text: ` ${label} ` }) });
      await expect(new WebNewDriver(stub.page).settingsGeneralExecutorValue()).resolves.toBe(kind);
    }
    const unknown = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator({ text: "Pooled" }) });
    await expect(new WebNewDriver(unknown.page).settingsGeneralExecutorValue()).resolves.toBe("Pooled");
  });

  it("reads option disablement and closes the picker", async () => {
    const option = new GeneralFakeLocator({ disabled: true });
    const listbox = new GeneralFakeLocator({}, { role: () => option });
    const stub = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator(),
      role: (role) => (role === "listbox" ? listbox : generalAbsent()),
    });
    await expect(new WebNewDriver(stub.page).settingsGeneralExecutorOptionDisabled("cloud_agent")).resolves.toBe(true);
    expect(stub.calls.pressed).toEqual(["Escape"]);
  });

  it("sets the executor through the picker option", async () => {
    const option = new GeneralFakeLocator();
    const listbox = new GeneralFakeLocator({}, { role: () => option });
    const trigger = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: (testId) => (testId === "general-executor-trigger" ? trigger : generalAbsent()),
      role: (role) => (role === "listbox" ? listbox : generalAbsent()),
    });
    await new WebNewDriver(stub.page).settingsGeneralSetExecutor("managed_runner");
    expect(trigger.click).toHaveBeenCalledTimes(1);
    expect(option.click).toHaveBeenCalledTimes(1);
  });

  it("reads the ticking switch and flips it only when needed", async () => {
    const on = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator({ attr: { "aria-checked": "true" } }),
    });
    await expect(new WebNewDriver(on.page).settingsGeneralTickingOn()).resolves.toBe(true);
    const off = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator({ attr: { "aria-checked": "false" } }),
    });
    await expect(new WebNewDriver(off.page).settingsGeneralTickingOn()).resolves.toBe(false);

    const toggle = new GeneralFakeLocator({ attr: { "aria-checked": "false" } });
    toggle.click.mockImplementation(async () => {
      toggle.state.attr = { "aria-checked": "true" };
    });
    const flip = generalSettingsPageDouble({ testId: () => toggle });
    await new WebNewDriver(flip.page).settingsGeneralSetTicking(true);
    expect(toggle.click).toHaveBeenCalledTimes(1);

    const settled = new GeneralFakeLocator({ attr: { "aria-checked": "true" } });
    const noop = generalSettingsPageDouble({ testId: () => settled });
    await new WebNewDriver(noop.page).settingsGeneralSetTicking(true);
    expect(settled.click).not.toHaveBeenCalled();
  });

  it("reads the budget and types replacements keystroke by keystroke", async () => {
    const input = new GeneralFakeLocator({ input: "7" });
    const stub = generalSettingsPageDouble({ locator: () => input });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsGeneralBudgetValue()).resolves.toBe("7");
    await driver.settingsGeneralSetBudget("12");
    expect(input.press).toHaveBeenCalledWith("ControlOrMeta+a", { timeout: 30_000 });
    expect(input.pressSequentially).toHaveBeenCalledWith("12", { timeout: 30_000 });
    await driver.settingsGeneralSetBudget("");
    expect(input.press).toHaveBeenCalledWith("Backspace", { timeout: 30_000 });
  });

  it("checks the no-cap box", async () => {
    const box = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: (testId) => (testId === "general-nocap" ? box : generalAbsent()),
    });
    await new WebNewDriver(stub.page).settingsGeneralSetNoCap(true);
    expect(box.setChecked).toHaveBeenCalledWith(true, { timeout: 30_000 });
  });

  it("reads the cadence caption and picks presets by label", async () => {
    const caption = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator({ text: "6 hours" }) });
    await expect(new WebNewDriver(caption.page).settingsGeneralCadenceText()).resolves.toBe("6 hours");

    const option = new GeneralFakeLocator();
    const listbox = new GeneralFakeLocator({}, { role: () => option });
    const trigger = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: (testId) => (testId === "general-cadence-trigger" ? trigger : generalAbsent()),
      role: (role) => (role === "listbox" ? listbox : generalAbsent()),
    });
    await new WebNewDriver(stub.page).settingsGeneralSetCadence("6 hours");
    expect(trigger.click).toHaveBeenCalledTimes(1);
    expect(option.click).toHaveBeenCalledTimes(1);
  });

  it("saves through the submit control and rides out the loading label", async () => {
    const submit = new GeneralFakeLocator();
    const form = new GeneralFakeLocator({}, { role: () => submit });
    const stub = generalSettingsPageDouble({ locator: () => form });
    await new WebNewDriver(stub.page).settingsGeneralSave();
    expect(submit.click).toHaveBeenCalledTimes(1);
    expect(stub.pageObject.waitForTimeout).toHaveBeenCalledWith(500);
  });
});

describe("WebNewDriver general tab repository bind", () => {
  it("reads the bound pill or the bind control", async () => {
    const bound = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator({ disabled: true }) });
    await expect(new WebNewDriver(bound.page).settingsGeneralBindState()).resolves.toBe("bound");
    const enabled = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator({ disabled: false }) });
    await expect(new WebNewDriver(enabled.page).settingsGeneralBindState()).resolves.toBe("bind");
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralBindState()).resolves.toBe("bind");
  });

  it("returns after the client-side refusal when no request fires", async () => {
    const toast = new GeneralFakeLocator({ text: "Bind failed" });
    const stub = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator(),
      locator: (selector) =>
        selector === '[data-testid="toast-notification"]'
          ? new GeneralFakeLocator({ items: [toast] })
          : generalAbsent(),
      response: "reject",
    });
    await new WebNewDriver(stub.page).settingsGeneralBind();
    expect(stub.pageObject.waitForEvent).not.toHaveBeenCalled();
  });

  it("settles on the bound pill after a successful bind", async () => {
    const pill = new GeneralFakeLocator({ disabled: true });
    const stub = generalSettingsPageDouble({
      testId: (testId) => (testId === "general-repo-bound" ? pill : new GeneralFakeLocator()),
      locator: () => new GeneralFakeLocator({ count: 0 }),
      response: "resolve",
      responseStatus: 200,
    });
    await new WebNewDriver(stub.page).settingsGeneralBind();
    expect(stub.pageObject.waitForEvent).not.toHaveBeenCalled();
  });

  it("rides the reload hop on the provider-missing 401", async () => {
    const stub = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator(),
      locator: () => new GeneralFakeLocator(),
      response: "resolve",
      responseStatus: 401,
    });
    await new WebNewDriver(stub.page).settingsGeneralBind();
    expect(stub.pageObject.waitForEvent).toHaveBeenCalled();
    expect(stub.calls.locators).toContain("#name");
  });
});

describe("WebNewDriver general tab emblem", () => {
  it("opens the picker and reports its visibility", async () => {
    const openButton = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: (testId) => {
        if (testId === "general-emblem-open") return openButton;
        if (testId === "general-emblem-picker") return new GeneralFakeLocator();
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsGeneralEmblemOpen();
    expect(openButton.click).toHaveBeenCalledTimes(1);
    await expect(driver.settingsGeneralEmblemPickerVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralEmblemPickerVisible()).resolves.toBe(
      false
    );
  });

  it("picks the alphabet choice matching the glyph", async () => {
    const choice = new GeneralFakeLocator();
    const search = new GeneralFakeLocator();
    const dialog = new GeneralFakeLocator(
      {},
      {
        testId: () => new GeneralFakeLocator(),
        label: () => search,
        role: () => choice,
      }
    );
    const stub = generalSettingsPageDouble({ testId: () => dialog });
    await new WebNewDriver(stub.page).settingsGeneralEmblemPick("🚀");
    expect(search.fill).toHaveBeenCalledWith("🚀", { timeout: 30_000 });
    expect(choice.click).toHaveBeenCalledTimes(1);
  });

  it("falls back to the typed-extra entry outside the alphabet", async () => {
    const typed = new GeneralFakeLocator();
    const dialog = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => (testId === "general-emblem-typed" ? typed : new GeneralFakeLocator()),
        label: () => new GeneralFakeLocator(),
        role: () => generalAbsent(),
      }
    );
    const stub = generalSettingsPageDouble({ testId: () => dialog });
    await new WebNewDriver(stub.page).settingsGeneralEmblemPick("🛸");
    expect(typed.click).toHaveBeenCalledTimes(1);
  });
});

describe("WebNewDriver general tab danger confirmations", () => {
  it("opens the delete dialog from the danger zone", async () => {
    const deleteButton = new GeneralFakeLocator();
    const zone = new GeneralFakeLocator({}, { role: () => deleteButton });
    const dialog = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      locator: (selector) => (selector === '[data-testid="general-danger-zone"]' ? zone : generalAbsent()),
      role: (role) => (role === "dialog" ? dialog : generalAbsent()),
    });
    await new WebNewDriver(stub.page).settingsGeneralOpenDelete();
    expect(deleteButton.click).toHaveBeenCalledTimes(1);
  });

  it("fills the project name scoped to the delete dialog", async () => {
    const nameInput = new GeneralFakeLocator();
    const dialog = new GeneralFakeLocator(
      {},
      { placeholder: (text) => (text === "Project name" ? nameInput : generalAbsent()) }
    );
    const stub = generalSettingsPageDouble({ role: (role) => (role === "dialog" ? dialog : generalAbsent()) });
    await new WebNewDriver(stub.page).settingsGeneralFillDeleteName("Forge");
    expect(nameInput.fill).toHaveBeenCalledWith("Forge", { timeout: 30_000 });
  });

  it("reads the confirm modal text or empty while closed", async () => {
    const dialog = new GeneralFakeLocator({ text: "  Archive Forge?  " });
    const open = generalSettingsPageDouble({ role: (role) => (role === "dialog" ? dialog : generalAbsent()) });
    await expect(new WebNewDriver(open.page).settingsGeneralConfirmModalText()).resolves.toBe("Archive Forge?");
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsGeneralConfirmModalText()).resolves.toBe(
      ""
    );
  });
});

describe("WebNewDriver general tab fetch stalls", () => {
  it("aborts and releases the details-carrying fetches", async () => {
    const stub = generalSettingsPageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.settingsGeneralStallDetails();
    await driver.settingsGeneralReleaseStall();
    expect(stub.routes).toHaveLength(2);
    expect(stub.routes[0]?.action).toBe("route");
    expect(stub.routes[1]?.action).toBe("unroute");
    expect(String(stub.routes[0]?.pattern)).toBe(String(stub.routes[1]?.pattern));
    // Single, bulk and bare-list project reads stall; sub-resources stay live.
    const pattern = stub.routes[0]?.pattern as RegExp;
    expect(pattern.test("/api/workspaces/acme/projects/p1/")).toBe(true);
    expect(pattern.test("/api/workspaces/acme/projects/?ids=p1")).toBe(true);
    expect(pattern.test("/api/workspaces/acme/projects/")).toBe(true);
    expect(pattern.test("/api/workspaces/acme/projects/p1/members/")).toBe(false);
  });
});

describe("WebNewDriver states tab groups", () => {
  function cardDouble(group: string, hooks: { body?: boolean; heads?: string[]; addDisabled?: boolean } = {}) {
    const body = new GeneralFakeLocator({ count: hooks.body === false ? 0 : 1 });
    return new GeneralFakeLocator(
      { attr: { "data-group": group } },
      {
        testId: (testId) => {
          if (testId === "state-group-body") return body;
          if (testId === "state-group-toggle" || testId === "state-group-add") {
            return new GeneralFakeLocator({
              disabled: testId === "state-group-add" ? (hooks.addDisabled ?? false) : false,
            });
          }
          return generalAbsent();
        },
        locator: (selector) => {
          if (selector === "h4") {
            return new GeneralFakeLocator({
              items: (hooks.heads ?? []).map((text) => new GeneralFakeLocator({ text })),
            });
          }
          return generalAbsent();
        },
      }
    );
  }

  it("opens the states address and settles on the cards", async () => {
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator(),
      role: () => new GeneralFakeLocator(),
    });
    await new WebNewDriver(stub.page).settingsStatesOpen("acme", "p1");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1/states");
  });

  it("reads group keys in DOM order, skipping blanks", async () => {
    const cards = new GeneralFakeLocator({
      items: [
        new GeneralFakeLocator({ attr: { "data-group": "backlog" } }),
        new GeneralFakeLocator({ attr: { "data-group": null } }),
        new GeneralFakeLocator({ attr: { "data-group": "unstarted" } }),
      ],
    });
    const stub = generalSettingsPageDouble({ locator: () => cards });
    await expect(new WebNewDriver(stub.page).settingsStatesGroupNames()).resolves.toEqual(["backlog", "unstarted"]);
  });

  it("reads expansion off the card body and toggles it", async () => {
    let expanded = true;
    const toggle = new GeneralFakeLocator();
    toggle.click.mockImplementation(async () => {
      expanded = !expanded;
    });
    const card = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "state-group-body") return new GeneralFakeLocator({ count: expanded ? 1 : 0 });
          if (testId === "state-group-toggle") return toggle;
          return generalAbsent();
        },
      }
    );
    const stub = generalSettingsPageDouble({ locator: () => card });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsStatesGroupExpanded("backlog")).resolves.toBe(true);
    await driver.settingsStatesToggleGroup("backlog");
    await expect(driver.settingsStatesGroupExpanded("backlog")).resolves.toBe(false);
  });

  it("reads row names and the add control state per card", async () => {
    const card = cardDouble("backlog", { heads: ["Backlog", "", "Triage"], addDisabled: false });
    const stub = generalSettingsPageDouble({ locator: () => card });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsStatesStateNames("backlog")).resolves.toEqual(["Backlog", "Triage"]);
    await expect(driver.settingsStatesAddEnabled("backlog")).resolves.toBe(true);
    const locked = cardDouble("backlog", { addDisabled: true });
    const lockedStub = generalSettingsPageDouble({ locator: () => locked });
    await expect(new WebNewDriver(lockedStub.page).settingsStatesAddEnabled("backlog")).resolves.toBe(false);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesAddEnabled("x")).resolves.toBe(false);
  });

  it("opens creation and reports the form", async () => {
    const add = new GeneralFakeLocator();
    const card = new GeneralFakeLocator(
      {},
      { testId: (testId) => (testId === "state-group-add" ? add : generalAbsent()) }
    );
    const stub = generalSettingsPageDouble({
      locator: () => card,
      testId: (testId) => (testId === "state-form-name" ? new GeneralFakeLocator() : generalAbsent()),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesOpenCreate("backlog");
    expect(add.click).toHaveBeenCalledTimes(1);
    await expect(driver.settingsStatesFormVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesFormVisible()).resolves.toBe(false);
  });
});

describe("WebNewDriver states tab form", () => {
  it("fills and reads the name and description", async () => {
    const name = new GeneralFakeLocator({ input: "  Beta  " });
    const description = new GeneralFakeLocator({ input: "blurb" });
    const stub = generalSettingsPageDouble({
      testId: (testId) => {
        if (testId === "state-form-name") return name;
        if (testId === "state-form-description") return description;
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesFillName("Gamma");
    await driver.settingsStatesFillDescription("new blurb");
    expect(name.fill).toHaveBeenCalledWith("Gamma", { timeout: 30_000 });
    expect(description.fill).toHaveBeenCalledWith("new blurb", { timeout: 30_000 });
    await expect(driver.settingsStatesNameValue()).resolves.toBe("Beta");
    await expect(driver.settingsStatesDescriptionValue()).resolves.toBe("blurb");
    const empty = generalSettingsPageDouble();
    await expect(new WebNewDriver(empty.page).settingsStatesNameValue()).resolves.toBe("");
    await expect(new WebNewDriver(empty.page).settingsStatesDescriptionValue()).resolves.toBe("");
  });

  it("reads the inline form error or empty", async () => {
    const alert = new GeneralFakeLocator({ text: "Name is taken" });
    const form = new GeneralFakeLocator({}, { role: () => alert });
    const stub = generalSettingsPageDouble({ testId: () => form });
    await expect(new WebNewDriver(stub.page).settingsStatesFormError()).resolves.toBe("Name is taken");
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesFormError()).resolves.toBe("");
  });

  it("submits, cancels and attempts through the form buttons", async () => {
    const create = new GeneralFakeLocator();
    const update = new GeneralFakeLocator();
    const cancel = new GeneralFakeLocator();
    const form = new GeneralFakeLocator(
      {},
      {
        role: (_role, options) => {
          const name = String(options?.name ?? "");
          if (name === "Create state") return create;
          if (name === "Save changes") return update;
          if (name === "Cancel") return cancel;
          return generalAbsent();
        },
      }
    );
    const stub = generalSettingsPageDouble({
      testId: () => form,
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesSubmitCreate();
    await driver.settingsStatesSubmitUpdate();
    await driver.settingsStatesCancelForm();
    await driver.settingsStatesAttemptSubmit();
    expect(create.click).toHaveBeenCalledTimes(2);
    expect(update.click).toHaveBeenCalledTimes(1);
    expect(cancel.click).toHaveBeenCalledTimes(1);
  });

  it("attempts the update button when no create button renders", async () => {
    const update = new GeneralFakeLocator();
    const form = new GeneralFakeLocator(
      {},
      {
        role: (_role, options) => (String(options?.name ?? "") === "Create state" ? generalAbsent() : update),
      }
    );
    const stub = generalSettingsPageDouble({ testId: () => form });
    await new WebNewDriver(stub.page).settingsStatesAttemptSubmit();
    expect(update.click).toHaveBeenCalledTimes(1);
  });

  it("reads the swatch color and picks palette members", async () => {
    const swatch = new GeneralFakeLocator();
    swatch.evaluate.mockResolvedValue("rgb(255, 105, 0)");
    const form = new GeneralFakeLocator(
      {},
      { testId: (testId) => (testId === "state-form-swatch" ? swatch : generalAbsent()) }
    );
    const palette = new GeneralFakeLocator(
      {},
      {
        testId: () =>
          new GeneralFakeLocator({
            items: [
              new GeneralFakeLocator({ attr: { "data-color": "#9CA3AF" } }),
              new GeneralFakeLocator({ attr: { "data-color": "#3B82F6" } }),
            ],
          }),
      }
    );
    const stub = generalSettingsPageDouble({
      testId: () => form,
      role: (role) => (role === "group" ? palette : generalAbsent()),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsStatesDotColor()).resolves.toBe("rgb(255, 105, 0)");
    await driver.settingsStatesOpenColorPicker();
    await driver.settingsStatesPickColor("#3b82f6");
    await expect(driver.settingsStatesPickColor("#0693E3")).rejects.toThrow('no swatch matching "#0693E3"');
  });
});

describe("WebNewDriver states tab rows", () => {
  function rowDouble(overrides: { grip?: boolean; gripVisible?: boolean; marker?: boolean } = {}) {
    const grip =
      overrides.grip === false ? generalAbsent() : new GeneralFakeLocator({ visible: overrides.gripVisible ?? true });
    return new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "state-row-grip") return grip;
          if (testId === "state-promote") return new GeneralFakeLocator();
          if (testId === "state-default-marker") {
            return overrides.marker === true ? new GeneralFakeLocator() : generalAbsent();
          }
          return generalAbsent();
        },
        role: () => new GeneralFakeLocator({ disabled: false }),
      }
    );
  }

  it("opens the editor and reports the grip", async () => {
    const row = rowDouble();
    const stub = generalSettingsPageDouble({
      locator: () => row,
      testId: () => new GeneralFakeLocator(),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesOpenEdit("Beta");
    await expect(driver.settingsStatesGripVisible("Beta")).resolves.toBe(true);
    expect(stub.calls.locators[0]).toContain('data-state-name="Beta"');

    const quoted = generalSettingsPageDouble({ locator: () => rowDouble() });
    await new WebNewDriver(quoted.page).settingsStatesGripVisible('Qu"ote\\x');
    expect(quoted.calls.locators[0]).toContain('data-state-name="Qu\\"ote\\\\x"');
  });

  it("reports no grip when the control is missing or hidden", async () => {
    const missing = generalSettingsPageDouble({ locator: () => rowDouble({ grip: false }) });
    await expect(new WebNewDriver(missing.page).settingsStatesGripVisible("Solo")).resolves.toBe(false);
    const hidden = generalSettingsPageDouble({ locator: () => rowDouble({ gripVisible: false }) });
    await expect(new WebNewDriver(hidden.page).settingsStatesGripVisible("Solo")).resolves.toBe(false);
  });

  it("drags the grip onto the target edge", async () => {
    const grip = new GeneralFakeLocator();
    const row = new GeneralFakeLocator(
      {},
      { testId: (testId) => (testId === "state-row-grip" ? grip : generalAbsent()) }
    );
    const target = new GeneralFakeLocator();
    target.boundingBox.mockResolvedValue({ x: 0, y: 0, width: 100, height: 40 });
    let calls = 0;
    const stub = generalSettingsPageDouble({
      locator: () => {
        calls += 1;
        return calls % 2 === 1 ? row : target;
      },
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesDragAbove("one", "two");
    expect(grip.dragTo).toHaveBeenCalledWith(target, { targetPosition: { x: 50, y: 6 } });
    await driver.settingsStatesDragBelow("one", "two");
    expect(grip.dragTo).toHaveBeenCalledWith(target, { targetPosition: { x: 50, y: 34 } });
  });

  it("marks the default and reads the marker", async () => {
    const promote = new GeneralFakeLocator();
    const row = new GeneralFakeLocator(
      {},
      { testId: (testId) => (testId === "state-promote" ? promote : generalAbsent()) }
    );
    const stub = generalSettingsPageDouble({ locator: () => row });
    await new WebNewDriver(stub.page).settingsStatesMarkDefault("Beta");
    expect(promote.click).toHaveBeenCalledTimes(1);

    const marked = generalSettingsPageDouble({ locator: () => rowDouble({ marker: true }) });
    await expect(new WebNewDriver(marked.page).settingsStatesIsDefault("Beta")).resolves.toBe(true);
    const plain = generalSettingsPageDouble({ locator: () => rowDouble() });
    await expect(new WebNewDriver(plain.page).settingsStatesIsDefault("Beta")).resolves.toBe(false);
  });
});

describe("WebNewDriver states tab delete", () => {
  function deleteRowDouble(disabled: boolean) {
    const button = new GeneralFakeLocator({ disabled });
    return new GeneralFakeLocator({}, { role: () => button });
  }

  it("reads disablement and opens the dialog", async () => {
    const stub = generalSettingsPageDouble({
      locator: () => deleteRowDouble(true),
      testId: () => new GeneralFakeLocator(),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsStatesDeleteDisabled("Beta")).resolves.toBe(true);
    await driver.settingsStatesDelete("Beta");
  });

  it("reads the modal text or empty while closed", async () => {
    const message = new GeneralFakeLocator({ text: "  Beta\nwill be removed  " });
    const stub = generalSettingsPageDouble({ testId: () => message });
    await expect(new WebNewDriver(stub.page).settingsStatesDeleteModalText()).resolves.toBe("Beta will be removed");
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesDeleteModalText()).resolves.toBe("");
  });

  it("confirms, cancels and attempts deletion", async () => {
    const confirm = new GeneralFakeLocator();
    const cancel = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator(),
      role: (_role, options) => {
        const name = String(options?.name ?? "");
        if (name === "Delete state") return confirm;
        if (name === "Cancel") return cancel;
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsStatesDeleteConfirm();
    await driver.settingsStatesDeleteCancel();
    await driver.settingsStatesAttemptDeleteConfirm();
    expect(confirm.click).toHaveBeenCalledTimes(2);
    expect(cancel.click).toHaveBeenCalledTimes(1);
  });
});

describe("WebNewDriver states tab toggle, skeleton and notes", () => {
  it("reads and flips the member toggle", async () => {
    const visible = generalSettingsPageDouble({ testId: () => new GeneralFakeLocator() });
    await expect(new WebNewDriver(visible.page).settingsStatesMemberToggleVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesMemberToggleVisible()).resolves.toBe(
      false
    );

    const on = generalSettingsPageDouble({
      testId: () => new GeneralFakeLocator({ attr: { "aria-checked": "true" } }),
    });
    await expect(new WebNewDriver(on.page).settingsStatesMemberToggleOn()).resolves.toBe(true);

    const toggle = new GeneralFakeLocator({ attr: { "aria-checked": "false" } });
    const stub = generalSettingsPageDouble({ testId: () => toggle });
    await new WebNewDriver(stub.page).settingsStatesSetMemberToggle(true);
    expect(toggle.click).toHaveBeenCalledTimes(1);
    const settled = new GeneralFakeLocator({ attr: { "aria-checked": "true" } });
    const noop = generalSettingsPageDouble({ testId: () => settled });
    await new WebNewDriver(noop.page).settingsStatesSetMemberToggle(true);
    expect(settled.click).not.toHaveBeenCalled();
  });

  it("denies outsiders on either heading", async () => {
    const denied = generalSettingsPageDouble({ role: () => new GeneralFakeLocator() });
    await expect(new WebNewDriver(denied.page).settingsStatesAccessDenied()).resolves.toBe(true);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesAccessDenied()).resolves.toBe(false);
  });

  it("shows the skeleton only with four bones up", async () => {
    const bones = new GeneralFakeLocator({
      items: [new GeneralFakeLocator(), new GeneralFakeLocator(), new GeneralFakeLocator(), new GeneralFakeLocator()],
    });
    const skeleton = new GeneralFakeLocator({}, { locator: () => bones });
    const stub = generalSettingsPageDouble({ testId: () => skeleton });
    await expect(new WebNewDriver(stub.page).settingsStatesSkeletonVisible()).resolves.toBe(true);
    const thin = new GeneralFakeLocator({}, { locator: () => new GeneralFakeLocator({ count: 2 }) });
    const thinStub = generalSettingsPageDouble({ testId: () => thin });
    await expect(new WebNewDriver(thinStub.page).settingsStatesSkeletonVisible()).resolves.toBe(false);
  });

  it("holds list reads and reads the empty note", async () => {
    const stub = generalSettingsPageDouble();
    await new WebNewDriver(stub.page).settingsStatesDelayLoad(5);
    expect(stub.pageObject.route).toHaveBeenCalledTimes(1);
    const hold = stub.pageObject.route.mock.calls[0]?.[1] as (route: {
      request: () => { method: () => string };
      continue: () => Promise<void>;
    }) => Promise<void>;
    const getRoute = { request: () => ({ method: () => "GET" }), continue: vi.fn(async () => undefined) };
    await hold(getRoute);
    expect(getRoute.continue).toHaveBeenCalledTimes(1);
    const postRoute = { request: () => ({ method: () => "POST" }), continue: vi.fn(async () => undefined) };
    await hold(postRoute);
    expect(postRoute.continue).toHaveBeenCalledTimes(1);

    const note = new GeneralFakeLocator({ text: "No states\navailable" });
    const card = new GeneralFakeLocator({}, { testId: () => note });
    const noteStub = generalSettingsPageDouble({ locator: () => card });
    await expect(new WebNewDriver(noteStub.page).settingsStatesEmptyNote("cancelled")).resolves.toBe(
      "No states available"
    );
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsStatesEmptyNote("x")).resolves.toBe("");
  });
});

describe("WebNewDriver labels tab list and form", () => {
  function labelRowDouble(name: string, group: boolean) {
    const nameSpan = new GeneralFakeLocator({ text: name });
    return new GeneralFakeLocator(
      { attr: group ? { "data-group": "true" } : {} },
      {
        testId: (testId) => {
          if (testId === "label-row-name") return new GeneralFakeLocator({ items: [nameSpan] });
          return new GeneralFakeLocator();
        },
      }
    );
  }

  it("opens the labels address", async () => {
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator(),
      role: () => new GeneralFakeLocator(),
    });
    await new WebNewDriver(stub.page).settingsLabelsOpen("acme", "p1");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1/labels");
  });

  it("reads names in DOM order and matches rows exactly", async () => {
    const rows = new GeneralFakeLocator({
      items: [labelRowDouble("beta", false), labelRowDouble("alphabet", false)],
    });
    const stub = generalSettingsPageDouble({ locator: () => rows });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsLabelsIsGroup("alphabet")).resolves.toBe(false);
    await expect(driver.settingsLabelsIsGroup("missing")).rejects.toThrow('label row "missing" not found');

    const names = new GeneralFakeLocator({
      items: [new GeneralFakeLocator({ text: " beta " }), new GeneralFakeLocator({ text: "" })],
    });
    const namesStub = generalSettingsPageDouble({ testId: () => names });
    await expect(new WebNewDriver(namesStub.page).settingsLabelsNames()).resolves.toEqual(["beta"]);
  });

  it("reads group membership off the row marker", async () => {
    const rows = new GeneralFakeLocator({ items: [labelRowDouble("parent", true)] });
    const stub = generalSettingsPageDouble({ locator: () => rows });
    await expect(new WebNewDriver(stub.page).settingsLabelsIsGroup("parent")).resolves.toBe(true);
  });

  it("opens creation and reports the form", async () => {
    const add = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      testId: (testId) => {
        if (testId === "label-add") return add;
        if (testId === "label-form-name") return new GeneralFakeLocator();
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsLabelsAddVisible()).resolves.toBe(true);
    await driver.settingsLabelsOpenCreate();
    expect(add.click).toHaveBeenCalledTimes(1);
    await expect(driver.settingsLabelsFormVisible()).resolves.toBe(true);
    const bare = generalSettingsPageDouble();
    await expect(new WebNewDriver(bare.page).settingsLabelsAddVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(bare.page).settingsLabelsFormVisible()).resolves.toBe(false);
  });

  it("fills the name, reads values and errors, and submits", async () => {
    const name = new GeneralFakeLocator({ input: "beta" });
    const create = new GeneralFakeLocator();
    const update = new GeneralFakeLocator();
    const cancel = new GeneralFakeLocator();
    const form = new GeneralFakeLocator(
      {},
      {
        role: (_role, options) => {
          const label = String(options?.name ?? "");
          if (label === "Create label") return create;
          if (label === "Save changes") return update;
          if (label === "Cancel") return cancel;
          return new GeneralFakeLocator({ text: "Name is taken" });
        },
      }
    );
    const stub = generalSettingsPageDouble({
      testId: (testId) => {
        if (testId === "label-form-name") return name;
        if (testId === "label-form") return form;
        return generalAbsent();
      },
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsLabelsFillName("gamma");
    expect(name.fill).toHaveBeenCalledWith("gamma", { timeout: 30_000 });
    await expect(driver.settingsLabelsNameValue()).resolves.toBe("beta");
    await expect(driver.settingsLabelsFormError()).resolves.toBe("Name is taken");
    await driver.settingsLabelsSubmitCreate();
    await driver.settingsLabelsSubmitUpdate();
    await driver.settingsLabelsCancelForm();
    await driver.settingsLabelsAttemptSubmit();
    expect(create.click).toHaveBeenCalledTimes(1);
    expect(update.click).toHaveBeenCalledTimes(2);
    expect(cancel.click).toHaveBeenCalledTimes(1);
  });

  it("attempts creation when no update button renders and reads no error", async () => {
    const create = new GeneralFakeLocator();
    const form = new GeneralFakeLocator(
      {},
      {
        role: (_role, options) => (String(options?.name ?? "") === "Save changes" ? generalAbsent() : create),
      }
    );
    const plainForm = new GeneralFakeLocator({}, { role: () => generalAbsent() });
    const stub = generalSettingsPageDouble({
      testId: (testId) => {
        if (testId === "label-form") return form;
        return generalAbsent();
      },
    });
    await new WebNewDriver(stub.page).settingsLabelsAttemptSubmit();
    expect(create.click).toHaveBeenCalledTimes(1);
    const clean = generalSettingsPageDouble({ testId: () => plainForm });
    await expect(new WebNewDriver(clean.page).settingsLabelsFormError()).resolves.toBe("");
  });

  it("reads the dot color and picks palette members", async () => {
    const dot = new GeneralFakeLocator();
    dot.evaluate.mockResolvedValue("rgb(255, 105, 0)");
    const form = new GeneralFakeLocator(
      {},
      { testId: (testId) => (testId === "label-form-dot" ? dot : generalAbsent()) }
    );
    const palette = new GeneralFakeLocator(
      {},
      {
        testId: () =>
          new GeneralFakeLocator({
            items: [
              new GeneralFakeLocator({ attr: { "data-color": "#FF6900" } }),
              new GeneralFakeLocator({ attr: { "data-color": "#0693E3" } }),
            ],
          }),
      }
    );
    const stub = generalSettingsPageDouble({
      testId: () => form,
      role: (role) => (role === "group" ? palette : generalAbsent()),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsLabelsDotColor()).resolves.toBe("rgb(255, 105, 0)");
    await driver.settingsLabelsOpenColorPicker();
    await driver.settingsLabelsPickColor("#0693e3");
    await expect(driver.settingsLabelsPickColor("#123456")).rejects.toThrow('no swatch matching "#123456"');
  });
});

describe("WebNewDriver labels tab rows and menus", () => {
  function menuRowDouble() {
    const nameSpan = new GeneralFakeLocator({ text: "beta" });
    return new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name") return new GeneralFakeLocator({ items: [nameSpan] });
          return new GeneralFakeLocator();
        },
      }
    );
  }

  it("opens the row menu and reads its items", async () => {
    const menu = new GeneralFakeLocator();
    const row = menuRowDouble();
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator({ items: [row] }),
      testId: () => menu,
      role: (role) =>
        role === "menuitem"
          ? new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "Edit label" })] })
          : generalAbsent(),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsLabelsOpenRowMenu("beta");
    await expect(driver.settingsLabelsMenuItems()).resolves.toEqual(["Edit label"]);
  });

  it("picks menu entries by escaped text", async () => {
    const entry = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({ role: () => entry });
    await new WebNewDriver(stub.page).settingsLabelsMenuPick("Remove from group (2)");
    expect(entry.click).toHaveBeenCalledTimes(1);
  });

  it("deletes through the trash control and the dialog", async () => {
    const trash = new GeneralFakeLocator();
    const nameSpan = new GeneralFakeLocator({ text: "beta" });
    const row = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name") return new GeneralFakeLocator({ items: [nameSpan] });
          if (testId === "label-delete") return trash;
          return new GeneralFakeLocator();
        },
      }
    );
    const dialog = new GeneralFakeLocator({ text: "  Delete label\nbeta will go  " });
    const confirm = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator({ items: [row] }),
      testId: (testId) => (testId === "label-delete" ? trash : new GeneralFakeLocator()),
      role: (role) => (role === "dialog" ? dialog : confirm),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsLabelsDeleteViaTrash("beta");
    expect(trash.click).toHaveBeenCalledTimes(1);
    await expect(driver.settingsLabelsDeleteModalText()).resolves.toBe("Delete label beta will go");
    await driver.settingsLabelsDeleteConfirm();
    await driver.settingsLabelsDeleteCancel();
    await driver.settingsLabelsAttemptDeleteConfirm();
    await driver.settingsLabelsDeleteAttemptConfirm();
    expect(confirm.click).toHaveBeenCalledTimes(4);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsLabelsDeleteModalText()).resolves.toBe("");
  });

  it("denies guests on the heading", async () => {
    const denied = generalSettingsPageDouble({ role: () => new GeneralFakeLocator() });
    await expect(new WebNewDriver(denied.page).settingsLabelsAccessDenied()).resolves.toBe(true);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsLabelsAccessDenied()).resolves.toBe(false);
  });
});

describe("WebNewDriver labels tab groups and drag", () => {
  function groupRowDouble(expanded: { value: boolean }) {
    const nameSpan = new GeneralFakeLocator({ text: "parent" });
    const toggle = new GeneralFakeLocator({
      attr: { "aria-expanded": expanded.value ? "true" : "false" },
    });
    toggle.click.mockImplementation(async () => {
      expanded.value = !expanded.value;
      toggle.state.attr = { "aria-expanded": expanded.value ? "true" : "false" };
    });
    return new GeneralFakeLocator(
      { attr: { "data-group": "true" } },
      {
        testId: (testId) => {
          if (testId === "label-row-name") return new GeneralFakeLocator({ items: [nameSpan] });
          if (testId === "label-group-toggle") return toggle;
          if (testId === "label-drag-handle") return new GeneralFakeLocator();
          return generalAbsent();
        },
        locator: (selector) =>
          selector === "ul [data-testid='label-row-name']"
            ? new GeneralFakeLocator({
                items: [new GeneralFakeLocator({ text: "child" }), new GeneralFakeLocator({ text: "" })],
              })
            : generalAbsent(),
      }
    );
  }

  it("reads children, toggles groups and reports collapse", async () => {
    const expanded = { value: true };
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator({ items: [groupRowDouble(expanded)] }),
    });
    // The toggle lookup re-resolves the row per call, so each double
    // needs its own expansion cell; share one through the stub.
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsLabelsChildrenOf("parent")).resolves.toEqual(["child"]);
    await expect(driver.settingsLabelsGroupCollapsed("parent")).resolves.toBe(false);
    await driver.settingsLabelsToggleGroup("parent");
    expect(expanded.value).toBe(false);
  });

  it("reports the grip only when it renders visibly", async () => {
    const shown = generalSettingsPageDouble({
      locator: () =>
        new GeneralFakeLocator({
          items: [
            new GeneralFakeLocator(
              {},
              {
                testId: (testId) => {
                  if (testId === "label-row-name")
                    return new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "beta" })] });
                  if (testId === "label-drag-handle") return new GeneralFakeLocator();
                  return generalAbsent();
                },
              }
            ),
          ],
        }),
    });
    await expect(new WebNewDriver(shown.page).settingsLabelsGripVisible("beta")).resolves.toBe(true);
    const hidden = generalSettingsPageDouble({ locator: () => new GeneralFakeLocator({ items: [] }) });
    await expect(new WebNewDriver(hidden.page).settingsLabelsGripVisible("beta")).rejects.toThrow(
      'label row "beta" not found'
    );
  });

  it("drags the handle onto targets and above them", async () => {
    const handle = new GeneralFakeLocator();
    handle.boundingBox.mockResolvedValue({ x: 0, y: 0, width: 10, height: 10 });
    const sourceRow = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name")
            return new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "one" })] });
          if (testId === "label-drag-handle") return handle;
          return generalAbsent();
        },
      }
    );
    const targetRow = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name")
            return new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "target-row" })] });
          return generalAbsent();
        },
      }
    );
    targetRow.boundingBox.mockResolvedValue({ x: 100, y: 100, width: 200, height: 40 });
    const stub = generalSettingsPageDouble({
      locator: () => new GeneralFakeLocator({ items: [sourceRow, targetRow] }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.settingsLabelsDragOnto("one", "target-row");
    expect(stub.pageObject.mouse.down).toHaveBeenCalledTimes(1);
    expect(stub.pageObject.mouse.up).toHaveBeenCalledTimes(1);
    expect(stub.pageObject.mouse.move).toHaveBeenCalledTimes(13);
    await driver.settingsLabelsDragAbove("one", "target-row");
    expect(stub.pageObject.mouse.down).toHaveBeenCalledTimes(2);
  });

  it("throws when the drag endpoints have no boxes", async () => {
    const row = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name")
            return new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "one" })] });
          return new GeneralFakeLocator();
        },
      }
    );
    const stub = generalSettingsPageDouble({ locator: () => new GeneralFakeLocator({ items: [row, row] }) });
    await expect(new WebNewDriver(stub.page).settingsLabelsDragOnto("one", "one")).rejects.toThrow("no boxes");
  });

  it("samples the drop feedback mid-drag", async () => {
    const handle = new GeneralFakeLocator();
    handle.boundingBox.mockResolvedValue({ x: 0, y: 0, width: 10, height: 10 });
    const row = new GeneralFakeLocator(
      {},
      {
        testId: (testId) => {
          if (testId === "label-row-name")
            return new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "one" })] });
          if (testId === "label-drag-handle") return handle;
          return new GeneralFakeLocator();
        },
      }
    );
    const target = new GeneralFakeLocator(
      {},
      {
        testId: (testId) =>
          testId === "label-row-name"
            ? new GeneralFakeLocator({ items: [new GeneralFakeLocator({ text: "two" })] })
            : new GeneralFakeLocator(),
      }
    );
    target.boundingBox.mockResolvedValue({ x: 0, y: 0, width: 10, height: 10 });
    const stub = generalSettingsPageDouble({
      locator: (selector) =>
        selector === '[data-testid="label-row"]' ? new GeneralFakeLocator({ items: [row, target] }) : target,
    });
    stub.pageObject.evaluate.mockResolvedValue("ring");
    const driver = new WebNewDriver(stub.page);
    // Both rows resolve through the same list double: the source matches
    // "one" while the target lookup falls through to the second row.
    await expect(driver.settingsLabelsDragFeedback("one", "two", "nest")).resolves.toBe("ring");
    expect(stub.pageObject.mouse.up).toHaveBeenCalledTimes(1);
  });

  it("reads scrollability, the empty state and the skeleton", async () => {
    const scrollable = generalSettingsPageDouble();
    scrollable.pageObject.evaluate.mockResolvedValue(true);
    await expect(new WebNewDriver(scrollable.page).settingsLabelsListScrollable()).resolves.toBe(true);

    const title = new GeneralFakeLocator({ text: "  No labels yet  " });
    const action = new GeneralFakeLocator();
    const stub = generalSettingsPageDouble({
      text: () => title,
      role: () => action,
      testId: () => new GeneralFakeLocator(),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.settingsLabelsEmptyTitle()).resolves.toBe("No labels yet");
    await driver.settingsLabelsEmptyAction();
    expect(action.click).toHaveBeenCalledTimes(1);
    await expect(new WebNewDriver(generalSettingsPageDouble().page).settingsLabelsEmptyTitle()).resolves.toBe("");

    const bones = new GeneralFakeLocator({
      items: [new GeneralFakeLocator(), new GeneralFakeLocator(), new GeneralFakeLocator(), new GeneralFakeLocator()],
    });
    const skeleton = new GeneralFakeLocator({}, { locator: () => bones });
    const skeletonStub = generalSettingsPageDouble({ testId: () => skeleton });
    await expect(new WebNewDriver(skeletonStub.page).settingsLabelsSkeletonVisible()).resolves.toBe(true);
    const thin = new GeneralFakeLocator({}, { locator: () => new GeneralFakeLocator({ count: 2 }) });
    const thinStub = generalSettingsPageDouble({ testId: () => thin });
    await expect(new WebNewDriver(thinStub.page).settingsLabelsSkeletonVisible()).resolves.toBe(false);
  });

  it("holds list reads across both label endpoints", async () => {
    const stub = generalSettingsPageDouble();
    await new WebNewDriver(stub.page).settingsLabelsDelayLoad(5);
    expect(stub.pageObject.route).toHaveBeenCalledTimes(2);
    expect(stub.routes.map((entry) => entry.pattern)).toEqual(["**/issue-labels/*", "**/api/workspaces/*/labels/"]);
  });
});
