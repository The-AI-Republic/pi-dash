// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver automations methods (NEWFRONT-890,
// AGT-034–037). A small Locator/Page double proves each selector, read and
// action branch against the shape of the shipped automations page without
// bringing up the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => vi.restoreAllMocks());

const ARCHIVE = "Auto-archive closed work items";
const CLOSE = "Auto-close work items";

interface LocatorOptions {
  count?: number;
  visible?: boolean;
  text?: string;
  input?: string;
  checked?: boolean;
  disabled?: boolean;
  waitForRejects?: boolean;
  attributes?: Record<string, string | null>;
  attributeRejects?: boolean;
  disabledRejects?: boolean;
  textRejects?: boolean;
  inputRejects?: boolean;
  items?: LocatorDouble[];
  role?: (role: string, options?: { name?: string | RegExp; exact?: boolean; level?: number }) => LocatorDouble;
  textLocator?: (text: string | RegExp, options?: { exact?: boolean }) => LocatorDouble;
  placeholder?: (placeholder: string) => LocatorDouble;
  testId?: (testId: string) => LocatorDouble;
  locator?: (selector: string) => LocatorDouble;
  first?: LocatorDouble;
  last?: LocatorDouble;
}

class LocatorDouble {
  readonly click = vi.fn(async () => undefined);
  readonly fill = vi.fn(async (_value: string) => undefined);
  readonly waitFor = vi.fn(async (_options?: unknown) => {
    if (this.options.waitForRejects === true) {
      // Playwright spends the whole timeout on an absent locator before it
      // rejects, so a raced visible locator wins. Keep that order.
      await new Promise((resolve) => setTimeout(resolve, 5));
      throw new Error("timed out");
    }
  });
  private readonly value: string;

  constructor(readonly options: LocatorOptions = {}) {
    this.value = options.input ?? "";
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.items?.length ?? this.options.count ?? 1);
  }

  isVisible(): Promise<boolean> {
    return Promise.resolve(this.options.visible ?? true);
  }

  isDisabled(): Promise<boolean> {
    if (this.options.disabledRejects === true) return Promise.reject(new Error("detached"));
    return Promise.resolve(this.options.disabled ?? false);
  }

  getAttribute(name: string): Promise<string | null> {
    if (this.options.attributeRejects === true) return Promise.reject(new Error("detached"));
    return Promise.resolve(this.options.attributes?.[name] ?? null);
  }

  innerText(): Promise<string> {
    if (this.options.textRejects === true) return Promise.reject(new Error("detached"));
    return Promise.resolve(this.options.text ?? "");
  }

  inputValue(): Promise<string> {
    if (this.options.inputRejects === true) return Promise.reject(new Error("detached"));
    return Promise.resolve(this.value);
  }

  first(): LocatorDouble {
    return this.options.first ?? this.options.items?.[0] ?? this;
  }

  last(): LocatorDouble {
    return this.options.last ?? this.options.items?.at(-1) ?? this;
  }

  nth(index: number): LocatorDouble {
    return this.options.items?.[index] ?? absent();
  }

  filter(_options: unknown): LocatorDouble {
    return this;
  }

  getByRole(role: string, options?: { name?: string | RegExp; exact?: boolean; level?: number }): LocatorDouble {
    return this.options.role?.(role, options) ?? absent();
  }

  getByText(text: string | RegExp, options?: { exact?: boolean }): LocatorDouble {
    return this.options.textLocator?.(text, options) ?? absent();
  }

  getByPlaceholder(placeholder: string): LocatorDouble {
    return this.options.placeholder?.(placeholder) ?? absent();
  }

  getByTestId(testId: string): LocatorDouble {
    return this.options.testId?.(testId) ?? absent();
  }

  locator(selector: string): LocatorDouble {
    return this.options.locator?.(selector) ?? absent();
  }
}

function absent(): LocatorDouble {
  return new LocatorDouble({ count: 0, visible: false, waitForRejects: true });
}

interface MonthModalOptions {
  open?: boolean;
  input?: string;
  inputRejects?: boolean;
  error?: boolean;
}

