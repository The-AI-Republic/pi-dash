// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver estimates creation-dialog, active-row,
// analytics-axis and cloud update-dialog methods (NEWFRONT-748,
// EST-008..016, EST-020, EST-029). A small Locator/Page double proves the
// selectors, read/action branches and the settle pollers without bringing
// up the parity stack.

import type { Page, Route } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => vi.restoreAllMocks());

interface FakeOptions {
  count?: number | (() => number);
  visible?: boolean;
  text?: string;
  input?: string;
  checked?: boolean;
  disabled?: boolean;
  attrs?: Record<string, string>;
  box?: { x: number; y: number; width: number; height: number } | null;
  waitForRejects?: boolean;
  items?: FakeLocator[];
  onClick?: () => void;
  onDblClick?: () => void;
  onFill?: (value: string) => void;
  onPressSequentially?: (value: string) => void;
  onCheck?: () => void;
  onSelectOption?: (values: unknown) => void;
  role?: (role: string, options?: { name?: string | RegExp; exact?: boolean }) => FakeLocator;
  textLocator?: (text: string | RegExp, options?: { exact?: boolean }) => FakeLocator;
  testId?: (testId: string) => FakeLocator;
  label?: (label: string | RegExp) => FakeLocator;
  locator?: (selector: string) => FakeLocator;
  filtered?: FakeLocator;
  filterFn?: (options: unknown) => FakeLocator;
  derived?: FakeLocator;
}

class FakeLocator {
  readonly click = vi.fn(async () => {
    this.options.onClick?.();
  });
  readonly dblclick = vi.fn(async () => {
    this.options.onDblClick?.();
  });
  readonly fill = vi.fn(async (value: string) => {
    this.options.onFill?.(value);
  });
  readonly pressSequentially = vi.fn(async (value: string, _options?: unknown) => {
    this.options.onPressSequentially?.(value);
  });
  readonly check = vi.fn(async () => {
    this.options.onCheck?.();
  });
  readonly selectOption = vi.fn(async (values: unknown) => {
    this.options.onSelectOption?.(values);
  });
  readonly waitFor = vi.fn(async (_options?: unknown) => {
    if (this.options.waitForRejects === true) throw new Error("timed out");
  });

  constructor(private readonly options: FakeOptions = {}) {}

