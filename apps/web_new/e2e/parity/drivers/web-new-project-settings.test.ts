// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver project feature-flag and automations
// methods (NEWFRONT-582, PSET-043–048, 053, 054). A small Locator/Page
// double proves the selectors, read/action branches and the project-write
// waiters without bringing up the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

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