interface PageOptions {
  /** How the page settles on open: the rows, the refusal, or neither. */
  settles?: "rows" | "refusal" | "neither";
  refusalVisible?: boolean;
  /** Archive switch state and readability. */
  archive?: LocatorOptions;
  /** Close switch state and readability. */
  close?: LocatorOptions;
  closePanelVisible?: boolean;
  closeStateText?: string;
  closeStateDisabled?: boolean;
  closeStateRejects?: boolean;
  delayTriggerVisible?: boolean;
  delayTriggerText?: string;
  options?: string[];
  builtInRows?: string[];
  extension?: "absent" | "empty" | "rows";
  month?: MonthModalOptions;
}

function pageDouble(options: PageOptions = {}) {
  const month: MonthModalOptions = options.month ?? {};
  const monthOpen = month.open ?? false;
  const monthInput = new LocatorDouble({
    count: month.open === false ? 0 : 1,
    input: month.input ?? "",
    inputRejects: month.inputRejects,
  });
  const monthError = new LocatorDouble({
    count: month.error === true ? 1 : 0,
    text: "Select a month between 1 and 12.",
  });
  const submit = new LocatorDouble();
  const cancel = new LocatorDouble();
  const dialog = new LocatorDouble({
    count: monthOpen ? 1 : 0,
    visible: monthOpen,
    placeholder: (placeholder) => (placeholder === "Enter Months" ? monthInput : absent()),
    role: (role, roleOptions) => {
      if (role !== "button") return absent();
      if (roleOptions?.name === "Submit") return submit;
      if (roleOptions?.name === "Cancel") return cancel;
      return absent();
    },
    textLocator: (text) => (String(text) === "Select a month between 1 and 12." ? monthError : absent()),
  });

  const archiveSwitch = new LocatorDouble(options.archive ?? {});
  const closeSwitch = new LocatorDouble(options.close ?? {});
  const delayTrigger = new LocatorDouble({
    count: options.delayTriggerVisible === false ? 0 : 1,
    visible: options.delayTriggerVisible !== false,
    text: options.delayTriggerText ?? "",
  });
  const closeState = new LocatorDouble({
    count: options.closePanelVisible === false ? 0 : 1,
    visible: options.closePanelVisible !== false,
    text: options.closeStateText ?? "",
    disabled: options.closeStateDisabled ?? false,
    disabledRejects: options.closeStateRejects,
  });

  /** Names of the options the driver clicked, in order. */
  const optionClicks: string[] = [];
  const optionDoubles = (options.options ?? []).map((text) => {
    const double = new LocatorDouble({ text });
    double.click.mockImplementation(async () => {
      optionClicks.push(text);
    });
    return double;
  });
  // A name filter narrows the list the way Playwright's option locator does;
  // an unfiltered read returns every option in order.
  const optionList = new LocatorDouble({
    items: optionDoubles,
    role: (role, roleOptions) => {
      if (role !== "option") return absent();
      const name = roleOptions?.name;
      // With a name the list narrows to the matching option; without one it
      // answers the full list, as the page-level `getByRole("option")` does.
      if (name === undefined) return new LocatorDouble({ items: optionDoubles });
      if (typeof name !== "string") return absent();
      return optionDoubles.find((option) => option.options.text === name) ?? absent();
    },
  });

  const rowFor = (title: string, control: LocatorDouble) => {
    const row = {
      first: () => row,
      getByRole: (role: string, roleOptions?: { name?: string | RegExp }) => {
        if (role === "switch" && roleOptions?.name === title) return control;
        if (role === "button" && roleOptions?.name instanceof RegExp) return delayTrigger;
        return absent();
      },
      locator: (selector: string) => (selector === 'button[aria-haspopup="listbox"]' ? closeState : absent()),
      last: () => closeState,
    };
    return row as unknown as LocatorDouble;
  };
  const archiveRow = rowFor(ARCHIVE, archiveSwitch);
  const closeRow = rowFor(CLOSE, closeSwitch);

  const headings = (options.builtInRows ?? []).map((text) => new LocatorDouble({ text }));
  const extensionHeadings = options.extension === "rows" ? [new LocatorDouble({ text: "Edition automation" })] : [];
  const rowsOf = (items: LocatorDouble[]) => {
    const double = new LocatorDouble({
      locator: (selector) =>
        selector === "[data-automation-row]"
          ? new LocatorDouble({
              role: (role) => (role === "heading" ? new LocatorDouble({ items }) : absent()),
            })
          : absent(),
    });
    // Record the queries so a test can prove which container was read.
    double.locator = vi.fn(double.locator.bind(double));
    return double;
  };
  // The section wraps both the built-in rows and the extension slot, so a
  // query scoped to it would also see edition rows; the container scopes to
  // the built-in pair alone.
  const settingsSection = rowsOf([...headings, ...extensionHeadings]);
  const builtInRows = rowsOf(headings);
  const extensionSlot = new LocatorDouble({
    count: options.extension === "absent" ? 0 : 1,
    visible: true,
    locator: (selector) =>
      selector === "[data-automation-row]"
        ? new LocatorDouble({ count: options.extension === "rows" ? 1 : 0 })
        : absent(),
  });

  // The page-level detached wait after a cancel: the title leaves the DOM.
  const closingHeading = new LocatorDouble({ count: 0, visible: false });

  const archiveHeading = new LocatorDouble({
    count: options.settles === "refusal" ? 0 : 1,
    waitForRejects: options.settles !== "rows",
  });
  const refusalHeading = new LocatorDouble({
    count: options.settles === "refusal" ? 1 : 0,
    visible: options.settles === "refusal",
    waitForRejects: options.settles !== "refusal",
  });

  const page = {
    goto: vi.fn(async (_url: string) => undefined),
    waitForLoadState: vi.fn(async (_state: string) => undefined),
    route: vi.fn(async (_pattern: string, _handler: (route: Route) => Promise<void>) => undefined),
    getByRole: vi.fn((role: string, roleOptions?: { name?: string | RegExp; exact?: boolean; level?: number }) => {
      if (role === "option") return optionList.getByRole(role, roleOptions);
      if (role === "heading") {
        if (roleOptions?.name === ARCHIVE) return archiveHeading;
        if (roleOptions?.name === "Customize time range") return closingHeading;
        if (typeof roleOptions?.name === "string" && roleOptions.name.startsWith("Oops!")) return refusalHeading;
        return absent();
      }
      if (role === "dialog") return dialog;
      if (role === "button" && roleOptions?.name === "Customize time range") return new LocatorDouble();
      return absent();
    }),
    getByTestId: vi.fn((testId: string) =>
      testId === "automations-settings"
        ? settingsSection
        : testId === "automations-built-in-rows"
          ? builtInRows
          : testId === "automations-extension"
            ? extensionSlot
            : absent()
    ),
    getByPlaceholder: vi.fn((_placeholder: string) => monthInput),
    getByText: vi.fn((_text: string | RegExp) => absent()),
    // Rows are chosen by the title their wrapper carries.
    locator: vi.fn((selector: string) =>
      selector === "[data-automation-row]"
        ? ({
            first: () => ({ filter: ({ hasText }: { hasText: string }) => rowFor(hasText, archiveSwitch) }),
            filter: ({ hasText }: { hasText: string }) =>
              hasText === ARCHIVE ? archiveRow : hasText === CLOSE ? closeRow : absent(),
          } as unknown as LocatorDouble)
        : absent()
    ),
  };

  return {
    page: page as unknown as Page,
    pageObject: page,
    monthInput,
    monthError,
    submit,
    cancel,
    archiveSwitch,
    closeSwitch,
    delayTrigger,
    closeState,
    optionDoubles,
    optionClicks,
    archiveHeading,
    refusalHeading,
    extensionSlot,
    section: settingsSection,
    rowsContainer: builtInRows,
  };
}