  count(): Promise<number> {
    if (this.options.items !== undefined) return Promise.resolve(this.options.items.length);
    const count = this.options.count;
    if (typeof count === "function") return Promise.resolve(count());
    return Promise.resolve(count ?? 1);
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

  inputValue(_options?: unknown): Promise<string> {
    return Promise.resolve(this.options.input ?? "");
  }

  getAttribute(name: string): Promise<string | null> {
    return Promise.resolve(this.options.attrs?.[name] ?? null);
  }

  boundingBox(): Promise<{ x: number; y: number; width: number; height: number } | null> {
    return Promise.resolve(this.options.box ?? null);
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

  filter(options: unknown): FakeLocator {
    if (this.options.filterFn !== undefined) return this.options.filterFn(options);
    return this.options.filtered ?? this;
  }

  getByLabel(label: string | RegExp): FakeLocator {
    return this.options.label?.(label) ?? absent();
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

/** Mutable creation-dialog state the doubles read; clicks mutate it like the dialog would. */
interface EstimatesDialogState {
  open: boolean;
  step: 1 | 2;
  families: { value: string; label: string; checked: boolean; disabled: boolean }[];
  cards: { title: string; detail: string }[];
  values: string[];
  form: { value: string } | null;
  errors: string[];
}

function defaultDialogState(): EstimatesDialogState {
  return {
    open: true,
    step: 2,
    families: [
      { value: "points", label: "Points", checked: true, disabled: false },
      { value: "categories", label: "Categories", checked: false, disabled: false },
    ],
    cards: [
      { title: "Custom", detail: "1, 2" },
      { title: "Fibonacci", detail: "1, 2, 3, 5, 8, 13" },
    ],
    values: ["1", "2"],
    form: null,
    errors: [],
  };
}

function nameMatches(name: string | RegExp | undefined, exact: boolean | undefined, text: string): boolean {
  if (name === undefined) return true;
  if (typeof name === "string") return exact === true ? text === name : text.toLowerCase().includes(name.toLowerCase());
  return name.test(text);
}

interface DialogDoubles {
  dialog: FakeLocator;
  backButton: FakeLocator;
  footerCancel: FakeLocator;
  addButton: FakeLocator;
  inputPresses: string[];
}

function dialogDouble(state: EstimatesDialogState): DialogDoubles {
  const inputPresses: string[] = [];
  const backButton = new FakeLocator({
    count: state.step === 2 ? 1 : 0,
    visible: state.step === 2,
    onClick: () => {
      state.step = 1;
      state.values = [];
      state.form = null;
    },
  });
  const footerCancel = new FakeLocator({
    onClick: () => {
      state.open = false;
    },
  });
  const addButton = new FakeLocator({
    count: state.values.length >= 6 ? 0 : 1,
    visible: state.values.length < 6,
    onClick: () => {
      state.values.push("");
      state.form = { value: "" };
    },
  });

  const buttonCollection = (): FakeLocator => {
    const items = state.cards.map((card) => new FakeLocator({ text: `${card.title}\n${card.detail}` }));
    items.push(new FakeLocator({ text: "Cancel" }));
    return new FakeLocator({ items });
  };

  const valueCollection = (): FakeLocator =>
    new FakeLocator({
      items: state.values.map(
        (value, at) =>
          new FakeLocator({
            text: value,
            box: { x: 100, y: 200 + 30 * at, width: 50, height: 30 },
            onDblClick: () => {
              state.form = { value };
            },
          })
      ),
    });

  const dialog = new FakeLocator({
    count: state.open ? 1 : 0,
    visible: state.open,
    role: (role, options) => {
      if (role !== "button") return absent();
      const name = options?.name;
      const exact = options?.exact;
      if (name === undefined) return buttonCollection();
      if (nameMatches(name, exact, "Back")) return state.step === 2 ? backButton : absent();
      if (nameMatches(name, exact, "Cancel")) {
        // The footer renders after the body, so its Cancel is last
        // even when a value form is open with its own Cancel above it.
        const items = state.form === null ? [] : [new FakeLocator({})];
        items.push(footerCancel);
        return new FakeLocator({ items });
      }
      if (nameMatches(name, exact, "Add a value")) return addButton;
      const editMatch = /^Edit value (\d+)$/.exec(typeof name === "string" ? name : "");
      if (editMatch !== null && exact === true) {
        const at = Number(editMatch[1]) - 1;
        return new FakeLocator({
          count: state.values[at] === undefined ? 0 : 1,
          onClick: () => {
            state.form = { value: state.values[at] ?? "" };
          },
        });
      }
      const removeMatch = /^Remove value (\d+)$/.exec(typeof name === "string" ? name : "");
      if (removeMatch !== null && exact === true) {
        const at = Number(removeMatch[1]) - 1;
        if (state.values.length <= 2 || state.values[at] === undefined) return absent();
        return new FakeLocator({
          onClick: () => {
            state.values.splice(at, 1);
            state.form = null;
          },
        });
      }
      const reorderMatch = /^Reorder value (\d+)$/.exec(typeof name === "string" ? name : "");
      if (reorderMatch !== null && exact === true) {
        const at = Number(reorderMatch[1]) - 1;
        if (state.values[at] === undefined) return new FakeLocator({ box: null });
        return new FakeLocator({ box: { x: 0, y: 10 * at, width: 10, height: 10 } });
      }
      return absent();
    },
    testId: (testId) => {
      if (testId === "estimate-dialog-step") {
        return new FakeLocator({ count: state.open ? 1 : 0, text: `Step ${state.step} of 2` });
      }
      if (testId === "estimate-value") return valueCollection();
      return absent();
    },
    locator: (selector) => {
      if (selector === "fieldset label") {
        return new FakeLocator({
          items: state.families.map(
            (family) =>
              new FakeLocator({
                text: family.label,
                locator: (inner) =>
                  inner === 'input[type="radio"]'
                    ? new FakeLocator({
                        attrs: { value: family.value },
                        checked: family.checked,
                        disabled: family.disabled,
                      })
                    : absent(),
              })
          ),
        });
      }
      const familyRadio = /^input\[name="estimate-family"\]\[value="([^"]+)"\]$/.exec(selector);
      if (familyRadio !== null) {
        const value = familyRadio[1] ?? "";
        return new FakeLocator({
          count: state.families.some((family) => family.value === value) ? 1 : 0,
          onClick: () => {
            for (const family of state.families) family.checked = family.value === value;
          },
        });
      }
      if (selector === "form input") {
        return new FakeLocator({
          count: state.form === null ? 0 : 1,
          input: state.form?.value ?? "",
          onFill: (value) => {
            if (state.form !== null) state.form.value = value;
          },
          onPressSequentially: (value) => {
            inputPresses.push(value);
            if (state.form !== null) state.form.value = value;
          },
        });
      }
      if (selector === "form") {
        const form = new FakeLocator({
          count: state.form === null ? 0 : 1,
          waitForRejects: state.form === null,
          locator: (inner) => {
            if (inner === 'button[type="submit"]') {
              return new FakeLocator({
                onClick: () => {
                  // An empty draft never commits: the form stays open
                  // with its error, like the dialog's validation.
                  if (state.form !== null && state.form.value !== "") {
                    state.values = state.values.map((entry) => (entry === "" ? (state.form?.value ?? entry) : entry));
                    if (!state.values.includes(state.form.value)) state.values.push(state.form.value);
                    state.form = null;
                  }
                },
              });
            }
            if (inner === 'button[type="button"]') {
              return new FakeLocator({
                onClick: () => {
                  state.values = state.values.filter((entry) => entry !== "");
                  state.form = null;
                },
              });
            }
            return absent();
          },
        });
        return form;
      }
      if (selector === '[id^="estimate-value-"][id$="-problem"]') {
        return new FakeLocator({ items: state.errors.map((text) => new FakeLocator({ text })) });
      }
      return absent();
    },
  });
  return { dialog, backButton, footerCancel, addButton, inputPresses };
}

/** Mutable update-dialog state the doubles read; clicks mutate it like the dialog would. */
interface UpdateDialogState {
  open: boolean;
  name: string;
  values: string[];
  form: { index: number; value: string } | null;
  confirming: string | null;
  choice: "move" | "clear";
  replacement: string;
}

function defaultUpdateState(): UpdateDialogState {
  return {
    open: true,
    name: "Points",
    values: ["1", "2", "3"],
    form: null,
    confirming: null,
    choice: "clear",
    replacement: "",
  };
}

interface UpdateDoubles {
  dialog: FakeLocator;
  selectedOptions: unknown[];
}

function labelMatches(label: string | RegExp, text: string): boolean {
  if (typeof label === "string") return text.toLowerCase().includes(label.toLowerCase());
  return label.test(text);
}

function updateDialogDouble(state: UpdateDialogState): UpdateDoubles {
  const selectedOptions: unknown[] = [];
  const valueCollection = (): FakeLocator =>
    new FakeLocator({
      items: state.values.map((value) => new FakeLocator({ text: value })),
    });
  const dialog = new FakeLocator({
    count: () => (state.open ? 1 : 0),
    role: (role, options) => {
      if (role === "radio") {
        const name = options?.name;
        if (name !== undefined && nameMatches(name, undefined, "Move affected work items to another value")) {
          return new FakeLocator({
            onCheck: () => {
              state.choice = "move";
            },
          });
        }
        if (name !== undefined && nameMatches(name, undefined, "Clear the estimate on affected work items")) {
          return new FakeLocator({
            onCheck: () => {
              state.choice = "clear";
            },
          });
        }
        return absent();
      }
      if (role !== "button") return absent();
      const name = options?.name;
      const exact = options?.exact;
      if (nameMatches(name, exact, "Save changes")) {
        return new FakeLocator({
          onClick: () => {
            state.open = false;
          },
        });
      }
      if (nameMatches(name, exact, "Remove value")) {
        // The staged removal drops its row and returns to the list.
        return new FakeLocator({
          onClick: () => {
            if (state.confirming !== null) {
              state.values = state.values.filter((entry) => entry !== state.confirming);
              state.confirming = null;
            }
          },
        });
      }
      if (nameMatches(name, exact, "Cancel")) {
        return new FakeLocator({
          onClick: () => {
            state.open = false;
          },
        });
      }
      const editMatch = /^Edit value (\d+)$/.exec(typeof name === "string" ? name : "");
      if (editMatch !== null && exact === true) {
        const at = Number(editMatch[1]) - 1;
        if (state.values[at] === undefined) return absent();
        return new FakeLocator({
          onClick: () => {
            state.form = { index: at, value: state.values[at] ?? "" };
          },
        });
      }
      const removeMatch = /^Remove value (\d+)$/.exec(typeof name === "string" ? name : "");
      if (removeMatch !== null && exact === true) {
        const at = Number(removeMatch[1]) - 1;
        if (state.values.length <= 2 || state.values[at] === undefined) return absent();
        return new FakeLocator({
          onClick: () => {
            state.confirming = state.values[at] ?? null;
            state.choice = "clear";
          },
        });
      }
      return absent();
    },
    textLocator: (text) => {
      if (state.confirming === null) return absent();
      const title = `Remove ${state.confirming}?`;
      if (typeof text === "string" ? title === text : text.test(title)) return new FakeLocator({ text: title });
      return absent();
    },
    testId: (testId) => {
      if (testId === "estimate-edit-value") return valueCollection();
      return absent();
    },
    label: (label) => {
      if (labelMatches(label, "System name")) {
        return new FakeLocator({
          input: state.name,
          onFill: (value) => {
            state.name = value;
          },
        });
      }
      if (labelMatches(label, "Replacement value")) {
        return new FakeLocator({
          onSelectOption: (values) => {
            selectedOptions.push(values);
            const picked = values as { label?: unknown };
            if (typeof picked.label === "string") state.replacement = picked.label;
          },
        });
      }
      return absent();
    },
    locator: (selector) => {
      if (selector === "form input") {
        return new FakeLocator({
          count: state.form === null ? 0 : 1,
          input: state.form?.value ?? "",
          onFill: (value) => {
            if (state.form !== null) state.form.value = value;
          },
        });
      }
      if (selector === "form") {
        return new FakeLocator({
          count: state.form === null ? 0 : 1,
          waitForRejects: state.form === null,
          locator: (inner) => {
            if (inner === 'button[type="submit"]') {
              return new FakeLocator({
                onClick: () => {
                  if (state.form !== null && state.form.value !== "") {
                    state.values = state.values.map((entry, at) =>
                      at === (state.form?.index ?? -1) ? (state.form?.value ?? entry) : entry
                    );
                    state.form = null;
                  }
                },
              });
            }
            return absent();
          },
        });
      }
      return absent();
    },
  });
  return { dialog, selectedOptions };
}

interface PageStubOptions {
  dialogState?: () => EstimatesDialogState;
  updateState?: () => UpdateDialogState;
  cloudRow?: boolean;
  activeRow?: { name: string; values: string } | null;
  analyticsOpen?: boolean;
  pickerLabels?: { metric: string; xAxis: string; split: string };
  pickerOptions?: string[];
  evaluateResult?: unknown;
  tableRows?: string[][];
  listboxOptions?: string[];
  dialogWaitForRejects?: boolean;
}

interface PageStub {
  page: Page;
  pageObject: {
    route: ReturnType<typeof vi.fn>;
    evaluate: ReturnType<typeof vi.fn>;
    waitForFunction: ReturnType<typeof vi.fn>;
    keyboard: { press: ReturnType<typeof vi.fn> };
    mouse: {
      move: ReturnType<typeof vi.fn>;
      down: ReturnType<typeof vi.fn>;
      up: ReturnType<typeof vi.fn>;
    };
    getByRole: ReturnType<typeof vi.fn>;
    getByTestId: ReturnType<typeof vi.fn>;
  };
  state: EstimatesDialogState;
  doubles: DialogDoubles;
  update: UpdateDialogState;
  updateDoubles: UpdateDoubles;
  analyticsButton: FakeLocator;
  tableDouble: FakeLocator;
  triggerClicks: string[];
  listboxClicks: string[];
  onMouseUp: (() => void) | null;
}

function pageDouble(options: PageStubOptions = {}): PageStub {
  const state = options.dialogState?.() ?? defaultDialogState();
  const doubles = dialogDouble(state);
  const update = options.updateState?.() ?? defaultUpdateState();
  const updateDoubles = updateDialogDouble(update);
  const analyticsButton = new FakeLocator();
  const triggerClicks: string[] = [];
  const listboxClicks: string[] = [];
  const stub: { onMouseUp: (() => void) | null } = { onMouseUp: null };

  const tableDouble = new FakeLocator({
    count: 1,
    role: (role) => {
      if (role !== "table") return absent();
      const rows = options.tableRows ?? [];
      return new FakeLocator({
        role: (inner) => {
          if (inner !== "row") return absent();
          return new FakeLocator({
            items: rows.map(
              (cells) =>
                new FakeLocator({
                  locator: (innerSelector) =>
                    innerSelector === "th, td"
                      ? new FakeLocator({ items: cells.map((text) => new FakeLocator({ text })) })
                      : absent(),
                })
            ),
          });
        },
      });
    },
  });

  const activeRow = options.activeRow === undefined ? { name: "Points", values: "1, 2, 3" } : options.activeRow;
  const activeRowDouble =
    activeRow === null
      ? new FakeLocator({ count: 0, visible: false })
      : new FakeLocator({
          locator: (selector) => {
            if (selector === "h4") return new FakeLocator({ text: activeRow.name });
            if (selector === "p") return new FakeLocator({ text: activeRow.values });
            return absent();
          },
          role: (role, roleOptions) => {
            if (role !== "button") return absent();
            // OSS rows offer deletion only; the edit control exists on
            // cloud rows, and clicking it opens the update dialog.
            if (options.cloudRow !== true) return absent();
            const name = roleOptions?.name;
            if (typeof name !== "object" || !(name instanceof RegExp)) return absent();
            if (name.test(`Edit ${activeRow.name}`)) {
              return new FakeLocator({
                onClick: () => {
                  update.open = true;
                },
              });
            }
            return absent();
          },
        });

  const dialogRoot = new FakeLocator({
    count: state.open || update.open || options.analyticsOpen === true ? 1 : 0,
    waitForRejects: options.dialogWaitForRejects ?? false,
    filterFn: (filterOptions) => {
      const hasText = (filterOptions as { hasText?: unknown })?.hasText;
      if (hasText instanceof RegExp && hasText.test("Edit estimate system")) return updateDoubles.dialog;
      return doubles.dialog;
    },
    filtered: doubles.dialog,
  });

  const pageObject = {
    route: vi.fn(async (_pattern: string, _handler: (route: Route) => Promise<void>) => undefined),
    evaluate: vi.fn(
      async () =>
        options.evaluateResult ?? options.pickerLabels ?? { metric: "Work item", xAxis: "Priority", split: "" }
    ),
    waitForFunction: vi.fn(async () => undefined),
    keyboard: { press: vi.fn(async (_key: string) => undefined) },
    mouse: {
      move: vi.fn(async (_x: number, _y: number, _options?: unknown) => undefined),
      down: vi.fn(async () => undefined),
      up: vi.fn(async () => {
        stub.onMouseUp?.();
      }),
    },
    getByRole: vi.fn((role: string, roleOptions?: { name?: string | RegExp; exact?: boolean }) => {
      if (role === "dialog") return dialogRoot;
      if (role === "button" && nameMatches(roleOptions?.name, roleOptions?.exact, "Analytics")) {
        return analyticsButton;
      }
      return absent();
    }),
    getByTestId: vi.fn((testId: string) => {
      const trigger = /^analytics-custom-trigger-(\d+)$/.exec(testId);
      if (trigger !== null) {
        return new FakeLocator({
          onClick: () => {
            triggerClicks.push(testId);
          },
        });
      }
      if (testId === "analytics-custom-table") return tableDouble;
      return absent();
    }),
  };

  const page = {
    ...pageObject,
    waitForTimeout: vi.fn(async (_ms: number) => undefined),
    url: vi.fn(() => "https://parity.test/acme/settings/projects/p1/estimates"),
    locator: vi.fn((selector: string) => {
      if (selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]') {
        return activeRowDouble;
      }
      if (selector === "[role='listbox'] [role='option']") {
        return new FakeLocator({
          items: (options.listboxOptions ?? options.pickerOptions ?? []).map(
            (text) =>
              new FakeLocator({
                text,
                onClick: () => {
                  listboxClicks.push(text);
                },
              })
          ),
        });
      }
      return absent();
    }),
    getByText: vi.fn((_text: string | RegExp) => absent()),
    getByPlaceholder: vi.fn((_placeholder: string) => absent()),
  };
  return {
    page: page as unknown as Page,
    pageObject,
    state,
    doubles,
    update,
    updateDoubles,
    analyticsButton,
    tableDouble,
    triggerClicks,
    listboxClicks,
    get onMouseUp() {
      return stub.onMouseUp;
    },
    set onMouseUp(handler: (() => void) | null) {
      stub.onMouseUp = handler;
    },
  };
}

describe("WebNewDriver estimates dialog shell", () => {
  it("opens the creation dialog through the empty-state control", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenCreateDialog();
    expect(stub.pageObject.getByRole).toHaveBeenCalledWith("button", { name: /add estimate system/i });
  });

  it("reads the step counter from the dialog", async () => {
    const stub = pageDouble({ dialogState: () => ({ ...defaultDialogState(), step: 1 }) });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesDialogStep()).resolves.toBe("1");
  });

  it("reads step two without confusing the counter's own total", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesDialogStep()).resolves.toBe("2");
  });

  it("reads null when the dialog is closed", async () => {
    const stub = pageDouble({ dialogState: () => ({ ...defaultDialogState(), open: false }) });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesDialogStep()).resolves.toBeNull();
  });

  it("backs to step one and settles on its counter", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesDialogBack();
    expect(stub.doubles.backButton.click).toHaveBeenCalledTimes(1);
    expect(stub.state.step).toBe(1);
    await expect(driver.estimatesDialogStep()).resolves.toBe("1");
  });

  it("reports the back control only on step two", async () => {
    const two = pageDouble();
    await expect(new WebNewDriver(two.page).estimatesBackVisible()).resolves.toBe(true);
    const one = pageDouble({ dialogState: () => ({ ...defaultDialogState(), step: 1 }) });
    await expect(new WebNewDriver(one.page).estimatesBackVisible()).resolves.toBe(false);
  });

  it("cancels through the footer action even with a value form open", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesDialogCancel();
    expect(stub.doubles.footerCancel.click).toHaveBeenCalledTimes(1);
    expect(stub.state.open).toBe(false);
  });

  it("parses starter cards and skips the footer cancel", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesStarterCards()).resolves.toEqual([
      { title: "Custom", detail: "1, 2" },
      { title: "Fibonacci", detail: "1, 2, 3, 5, 8, 13" },
    ]);
  });

  it("reads step-two values in display order", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["10", "20", "3"] }),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesStepTwoValues()).resolves.toEqual(["10", "20", "3"]);
  });
});