function routeDouble(method: string) {
  return {
    request: () => ({ method: () => method }),
    continue: vi.fn(async () => undefined),
    fulfill: vi.fn(async (_options: unknown) => undefined),
  };
}

describe("WebNewDriver automations page", () => {
  it("opens the settings route and settles on the archive row", async () => {
    const stub = pageDouble({ settles: "rows" });
    await new WebNewDriver(stub.page).automationsOpen("acme", "p1");
    expect(stub.pageObject.goto).toHaveBeenCalledWith("/acme/settings/projects/p1/automations");
    expect(stub.pageObject.goto).toHaveBeenCalledTimes(1);
  });

  it("settles on the refusal view for a caller denied the page", async () => {
    const stub = pageDouble({ settles: "refusal", refusalVisible: true });
    const driver = new WebNewDriver(stub.page);
    await driver.automationsOpen("acme", "p1");
    await expect(driver.automationsNotAuthorizedVisible()).resolves.toBe(true);
  });

  it("retries once, then refuses to pretend the page settled", async () => {
    const stub = pageDouble({ settles: "neither" });
    await expect(new WebNewDriver(stub.page).automationsOpen("acme", "p1")).rejects.toThrow(
      "settled on neither the rows nor the refusal view"
    );
    expect(stub.pageObject.goto).toHaveBeenCalledTimes(2);
  });

  it("reports no refusal when the refusal heading is absent", async () => {
    const stub = pageDouble({ settles: "rows" });
    await expect(new WebNewDriver(stub.page).automationsNotAuthorizedVisible()).resolves.toBe(false);
  });
});

describe("WebNewDriver automations rows", () => {
  it("reads an enabled archive row with its month label", async () => {
    const stub = pageDouble({
      archive: { attributes: { "aria-checked": "true" }, disabled: false },
      delayTriggerText: "  6   months ",
    });
    await expect(new WebNewDriver(stub.page).automationsArchiveRow()).resolves.toEqual({
      toggleOn: true,
      toggleDisabled: false,
      pickerVisible: true,
      pickerLabel: "6 months",
    });
  });

  it("reads a disabled row with no picker as off and empty", async () => {
    const stub = pageDouble({ archive: { attributes: { "aria-checked": "false" } }, delayTriggerVisible: false });
    await expect(new WebNewDriver(stub.page).automationsArchiveRow()).resolves.toEqual({
      toggleOn: false,
      toggleDisabled: false,
      pickerVisible: false,
      pickerLabel: "",
    });
  });

  it("treats unreadable toggle reads as off and disabled", async () => {
    const stub = pageDouble({
      archive: { attributeRejects: true, disabledRejects: true },
      delayTriggerText: "",
    });
    await expect(new WebNewDriver(stub.page).automationsArchiveRow()).resolves.toEqual({
      toggleOn: false,
      toggleDisabled: true,
      pickerVisible: true,
      pickerLabel: "",
    });
  });

  it("flips the archive row through its own switch", async () => {
    const stub = pageDouble();
    await new WebNewDriver(stub.page).automationsArchiveToggle();
    expect(stub.archiveSwitch.click).toHaveBeenCalledTimes(1);
    expect(stub.closeSwitch.click).not.toHaveBeenCalled();
  });

  it("chooses a one-month preset by its singular label", async () => {
    const stub = pageDouble({ options: ["1 month", "3 months"] });
    await new WebNewDriver(stub.page).automationsArchiveSetPreset(1);
    expect(stub.delayTrigger.click).toHaveBeenCalledTimes(1);
    expect(stub.optionClicks).toEqual(["1 month"]);
  });

  it("chooses a multi-month preset by its plural label", async () => {
    const stub = pageDouble({ options: ["3 months", "6 months"] });
    await new WebNewDriver(stub.page).automationsCloseSetPreset(6);
    expect(stub.optionClicks).toEqual(["6 months"]);
  });

  it("opens the custom dialog from either row's picker", async () => {
    const archive = pageDouble({ month: { open: true } });
    await new WebNewDriver(archive.page).automationsArchiveOpenCustom();
    expect(archive.delayTrigger.click).toHaveBeenCalledTimes(1);
    expect(archive.monthInput.waitFor).toHaveBeenCalled();

    const close = pageDouble({ month: { open: true } });
    await new WebNewDriver(close.page).automationsCloseOpenCustom();
    expect(close.delayTrigger.click).toHaveBeenCalledTimes(1);
  });
});