describe("WebNewDriver estimates family choice", () => {
  it("reads the family radios with their state", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesFamilyOptions()).resolves.toEqual([
      { value: "points", label: "Points", disabled: false, checked: true },
      { value: "categories", label: "Categories", disabled: false, checked: false },
    ]);
  });

  it("selects a family and settles on its starters", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesSelectFamily("categories");
    expect(stub.state.families.map((family) => family.checked)).toEqual([false, true]);
  });

  it("selects the cloud-only time family the same way", async () => {
    const stub = pageDouble({
      dialogState: () => ({
        ...defaultDialogState(),
        families: [
          { value: "points", label: "Points", checked: true, disabled: false },
          { value: "categories", label: "Categories", checked: false, disabled: false },
          { value: "time", label: "Time", checked: false, disabled: false },
        ],
      }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesSelectFamily("time");
    expect(stub.state.families.map((family) => family.checked)).toEqual([false, false, true]);
  });
});

describe("WebNewDriver estimates value arrangement", () => {
  it("drags a value before its target with a real mouse path", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", "3"] }),
    });
    stub.onMouseUp = () => {
      const [moved] = stub.state.values.splice(0, 1);
      stub.state.values.splice(1, 0, moved ?? "");
    };
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesDragValueBefore(0, 2);
    expect(stub.state.values).toEqual(["2", "1", "3"]);
    expect(stub.pageObject.mouse.move).toHaveBeenNthCalledWith(1, 5, 5);
    expect(stub.pageObject.mouse.move).toHaveBeenNthCalledWith(2, 125, 264, { steps: 12 });
    expect(stub.pageObject.mouse.down).toHaveBeenCalledTimes(1);
    expect(stub.pageObject.mouse.up).toHaveBeenCalledTimes(1);
  });

  it("fails fast when a dragged row has no box", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesDragValueBefore(0, 5)).rejects.toThrow("[parity] value row has no box.");
  });

  it("starts an edit through double-click with the row value pre-populated", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesEditValueStart(0, "double-click");
    expect(stub.state.form).toEqual({ value: "1" });
    await expect(driver.estimatesOpenFormValue()).resolves.toBe("1");
  });

  it("starts an edit through the row control", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesEditValueStart(1, "control");
    expect(stub.state.form).toEqual({ value: "2" });
  });

  it("removes a value and settles on the shorter row set", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["10", "20", "3"] }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesRemoveValue(0);
    expect(stub.state.values).toEqual(["20", "3"]);
  });

  it("offers remove only above two values", async () => {
    const three = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["10", "20", "3"] }),
    });
    await expect(new WebNewDriver(three.page).estimatesRemoveVisible(0)).resolves.toBe(true);
    const two = pageDouble();
    await expect(new WebNewDriver(two.page).estimatesRemoveVisible(0)).resolves.toBe(false);
  });

  it("opens the add form through its control", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesAddOpen();
    expect(stub.doubles.addButton.click).toHaveBeenCalledTimes(1);
    expect(stub.state.form).toEqual({ value: "" });
  });

  it("offers add only below six values", async () => {
    const few = pageDouble();
    await expect(new WebNewDriver(few.page).estimatesAddVisible()).resolves.toBe(true);
    const full = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", "3", "4", "5", "6"] }),
    });
    await expect(new WebNewDriver(full.page).estimatesAddVisible()).resolves.toBe(false);
  });

  it("types into the open add form keystroke by keystroke", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", ""], form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesAddType("123");
    expect(stub.doubles.inputPresses).toEqual(["123"]);
    expect(stub.state.form).toEqual({ value: "123" });
  });
});

describe("WebNewDriver estimates value forms and validation", () => {
  it("reads the open form value and null with no form", async () => {
    const open = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), form: { value: "9" } }),
    });
    await expect(new WebNewDriver(open.page).estimatesOpenFormValue()).resolves.toBe("9");
    const closed = pageDouble();
    await expect(new WebNewDriver(closed.page).estimatesOpenFormValue()).resolves.toBeNull();
  });

  it("counts open forms", async () => {
    const open = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), form: { value: "" } }),
    });
    await expect(new WebNewDriver(open.page).estimatesOpenFormCount()).resolves.toBe(1);
    const closed = pageDouble();
    await expect(new WebNewDriver(closed.page).estimatesOpenFormCount()).resolves.toBe(0);
  });

  it("fills the open form without submitting it", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenFormFill("abc");
    expect(stub.state.form).toEqual({ value: "abc" });
  });

  it("submits the open form and commits its draft", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", ""], form: { value: "3" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenFormSubmit();
    expect(stub.state.values).toEqual(["1", "2", "3"]);
    expect(stub.state.form).toBeNull();
  });

  it("cancels the open form and drops its placeholder row", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", ""], form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenFormCancel();
    expect(stub.state.values).toEqual(["1", "2"]);
    expect(stub.state.form).toBeNull();
  });

  it("saves through fill plus submit and settles on the landed value", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", ""], form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenFormSave("3");
    expect(stub.state.values).toEqual(["1", "2", "3"]);
    expect(stub.state.form).toBeNull();
  });

  it("dismisses a leftover form when the save does not commit", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), values: ["1", "2", ""], form: { value: "" } }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenFormSave("");
    expect(stub.state.form).toBeNull();
    expect(stub.state.values).toEqual(["1", "2"]);
  });

  it("reads validation markers in order and skips blank ones", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), errors: ["Enter a numeric value.", "   "] }),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesValueErrors()).resolves.toEqual(["Enter a numeric value."]);
  });

  it("counts validation markers", async () => {
    const stub = pageDouble({
      dialogState: () => ({ ...defaultDialogState(), errors: ["One.", "Two."] }),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesValueErrorCount()).resolves.toBe(2);
    const clean = pageDouble();
    await expect(new WebNewDriver(clean.page).estimatesValueErrorCount()).resolves.toBe(0);
  });
});