describe("WebNewDriver auto-close row", () => {
  it("reads the cancelled-state target and its enabled picker", async () => {
    const stub = pageDouble({
      close: { attributes: { "aria-checked": "true" } },
      delayTriggerText: "3 months",
      closePanelVisible: true,
      closeStateText: " DROPPED ",
      closeStateDisabled: false,
    });
    await expect(new WebNewDriver(stub.page).automationsCloseRow()).resolves.toEqual({
      toggleOn: true,
      toggleDisabled: false,
      pickerVisible: true,
      pickerLabel: "3 months",
      stateLabel: "DROPPED",
      statePickerDisabled: false,
    });
  });

  it("reads a hidden detail panel as no target with the picker unavailable", async () => {
    const stub = pageDouble({ closePanelVisible: false });
    await expect(new WebNewDriver(stub.page).automationsCloseRow()).resolves.toEqual({
      toggleOn: false,
      toggleDisabled: false,
      pickerVisible: true,
      pickerLabel: "",
      stateLabel: "",
      statePickerDisabled: true,
    });
  });

  it("treats an unreadable picker as disabled", async () => {
    const stub = pageDouble({ closePanelVisible: true, closeStateText: "State", closeStateRejects: true });
    await expect(new WebNewDriver(stub.page).automationsCloseRow()).resolves.toMatchObject({
      stateLabel: "State",
      statePickerDisabled: true,
    });
  });

  it("flips the close row through its own switch", async () => {
    const stub = pageDouble();
    await new WebNewDriver(stub.page).automationsCloseToggle();
    expect(stub.closeSwitch.click).toHaveBeenCalledTimes(1);
    expect(stub.archiveSwitch.click).not.toHaveBeenCalled();
  });

  it("lists the cancelled-state options and closes the picker again", async () => {
    const stub = pageDouble({ options: ["Cancelled", " Dropped "] });
    await expect(new WebNewDriver(stub.page).automationsCloseStateOptions()).resolves.toEqual(["Cancelled", "Dropped"]);
    expect(stub.closeState.click).toHaveBeenCalledTimes(2);
  });

  it("refuses to read options from the disabled placeholder picker", async () => {
    // The page disables the trigger until a second cancelled state exists,
    // so there is no open list to read; the driver says so instead of
    // hanging on a click that can never open it.
    const stub = pageDouble({ closeStateDisabled: true, closeStateText: "State", options: [] });
    await expect(new WebNewDriver(stub.page).automationsCloseStateOptions()).rejects.toThrow(
      "[parity] the cancelled-state picker is disabled; a second cancelled state is required"
    );
    expect(stub.closeState.click).not.toHaveBeenCalled();
  });

  it("treats an unreadable trigger as disabled rather than clicking it", async () => {
    const stub = pageDouble({ closeStateRejects: true, options: ["Cancelled"] });
    await expect(new WebNewDriver(stub.page).automationsCloseStateOptions()).rejects.toThrow(
      "the cancelled-state picker is disabled"
    );
    expect(stub.closeState.click).not.toHaveBeenCalled();
  });

  it("picks a cancelled state by name", async () => {
    const stub = pageDouble({ options: ["Cancelled", "Dropped"] });
    await new WebNewDriver(stub.page).automationsCloseSetState("Dropped");
    expect(stub.closeState.click).toHaveBeenCalledTimes(1);
    expect(stub.optionClicks).toEqual(["Dropped"]);
  });
});