describe("WebNewDriver estimates active row and delete spy", () => {
  it("reads the active row name and joined values", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesActiveRow()).resolves.toEqual({ name: "Points", values: "1, 2, 3" });
  });

  it("reads null when no active row shows", async () => {
    const stub = pageDouble({ activeRow: null });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesActiveRow()).resolves.toBeNull();
  });

  it("reads null when the row carries no name", async () => {
    const stub = pageDouble({ activeRow: { name: "  ", values: "1, 2" } });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesActiveRow()).resolves.toBeNull();
  });

  it("refuses whole-system deletes behind the dialog's back", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesEditDeleteFailEstimateDelete();
    expect(stub.pageObject.route).toHaveBeenCalledTimes(1);
    const [pattern, handler] = stub.pageObject.route.mock.calls[0] as [string, (route: Route) => Promise<void>];
    expect(pattern).toBe("**/api/workspaces/*/projects/*/estimates/*/");

    const refused = {
      request: () => ({ method: () => "DELETE" }),
      fulfill: vi.fn(async (_response: unknown) => undefined),
      continue: vi.fn(async () => undefined),
    };
    await handler(refused as unknown as Route);
    expect(refused.fulfill).toHaveBeenCalledWith({
      status: 500,
      body: JSON.stringify({ error: "parity refuses this delete" }),
    });
    expect(refused.continue).not.toHaveBeenCalled();

    const passed = {
      request: () => ({ method: () => "GET" }),
      fulfill: vi.fn(async (_response: unknown) => undefined),
      continue: vi.fn(async () => undefined),
    };
    await handler(passed as unknown as Route);
    expect(passed.continue).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });
});

describe("WebNewDriver estimates analytics axis", () => {
  it("opens analytics through the header entry", async () => {
    const stub = pageDouble({ analyticsOpen: true });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesOpenAnalytics();
    expect(stub.analyticsButton.click).toHaveBeenCalledTimes(1);
  });

  it("reads the metric and x-axis labels", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesAnalyticsYAxisValue()).resolves.toBe("Work item");
    await expect(driver.estimatesAnalyticsXAxisValue()).resolves.toBe("Priority");
  });

  it("reads the metric options through its own trigger", async () => {
    const stub = pageDouble({ evaluateResult: ["Work item"] });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesAnalyticsYAxisOptions()).resolves.toEqual(["Work item"]);
    expect(stub.triggerClicks).toEqual(["analytics-custom-trigger-0"]);
    expect(stub.pageObject.keyboard.press).toHaveBeenCalledWith("Escape");
  });

  it("reads the x-axis options through its own trigger", async () => {
    const stub = pageDouble({ evaluateResult: ["Priority", "Estimate point"] });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesAnalyticsXAxisOptions()).resolves.toEqual(["Priority", "Estimate point"]);
    expect(stub.triggerClicks).toEqual(["analytics-custom-trigger-1"]);
  });

  it("picks the x-axis option by label", async () => {
    const stub = pageDouble({ listboxOptions: ["Priority", "Estimate point"] });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesAnalyticsPickXAxis("Estimate point");
    expect(stub.triggerClicks).toEqual(["analytics-custom-trigger-1"]);
    expect(stub.listboxClicks).toEqual(["Estimate point"]);
  });

  it("reads the aggregation table with its header first", async () => {
    const stub = pageDouble({
      tableRows: [
        ["Estimate point", "Count"],
        ["1", "1"],
        ["3", "1"],
      ],
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesAnalyticsTableRows()).resolves.toEqual([
      ["Estimate point", "Count"],
      ["1", "1"],
      ["3", "1"],
    ]);
  });
});