describe("WebNewDriver custom-month dialog", () => {
  it("reads nothing while the dialog is closed", async () => {
    const stub = pageDouble({ month: { open: false } });
    await expect(new WebNewDriver(stub.page).automationsMonthModal()).resolves.toBeNull();
  });

  it("reads the seeded value with no range message", async () => {
    const stub = pageDouble({ month: { open: true, input: "1" } });
    await expect(new WebNewDriver(stub.page).automationsMonthModal()).resolves.toEqual({
      title: "Customize time range",
      inputValue: "1",
      error: null,
    });
  });

  it("reports the range message when the dialog shows it", async () => {
    const stub = pageDouble({ month: { open: true, input: "", error: true } });
    await expect(new WebNewDriver(stub.page).automationsMonthModal()).resolves.toEqual({
      title: "Customize time range",
      inputValue: "",
      error: "Select a month between 1 and 12.",
    });
  });

  it("reads an unreadable field as empty", async () => {
    const stub = pageDouble({ month: { open: true, inputRejects: true } });
    await expect(new WebNewDriver(stub.page).automationsMonthModal()).resolves.toMatchObject({ inputValue: "" });
  });

  it("fills and submits through the dialog's own controls", async () => {
    const stub = pageDouble({ month: { open: true } });
    const driver = new WebNewDriver(stub.page);
    await driver.automationsMonthFill("7");
    await driver.automationsMonthSubmit();
    expect(stub.monthInput.fill).toHaveBeenCalledWith("7");
    expect(stub.submit.click).toHaveBeenCalledTimes(1);
    expect(stub.cancel.click).not.toHaveBeenCalled();
  });

  it("cancels and waits for the dialog heading to detach", async () => {
    const stub = pageDouble({ month: { open: true } });
    await new WebNewDriver(stub.page).automationsMonthCancel();
    expect(stub.cancel.click).toHaveBeenCalledTimes(1);
  });
});

describe("WebNewDriver automations failures and slots", () => {
  it("fails exactly the next project PATCH and passes everything else through", async () => {
    const stub = pageDouble();
    await new WebNewDriver(stub.page).automationsFailUpdateOnce();
    const handler = stub.pageObject.route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    expect(stub.pageObject.route).toHaveBeenCalledWith("**/api/workspaces/*/projects/*/", expect.any(Function));

    const read = routeDouble("GET");
    await handler(read as unknown as Route);
    expect(read.continue).toHaveBeenCalledTimes(1);

    const failed = routeDouble("PATCH");
    await handler(failed as unknown as Route);
    expect(failed.fulfill).toHaveBeenCalledTimes(1);
    expect(failed.continue).not.toHaveBeenCalled();

    const later = routeDouble("PATCH");
    await handler(later as unknown as Route);
    expect(later.continue).toHaveBeenCalledTimes(1);
    expect(later.fulfill).not.toHaveBeenCalled();
  });

  it("lists the built-in row headings in display order", async () => {
    const stub = pageDouble({ builtInRows: [ARCHIVE, CLOSE] });
    await expect(new WebNewDriver(stub.page).automationsBuiltInRows()).resolves.toEqual([ARCHIVE, CLOSE]);
  });

  it("leaves an edition's extension rows out of the built-in list", async () => {
    const stub = pageDouble({ builtInRows: [ARCHIVE, CLOSE], extension: "rows" });
    await expect(new WebNewDriver(stub.page).automationsBuiltInRows()).resolves.toEqual([ARCHIVE, CLOSE]);
    expect(stub.rowsContainer.locator).toHaveBeenCalledWith("[data-automation-row]");
    expect(stub.section.locator).not.toHaveBeenCalled();
  });

  it("reads an unreadable heading as empty text", async () => {
    const stub = pageDouble({ builtInRows: [" "] });
    await expect(new WebNewDriver(stub.page).automationsBuiltInRows()).resolves.toEqual([""]);
  });

  it("reports no extension rows for the community slot", async () => {
    const empty = pageDouble({ extension: "empty" });
    await expect(new WebNewDriver(empty.page).automationsHasExtensionRows()).resolves.toBe(false);
  });

  it("reports no extension rows when the slot is not mounted", async () => {
    const missing = pageDouble({ extension: "absent" });
    await expect(new WebNewDriver(missing.page).automationsHasExtensionRows()).resolves.toBe(false);
  });

  it("reports extension rows when the slot contributes its own", async () => {
    const stub = pageDouble({ extension: "rows" });
    await expect(new WebNewDriver(stub.page).automationsHasExtensionRows()).resolves.toBe(true);
  });
});