describe("WebNewDriver estimates cloud update dialog", () => {
  it("sees no edit control on OSS rows", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesEditDeleteActiveRowEditControlVisible()).resolves.toBe(false);
  });

  it("sees the edit control on cloud rows and opens the dialog through it", async () => {
    const stub = pageDouble({
      cloudRow: true,
      updateState: () => ({ ...defaultUpdateState(), open: false }),
    });
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesEditDeleteActiveRowEditControlVisible()).resolves.toBe(true);
    await expect(driver.estimatesUpdateDialogOpen()).resolves.toBe(false);
    await driver.estimatesActiveRowEditClick();
    await expect(driver.estimatesUpdateDialogOpen()).resolves.toBe(true);
  });

  it("reads the update dialog values in row order", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesUpdateDialogValues()).resolves.toEqual(["1", "2", "3"]);
  });

  it("renames the system through the name field", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogSetName("Renamed");
    expect(stub.update.name).toBe("Renamed");
  });

  it("rewrites one value through its edit control", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogEditValue(1, "5");
    expect(stub.update.values).toEqual(["1", "5", "3"]);
  });

  it("saves the dialog and settles on its close", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogSave();
    expect(stub.update.open).toBe(false);
  });

  it("cancels the dialog without saving", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogCancel();
    expect(stub.update.open).toBe(false);
  });

  it("reads no removal title outside a confirmation", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await expect(driver.estimatesUpdateDialogRemovalTitle()).resolves.toBe("");
  });

  it("opens the reassignment confirmation for a saved value", async () => {
    const stub = pageDouble();
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogRemoveValue(1);
    await expect(driver.estimatesUpdateDialogRemovalTitle()).resolves.toBe("Remove 2?");
    expect(stub.update.choice).toBe("clear");
  });

  it("chooses the move reassignment with its replacement", async () => {
    const stub = pageDouble({
      updateState: () => ({ ...defaultUpdateState(), confirming: "2" }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogChooseMove("3");
    expect(stub.update.choice).toBe("move");
    expect(stub.update.replacement).toBe("3");
    expect(stub.updateDoubles.selectedOptions).toEqual([{ label: "3" }]);
  });

  it("chooses clearing for the pending removal", async () => {
    const stub = pageDouble({
      updateState: () => ({ ...defaultUpdateState(), confirming: "2", choice: "move" }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogChooseClear();
    expect(stub.update.choice).toBe("clear");
  });

  it("confirms the removal and returns to the value list", async () => {
    const stub = pageDouble({
      updateState: () => ({ ...defaultUpdateState(), confirming: "2" }),
    });
    const driver = new WebNewDriver(stub.page);
    await driver.estimatesUpdateDialogConfirmRemoval();
    expect(stub.update.values).toEqual(["1", "3"]);
    await expect(driver.estimatesUpdateDialogRemovalTitle()).resolves.toBe("");
  });
});
