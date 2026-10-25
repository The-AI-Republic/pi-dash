// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver project-scheduler methods
// (NEWFRONT-604, PSET-055–061). Playwright is replaced by a small
// Locator/Page double so the assertions pin the addresses, selectors and
// read shapes without a browser.

import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Options {
  count?: number;
  visible?: boolean;
  text?: string;
  content?: string;
  input?: string;
  checked?: boolean;
  selected?: boolean;
  disabled?: boolean;
  attribute?: string | null;
  items?: FakeLocator[];
  locate?: (selector: string) => FakeLocator;
  role?: (role: string, options?: Record<string, unknown>) => FakeLocator;
  byText?: (text: string | RegExp, options?: Record<string, unknown>) => FakeLocator;
  byLabel?: (label: string) => FakeLocator;
  filter?: (options: unknown) => FakeLocator;
  first?: () => FakeLocator;
  last?: () => FakeLocator;
  or?: (other: FakeLocator) => FakeLocator;
  click?: () => Promise<void>;
  fill?: (value: string) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  selectOption?: (value: string) => Promise<void>;
  evaluateAll?: (fn: (nodes: unknown[]) => unknown) => Promise<unknown>;
  testId?: (id: string) => FakeLocator;
  all?: () => FakeLocator[];
  waitFor?: () => Promise<void>;
  innerText?: () => Promise<string>;
  evaluate?: unknown;
  throws?: boolean;
}

class FakeLocator {
  readonly options: Options;
  readonly click: ReturnType<typeof vi.fn>;
  readonly fill: ReturnType<typeof vi.fn>;
  readonly selectOption: ReturnType<typeof vi.fn>;
  readonly evaluateAll: ReturnType<typeof vi.fn>;
  readonly waitFor = vi.fn(async () => undefined);

  constructor(options: Options = {}) {
    this.options = options;
    this.click = vi.fn(options.click ?? (async () => undefined));
    this.fill = vi.fn(options.fill ?? (async () => undefined));
    this.selectOption = vi.fn(options.selectOption ?? (async () => undefined));
    this.evaluateAll = vi.fn(options.evaluateAll ?? (async () => []));
    this.waitFor.mockImplementation(options.waitFor ?? (async () => undefined));
  }

  private fail<T>(value: T): Promise<T> {
    return this.options.throws === true ? Promise.reject(new Error("read failed")) : Promise.resolve(value);
  }
  count(): Promise<number> {
    return this.fail(this.options.items?.length ?? this.options.count ?? 0);
  }
  all(): Promise<FakeLocator[]> {
    return this.fail(this.options.all !== undefined ? this.options.all() : (this.options.items ?? []));
  }
  isVisible(): Promise<boolean> {
    return this.fail(this.options.visible ?? true);
  }
  isChecked(): Promise<boolean> {
    return this.fail(this.options.checked ?? false);
  }
  isSelected(): Promise<boolean> {
    return this.fail(this.options.selected ?? false);
  }
  isDisabled(): Promise<boolean> {
    return this.fail(this.options.disabled ?? false);
  }
  innerText(): Promise<string> {
    return this.options.innerText !== undefined ? this.options.innerText() : this.fail(this.options.text ?? "");
  }
  textContent(): Promise<string | null> {
    return this.fail(this.options.content ?? this.options.text ?? "");
  }
  inputValue(): Promise<string> {
    return this.fail(this.options.input ?? "");
  }
  getAttribute(name: string): Promise<string | null> {
    if (this.options.getAttribute !== undefined) return this.options.getAttribute(name);
    return Promise.resolve(this.options.attribute ?? null);
  }
  first(): FakeLocator {
    if (this.options.first !== undefined) return this.options.first();
    return this.options.items?.[0] ?? this;
  }
  last(): FakeLocator {
    if (this.options.last !== undefined) return this.options.last();
    return this.options.items?.at(-1) ?? this;
  }
  or(other: FakeLocator): FakeLocator {
    if (this.options.or !== undefined) return this.options.or(other);
    return this;
  }
  nth(index: number): FakeLocator {
    return this.options.items?.[index] ?? absent();
  }
  filter(options: unknown): FakeLocator {
    return this.options.filter?.(options) ?? this;
  }
  getByRole(role: string, roleOptions?: Record<string, unknown>): FakeLocator {
    return this.options.role?.(role, roleOptions) ?? this;
  }
  getByText(text: string | RegExp, textOptions?: Record<string, unknown>): FakeLocator {
    return this.options.byText?.(text, textOptions) ?? this;
  }
  getByLabel(label: string): FakeLocator {
    return this.options.byLabel?.(label) ?? absent();
  }
  getByTestId(id: string): FakeLocator {
    return this.options.testId?.(id) ?? absent();
  }
  evaluate(): Promise<unknown> {
    if (this.options.throws === true) return Promise.reject(new Error("evaluate failed"));
    return Promise.resolve(this.options.evaluate ?? "");
  }
  locator(selector: string): FakeLocator {
    return this.options.locate?.(selector) ?? absent();
  }
}

function absent(): FakeLocator {
  return new FakeLocator({ count: 0 });
}

function fake(options: Options = {}): FakeLocator & Locator {
  return new FakeLocator(options) as unknown as FakeLocator & Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    waitForTimeout: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => ({})),
    waitForURL: vi.fn(async () => undefined),
    setViewportSize: vi.fn(async () => undefined),
    url: vi.fn(() => "http://localhost/acme/projects/p1/schedulers/calendar"),
    context: vi.fn(() => ({ newPage: vi.fn(async () => pageOf()) })),
    close: vi.fn(async () => undefined),
    route: vi.fn(async () => undefined),
    unroute: vi.fn(async () => undefined),
    getByRole: vi.fn(() => absent()),
    getByText: vi.fn(() => absent()),
    getByTestId: vi.fn(() => absent()),
    locator: vi.fn(() => absent()),
    ...overrides,
  } as unknown as Page;
}

describe("scheduler settings entry", () => {
  it("opens the settings address and settles on the installs table", async () => {
    const goto = vi.fn(async () => null);
    const header = fake({ count: 1 });
    const page = pageOf({
      goto,
      getByRole: () => fake({ count: 1, items: [header] }),
    });
    await new WebNewDriver(page).schedulerOpenSettingsSchedulers("acme", "p1");
    expect(goto).toHaveBeenCalledWith("/acme/settings/projects/p1/schedulers");
  });

  it("reports the panel only when the Scheduler column header shows", async () => {
    const shown = pageOf({ getByRole: () => fake({ count: 1, visible: true }) });
    await expect(new WebNewDriver(shown).schedulerSettingsPanelVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(pageOf()).schedulerSettingsPanelVisible()).resolves.toBe(false);
  });
});

describe("install table reads", () => {
  function tablePage(): Page {
    const identity = fake({
      locate: (selector) =>
        selector === "div"
          ? fake({ text: "Nightly audit" })
          : selector === "code"
            ? fake({ text: "nightly" })
            : absent(),
    });
    const cells = [
      identity,
      fake({ text: "Every day", attribute: "FREQ=DAILY" }),
      fake({ text: "Jan 2, 2026" }),
      fake({ text: "(never)" }),
      fake({ text: "Enabled" }),
      fake({ text: "Jan 1, 2026" }),
      fake({ role: () => fake({ count: 2 }) }),
    ];
    const dataRow = fake({
      items: cells,
      locate: (selector) => (selector === "td[colspan]" ? absent() : (dataRow as unknown as FakeLocator)),
    });
    const emptyRow = fake({ locate: (selector) => (selector === "td[colspan]" ? fake({ count: 1 }) : absent()) });
    const rows = fake({ items: [dataRow, emptyRow] });
    return pageOf({ locator: () => rows });
  }

  it("reads one row per data row, skipping the spanning empty state", async () => {
    const read = await new WebNewDriver(tablePage()).schedulerProjectRows();
    expect(read).toEqual([
      {
        name: "Nightly audit",
        handle: "nightly",
        schedule: "Every day",
        scheduleTitle: "FREQ=DAILY",
        nextRun: "Jan 2, 2026",
        lastRun: "(never)",
        status: "Enabled",
        updated: "Jan 1, 2026",
        manageVisible: true,
      },
    ]);
  });

  it("reads the switch state and flips it through the row", async () => {
    const click = vi.fn(async () => undefined);
    const row = fake({
      role: (role) => (role === "switch" ? fake({ attribute: "true", disabled: true, click }) : absent()),
    });
    const page = pageOf({ locator: () => fake({ filter: () => row }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectToggleState("nightly")).resolves.toEqual({ checked: true, disabled: true });
    await driver.schedulerProjectToggle("nightly");
    expect(click).toHaveBeenCalled();
  });

  it("reports the empty prompt and the creation control", async () => {
    const page = pageOf({
      getByText: () => fake({ count: 1, visible: true }),
      getByRole: () => fake({ count: 1, visible: true }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectEmptyVisible()).resolves.toBe(true);
    await expect(driver.schedulerProjectNewVisible()).resolves.toBe(true);
  });
});

describe("install dialog", () => {
  it("reads the dead-end mode and its text", async () => {
    const dialog = fake({
      byText: (text) => (String(text).includes("No schedulers available") ? fake({ count: 1 }) : absent()),
      text: "No schedulers available Ask a workspace admin to add one.",
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectInstallMode()).resolves.toBe("dead-end");
    await expect(driver.schedulerProjectInstallDeadEndText()).resolves.toContain("No schedulers available");
  });

  it("reads the selected tab as the create path", async () => {
    const tabs = fake({
      items: [fake({ text: "Install existing" }), fake({ text: "Create new" })],
      role: (role) => (role === "tab" ? fake({ text: "Create new" }) : absent()),
    });
    const dialog = fake({
      byText: () => absent(),
      role: (role, options) =>
        role !== "tab" ? absent() : options?.["selected"] === true ? fake({ text: "Create new" }) : tabs,
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectInstallTabs()).resolves.toEqual(["Install existing", "Create new"]);
    await expect(driver.schedulerProjectInstallMode()).resolves.toBe("create");
  });

  it("reads options as name plus handle and selects by handle", async () => {
    const selectOption = vi.fn(async () => undefined);
    const options = fake({
      items: [fake({ text: "Nightly (nightly)", attribute: "s1" }), fake({ text: "Weekly (weekly)", attribute: "s2" })],
    });
    const select = fake({ count: 1, input: "s1", selectOption, locate: () => options });
    const dialog = fake({ locate: () => select });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectInstallOptions()).resolves.toEqual([
      { name: "Nightly", handle: "nightly", selected: true },
      { name: "Weekly", handle: "weekly", selected: false },
    ]);
    await driver.schedulerProjectInstallSelect("weekly");
    expect(selectOption).toHaveBeenCalledWith("s2");
  });

  it("throws when the chosen handle is not offered", async () => {
    const select = fake({ input: "", locate: () => fake({ count: 0 }) });
    const dialog = fake({ locate: () => select });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerProjectInstallSelect("gone")).rejects.toThrow(
      "offers no definition with handle gone"
    );
  });

  it("reads schedule values and reports visible alerts as errors", async () => {
    const fields: Record<string, FakeLocator> = {
      "Starts at": fake({ input: "2026-01-02T09:30" }),
      "Time zone": fake({ input: "Europe/Paris" }),
      "Recurrence (RRULE)": fake({ input: "FREQ=DAILY" }),
      "Project context (optional)": fake({ input: "ctx" }),
    };
    const alerts = fake({ items: [fake({ text: "Start time is required." }), fake({ text: "" })] });
    const dialog = fake({
      byLabel: (label) => fields[label] ?? absent(),
      role: (role) => (role === "switch" ? fake({ attribute: "true" }) : absent()),
      locate: (selector) => (selector === '[role="alert"]' ? alerts : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerProjectInstallScheduleValues()).resolves.toEqual({
      dtstart: "2026-01-02T09:30",
      tzid: "Europe/Paris",
      rrule: "FREQ=DAILY",
      extraContext: "ctx",
      enabled: true,
    });
    await expect(driver.schedulerProjectInstallErrors()).resolves.toEqual(["Start time is required."]);
  });

  it("reveals the advanced rule field before filling a rule the builder hides", async () => {
    let expanded = false;
    const toggle = fake({
      count: 1,
      click: async () => {
        expanded = true;
      },
      getAttribute: async (name: string) => (name === "aria-expanded" && expanded ? "true" : "false"),
    });
    const raw = fake({ count: 1, visible: false, fill: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: (label: string) => (label === "Recurrence (RRULE)" ? raw : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerProjectEditFill({ rrule: "FREQ=WEEKLY;BYDAY=MO" });
    expect(toggle.click).toHaveBeenCalledTimes(1);
    expect(raw.fill).toHaveBeenCalledWith("FREQ=WEEKLY;BYDAY=MO");
  });

  it("fills an already visible rule without touching the disclosure", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 1, visible: true, fill: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: (label: string) => (label === "Recurrence (RRULE)" ? raw : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerProjectEditFill({ rrule: "FREQ=DAILY" });
    expect(toggle.click).not.toHaveBeenCalled();
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY");
  });

  it("reads edit values from the findings group, not the recurrence Ends radios", async () => {
    const checked = fake({ items: [fake({ text: "Create issues" })] });
    const group = fake({ role: (role: string) => (role === "radio" ? checked : absent()) });
    const fields: Record<string, FakeLocator> = {
      "Starts at": fake({ input: "2026-01-02T09:30" }),
      "Time zone": fake({ input: "Europe/Paris" }),
      "Recurrence (RRULE)": fake({ input: "FREQ=DAILY" }),
      "Project context (optional)": fake({ input: "ctx" }),
      Pod: fake({ input: "p1" }),
    };
    const dialog = fake({
      byLabel: (label) => fields[label] ?? absent(),
      role: (role, options) => {
        if (role === "switch") return fake({ attribute: "true" });
        if (role === "radiogroup" && options?.name === "What to do with findings") return group;
        return absent();
      },
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerProjectEditValues()).resolves.toEqual({
      dtstart: "2026-01-02T09:30",
      tzid: "Europe/Paris",
      rrule: "FREQ=DAILY",
      extraContext: "ctx",
      enabled: true,
      outcomeLabel: "Create issues",
      pod: "p1",
    });
  });

  it("leaves the edit outcome label blank when the checked radio cannot be read", async () => {
    const checked = fake({
      items: [fake({ innerText: async () => Promise.reject(new Error("detached")) })],
    });
    const group = fake({ role: (role: string) => (role === "radio" ? checked : absent()) });
    const fields: Record<string, FakeLocator> = {
      "Starts at": fake({ input: "" }),
      "Time zone": fake({ input: "UTC" }),
      "Recurrence (RRULE)": fake({ input: "" }),
      "Project context (optional)": fake({ input: "" }),
      Pod: fake({ input: "" }),
    };
    const dialog = fake({
      byLabel: (label) => fields[label] ?? absent(),
      role: (role, options) => {
        if (role === "switch") return fake({ attribute: "false" });
        if (role === "radiogroup" && options?.name === "What to do with findings") return group;
        return absent();
      },
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerProjectEditValues()).resolves.toMatchObject({ outcomeLabel: "" });
  });

  it("reads the outcome modes and the current help line", async () => {
    const group = fake({
      role: (role) =>
        role === "radio"
          ? fake({
              items: [fake({ checked: true, text: "Create issues" }), fake({ checked: false, text: "Apply fix" })],
            })
          : absent(),
      locate: () => fake({ text: "Files an issue." }),
    });
    const dialog = fake({ role: (role) => (role === "radiogroup" ? group : absent()) });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerOutcomeState()).resolves.toEqual({
      options: [
        { label: "Create issues", checked: true },
        { label: "Apply fix", checked: false },
      ],
      help: "Files an issue.",
    });
  });

  it("reads pod options and reports the disabled picker", async () => {
    const options = fake({
      items: [fake({ text: "Project default pod", attribute: "" }), fake({ text: "Rig (default)", attribute: "p1" })],
    });
    const select = fake({ input: "p1", disabled: true, locate: () => options });
    const dialog = fake({ byLabel: (label) => (label === "Pod" ? select : absent()) });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerPodOptions()).resolves.toEqual([
      { value: "", label: "Project default pod", selected: false },
      { value: "p1", label: "Rig (default)", selected: true },
    ]);
    await expect(driver.schedulerPodDisabled()).resolves.toBe(true);
  });
});

describe("install detail", () => {
  it("opens the detail address", async () => {
    const goto = vi.fn(async () => null);
    const page = pageOf({
      goto,
      getByRole: () => fake({ items: [fake()] }),
      getByText: () => fake({ items: [fake()] }),
    });
    await new WebNewDriver(page).schedulerOpenProjectBinding("acme", "p1", "b1");
    expect(goto).toHaveBeenCalledWith("/acme/projects/p1/schedulers/b1");
  });

  it("reads the header facts", async () => {
    const header = fake({
      byText: (text) => (text === "builtin" ? fake({ count: 1 }) : absent()),
      role: (role) => (role === "heading" ? fake({ text: "Nightly" }) : absent()),
      locate: (selector) => (selector === "code" ? fake({ text: "nightly" }) : absent()),
    });
    const section = fake({
      locate: (selector) => (selector === "xpath=preceding-sibling::header[1]" ? header : absent()),
    });
    const page = pageOf({
      getByRole: () => fake({ locate: () => section }),
    });
    await expect(new WebNewDriver(page).schedulerBindingHeader()).resolves.toEqual({
      name: "Nightly",
      handle: "nightly",
      badges: ["builtin"],
      workspaceLinkVisible: false,
      editVisible: false,
      uninstallVisible: false,
    });
  });

  it("reads config rows from the following definition list", async () => {
    const terms = [
      fake({ content: "Schedule", locate: () => fake({ text: "Every day" }) }),
      fake({ content: "Time zone", locate: () => fake({ text: "UTC" }) }),
    ];
    const labels = fake({ items: terms });
    const grid = fake({
      locate: (selector) => (selector === ":scope > div > dt" ? labels : fake({ text: "Every day" })),
    });
    const heading = fake({ locate: (selector) => (selector === "xpath=following-sibling::dl[1]" ? grid : absent()) });
    const page = pageOf({ getByRole: () => heading });
    await expect(new WebNewDriver(page).schedulerBindingConfig()).resolves.toEqual([
      { label: "Schedule", value: "Every day" },
      { label: "Time zone", value: "UTC" },
    ]);
  });

  it("reads the error banner only when present", async () => {
    await expect(new WebNewDriver(pageOf()).schedulerBindingLastError()).resolves.toBeNull();
    const mark = fake({
      count: 1,
      locate: () => fake({ locate: (selector) => (selector === "pre" ? fake({ text: "boom" }) : absent()) }),
    });
    const page = pageOf({ getByText: (text) => (String(text) === "Last error" ? mark : absent()) });
    await expect(new WebNewDriver(page).schedulerBindingLastError()).resolves.toBe("boom");
  });

  it("reads revealed state and toggles the prompt", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "button" && String(options?.["name"] ?? "") === "Hide resolved prompt"
          ? fake({ count: 1, visible: true, click })
          : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerBindingPromptState()).resolves.toEqual({ toggleVisible: true, revealed: true });
    await driver.schedulerBindingPromptToggle();
    expect(click).toHaveBeenCalled();
  });

  it("reads run rows, the empty sentence and the pager", async () => {
    const cells = ["Jan 1", "Jan 2", "Completed", "1m 2s", "default", "ok"].map((text) => fake({ text }));
    const cellsNode = fake({ items: cells });
    const row = fake({
      items: cells,
      locate: (selector) => (selector === "td[colspan]" ? absent() : cellsNode),
    });
    const rows = fake({ items: [row], locate: () => absent() });
    const section = fake({
      locate: (selector) => (selector === "tbody tr" ? rows : absent()),
      byText: () => fake({ count: 1, text: "Page 1 of 3", locate: () => fake() }),
    });
    const page = pageOf({
      getByRole: () => fake({ locate: (selector) => (selector === "xpath=ancestor::section[1]" ? section : absent()) }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerBindingRuns()).resolves.toEqual([
      { started: "Jan 1", ended: "Jan 2", status: "Completed", duration: "1m 2s", pod: "default", result: "ok" },
    ]);
    await expect(driver.schedulerBindingRunsEmpty()).resolves.toBeNull();
    await expect(driver.schedulerBindingRunsPager()).resolves.toEqual({
      text: "Page 1 of 3",
      prevDisabled: false,
      nextDisabled: false,
    });
  });

  it("reports the not-available notice and the detail switch", async () => {
    const page = pageOf({
      getByText: () => fake({ count: 1, visible: true }),
      getByRole: () => fake({ attribute: "false" }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerBindingRemovedVisible()).resolves.toBe(true);
    await expect(driver.schedulerBindingToggleState()).resolves.toEqual({ checked: false, disabled: false });
  });
});

describe("notifications", () => {
  it("reads every visible toast with text", async () => {
    const cards = fake({ items: [fake({ text: "Scheduler installed\nNightly is live." }), fake({ text: "" })] });
    const page = pageOf({ locator: () => cards });
    await expect(new WebNewDriver(page).schedulerVisibleToasts()).resolves.toEqual([
      { title: "Scheduler installed", message: "Nightly is live." },
    ]);
  });
});

describe("recurrence builder", () => {
  interface ChipReading {
    label: string;
    pressed: boolean;
  }

  function weekdayGroup(readings: ChipReading[]): FakeLocator {
    const chips = readings.map((reading) =>
      fake({
        count: 1,
        getAttribute: async (name: string) =>
          name === "aria-label" ? reading.label : name === "aria-pressed" ? String(reading.pressed) : null,
      })
    );
    return fake({ count: 1, role: (role: string) => (role === "button" ? fake({ items: chips }) : absent()) });
  }

  function endsGroup(mode: string): FakeLocator {
    return fake({
      count: 1,
      role: (role: string) =>
        role === "radio" ? fake({ count: 1, first: () => fake({ attribute: mode }) }) : absent(),
    });
  }

  it("reads every control and the live sentence", async () => {
    const raw = "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR";
    const dialog = fake({
      role: (role: string, options?: Record<string, unknown>) => {
        if (role !== "group") return absent();
        return String(options?.["name"] ?? "") === "Repeat on"
          ? weekdayGroup([
              { label: "Sunday", pressed: false },
              { label: "Monday", pressed: true },
              { label: "Friday", pressed: true },
            ])
          : endsGroup("after");
      },
      byLabel: (label: string) => {
        const values: Record<string, string> = {
          "Repeat unit": "week",
          "Repeat interval": "2",
          "Monthly pattern": "weekday",
          "Weekday ordinal": "2",
          "Weekday of the month": "MO",
          "End date": "2027-01-01",
          "Number of occurrences": "10",
          "Recurrence (RRULE)": raw,
        };
        const value = values[label];
        if (value === undefined) return absent();
        if (label !== "Repeat unit") return fake({ count: 1, visible: true, input: value });
        // The unit select exposes its option list so the scenario can prove
        // no sub-daily cadence is offered.
        return fake({
          count: 1,
          visible: true,
          input: value,
          locate: (selector: string) =>
            selector === "option"
              ? fake({
                  evaluateAll: async (fn: (nodes: unknown[]) => unknown) =>
                    fn(["none", "day", "week", "month", "year"].map((unit) => ({ value: unit }))),
                })
              : absent(),
        });
      },
      locate: (selector: string) => {
        const line = fake({ text: "every 2 weeks on Monday, Friday" });
        line.options.locate = () => line;
        return selector === "p" ? line : absent();
      },
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const facts = await new WebNewDriver(page).schedulerRecurrenceFacts();
    expect(facts).toEqual({
      visible: true,
      interval: "2",
      unit: "week",
      unitOptions: ["none", "day", "week", "month", "year"],
      weekdays: [
        { name: "Sunday", pressed: false },
        { name: "Monday", pressed: true },
        { name: "Friday", pressed: true },
      ],
      monthlyPattern: "weekday",
      dayOfMonth: null,
      ordinal: "2",
      weekdayOfMonth: "MO",
      ends: "after",
      endDate: "2027-01-01",
      occurrences: "10",
      rawVisible: true,
      rawValue: raw,
      advancedVisible: false,
      sentence: "every 2 weeks on Monday, Friday",
    });
  });

  it("reports an absent builder but still reads the raw field", async () => {
    const dialog = fake({
      role: () => absent(),
      byLabel: (label: string) =>
        label === "Recurrence (RRULE)" ? fake({ count: 1, visible: true, input: "FREQ=DAILY" }) : absent(),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerRecurrenceFacts()).resolves.toMatchObject({
      visible: false,
      rawVisible: true,
      rawValue: "FREQ=DAILY",
      weekdays: [],
      ends: null,
    });
  });

  it("acts on each control", async () => {
    const fill = vi.fn(async () => undefined);
    const selectOption = vi.fn(async () => undefined);
    const click = vi.fn(async () => undefined);
    const control = fake({ count: 1, fill, selectOption, click });
    const group = fake({ role: (role: string) => (role === "button" || role === "radio" ? control : absent()) });
    const dialog = fake({ byLabel: () => control, role: () => group, locate: () => absent() });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await driver.schedulerRecurrenceAct({ kind: "interval", value: 3 });
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "month" });
    await driver.schedulerRecurrenceAct({ kind: "weekday", value: "Friday" });
    await driver.schedulerRecurrenceAct({ kind: "monthly", value: "weekday" });
    await driver.schedulerRecurrenceAct({ kind: "day", value: 15 });
    await driver.schedulerRecurrenceAct({ kind: "ordinal", value: "2" });
    await driver.schedulerRecurrenceAct({ kind: "weekdayOfMonth", value: "MO" });
    await driver.schedulerRecurrenceAct({ kind: "ends", value: "after" });
    await driver.schedulerRecurrenceAct({ kind: "endDate", value: "2027-01-01" });
    await driver.schedulerRecurrenceAct({ kind: "occurrences", value: 12 });
    expect(fill.mock.calls.map((call) => call[0])).toEqual(["3", "15", "2027-01-01", "12"]);
    expect(selectOption.mock.calls.map((call) => call[0])).toEqual(["month", "weekday", "2", "MO"]);
    expect(click).toHaveBeenCalledTimes(2);
  });

  it("resolves the Never and On-date ends labels", async () => {
    const click = vi.fn(async () => undefined);
    const radio = fake({ count: 1, click });
    const group = fake({ role: (role: string) => (role === "radio" ? radio : absent()) });
    const dialog = fake({ role: () => group });
    const driver = new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => dialog }) }));
    await driver.schedulerRecurrenceAct({ kind: "ends", value: "never" });
    await driver.schedulerRecurrenceAct({ kind: "ends", value: "on" });
    expect(click).toHaveBeenCalledTimes(2);
  });

  it("opens the raw editor through the advanced disclosure", async () => {
    let expanded = false;
    const toggle = fake({
      count: 1,
      click: async () => {
        expanded = true;
      },
      getAttribute: async (name: string) => (name === "aria-expanded" && expanded ? "true" : "false"),
    });
    const raw = fake({ count: 1, visible: false, fill: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: (label: string) => (label === "Recurrence (RRULE)" ? raw : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerRecurrenceAdvanced()).resolves.toBe(true);
    expect(toggle.click).toHaveBeenCalledTimes(1);
    await driver.schedulerRecurrenceRaw("FREQ=DAILY;COUNT=2");
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY;COUNT=2");
  });

  it("does not touch the disclosure when the raw field is already open", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 1, visible: true, fill: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: (label: string) => (label === "Recurrence (RRULE)" ? raw : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerRecurrenceRaw("FREQ=DAILY");
    expect(toggle.click).not.toHaveBeenCalled();
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY");
  });

  it("reveals the rule disclosure before filling the schedule", async () => {
    let expanded = false;
    const fill = vi.fn(async () => undefined);
    const toggle = fake({
      count: 1,
      click: async () => {
        expanded = true;
      },
      getAttribute: async (name: string) => (name === "aria-expanded" && expanded ? "true" : "false"),
    });
    const scheduleField = (label: string) =>
      label === "Recurrence (RRULE)"
        ? fake({
            count: 1,
            get visible() {
              return expanded;
            },
            fill,
          })
        : fake({ count: 1, fill: vi.fn(async () => undefined), selectOption: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: scheduleField,
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerProjectEditFill({ rrule: "FREQ=DAILY;COUNT=2" });
    expect(toggle.click).toHaveBeenCalledTimes(1);
    expect(fill).toHaveBeenCalledWith("FREQ=DAILY;COUNT=2");
  });

  it("fills the rule directly when the disclosure is already open", async () => {
    const fill = vi.fn(async () => undefined);
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? toggle : absent()),
      byLabel: (label: string) =>
        label === "Recurrence (RRULE)"
          ? fake({ count: 1, visible: true, fill })
          : fake({ count: 1, fill: vi.fn(async () => undefined), selectOption: vi.fn(async () => undefined) }),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerProjectEditFill({ rrule: "FREQ=DAILY" });
    expect(toggle.click).not.toHaveBeenCalled();
    expect(fill).toHaveBeenCalledWith("FREQ=DAILY");
  });

  it("treats a forced raw rule as already advanced", async () => {
    const dialog = fake({
      role: () => absent(),
      byLabel: (label: string) => (label === "Recurrence (RRULE)" ? fake({ count: 1, visible: true }) : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerRecurrenceAdvanced()).resolves.toBe(true);
  });

  it("leaves the oracle's builder actions target-only", async () => {
    const { WebDriver } = await import("./web");
    const driver = new WebDriver(pageOf({ getByRole: () => fake({ last: () => fake({ locate: () => absent() }) }) }));
    await expect(driver.schedulerRecurrenceAct({ kind: "unit", value: "week" })).rejects.toThrow(
      "recurrence builder is a web_new target"
    );
  });
});

describe("calendar surface", () => {
  function calendarPage(overrides: {
    week?: FakeLocator;
    month?: FakeLocator;
    ready?: FakeLocator;
    role?: (role: string, options?: Record<string, unknown>) => FakeLocator;
    title?: string;
    goto?: ReturnType<typeof vi.fn>;
  }): Page {
    return pageOf({
      goto: overrides.goto ?? vi.fn(async () => null),
      getByTestId: (id: string) => {
        if (id === "calendar-week-view") return overrides.week ?? absent();
        if (id === "calendar-month-view") return overrides.month ?? absent();
        if (id === "calendar-period-label") return fake({ text: overrides.title ?? "October 2026" });
        return absent();
      },
      getByRole:
        overrides.role ??
        ((role: string) =>
          role === "region"
            ? fake({ count: 1, locate: () => absent() })
            : role === "heading"
              ? fake({ count: 0 })
              : absent()),
    });
  }

  it("opens the calendar address and settles on a rendered body", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => undefined);
    const page = calendarPage({ month: fake({ count: 1 }), ready: fake({ count: 1, waitFor }), goto });
    const driver = new WebNewDriver(page);
    await driver.schedulerOpenProjectCalendar("acme", "p1");
    expect(goto).toHaveBeenCalledWith("/acme/projects/p1/schedulers/calendar");
  });

  it("reads the mounted grid as the active view", async () => {
    const week = fake({ count: 1 });
    await expect(new WebNewDriver(calendarPage({ week })).schedulerCalendarView()).resolves.toBe("week");
    await expect(new WebNewDriver(calendarPage({})).schedulerCalendarView()).resolves.toBe("month");
  });

  it("switches the view and waits for the target grid", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const month = fake({ count: 1, waitFor });
    const page = calendarPage({
      month,
      role: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Month" ? fake({ click }) : absent(),
    });
    await new WebNewDriver(page).schedulerCalendarSetView("month");
    expect(click).toHaveBeenCalled();
    expect(waitFor).toHaveBeenCalled();

    // The week side picks its own button and settles on the week grid.
    const weekWaitFor = vi.fn(async () => undefined);
    const week = fake({ count: 1, waitFor: weekWaitFor });
    const weekPage = calendarPage({
      week,
      role: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Week" ? fake({ click }) : absent(),
    });
    await new WebNewDriver(weekPage).schedulerCalendarSetView("week");
    expect(weekWaitFor).toHaveBeenCalled();
  });

  it("steps the month view until its caption changes", async () => {
    let label = "October 2026";
    const click = vi.fn(async () => {
      label = "November 2026";
    });
    const page = pageOf({
      getByTestId: (id: string) => (id === "calendar-period-label" ? fake({ innerText: async () => label }) : absent()),
      getByRole: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Next" ? fake({ click }) : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarTitle()).resolves.toBe("October 2026");
    await driver.schedulerCalendarStep("next");
    expect(click).toHaveBeenCalled();

    // The month view's other direction clicks its own control.
    label = "December 2026";
    const previous = vi.fn(async () => {
      label = "November 2026";
    });
    const backward = pageOf({
      getByTestId: (id: string) => (id === "calendar-period-label" ? fake({ innerText: async () => label }) : absent()),
      getByRole: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Previous" ? fake({ click: previous }) : absent(),
    });
    await new WebNewDriver(backward).schedulerCalendarStep("prev");
    expect(previous).toHaveBeenCalled();
  });

  it("steps the week view by its day caption, even within one month", async () => {
    let day = "Sun 25";
    const click = vi.fn(async () => {
      day = "Sun 1";
    });
    const week = fake({
      count: 1,
      testId: (id: string) =>
        id === "calendar-week-day-head" ? fake({ locate: () => fake({ text: day }) }) : absent(),
    });
    const page = pageOf({
      getByTestId: (id: string) => (id === "calendar-week-view" ? week : absent()),
      getByRole: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Next" ? fake({ click }) : absent(),
    });
    await new WebNewDriver(page).schedulerCalendarStep("next");
    expect(click).toHaveBeenCalled();
  });

  it("jumps home to the current month-year caption", async () => {
    const current = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(new Date());
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "calendar-period-label"
          ? fake({ text: current })
          : id === "calendar-month-view"
            ? fake({ count: 1 })
            : absent(),
      getByRole: (role, options) =>
        role === "button" && String(options?.["name"] ?? "") === "Today" ? fake({ click }) : absent(),
    });
    await new WebNewDriver(page).schedulerCalendarToday();
    expect(click).toHaveBeenCalled();
  });

  it("reports the empty and truncation states", async () => {
    const page = pageOf({
      getByRole: (role: string) => (role === "heading" ? fake({ count: 1, visible: true }) : absent()),
      getByText: () => fake({ count: 1, visible: true }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarEmptyVisible()).resolves.toBe(true);
    await expect(driver.schedulerCalendarTruncatedVisible()).resolves.toBe(true);
    const blank = new WebNewDriver(pageOf());
    await expect(blank.schedulerCalendarEmptyVisible()).resolves.toBe(false);
    await expect(blank.schedulerCalendarTruncatedVisible()).resolves.toBe(false);
  });

  it("reads month cells by test id, one block per titled button", async () => {
    const block = fake({
      locate: (selector: string) =>
        selector === ":scope > span" ? fake({ items: [fake({ text: "9:00 AM" }), fake({ text: "Alpha" })] }) : absent(),
      attribute: "Alpha · 9:00 AM",
      evaluate: "rgb(10, 20, 30)",
    });
    const cell = fake({
      testId: (id: string) => (id === "calendar-month-day-number" ? fake({ text: "18" }) : absent()),
      locate: (selector: string) => (selector === "button[title]" ? fake({ items: [block] }) : absent()),
    });
    const month = fake({ testId: (id: string) => (id === "calendar-month-day" ? fake({ items: [cell] }) : absent()) });
    const page = pageOf({
      getByTestId: (id: string) => (id === "calendar-month-view" ? month : absent()),
    });
    await expect(new WebNewDriver(page).schedulerCalendarMonthBlocks()).resolves.toEqual([
      { day: "18", time: "9:00 AM", name: "Alpha", title: "Alpha · 9:00 AM", background: "rgb(10, 20, 30)" },
    ]);
  });

  it("reads month overflow from untitled controls and today from its marker", async () => {
    const cell = fake({
      locate: (selector: string) =>
        selector === "button:not([title])"
          ? fake({ items: [fake({ text: "+ 20 more" }), fake({ text: " " })] })
          : absent(),
    });
    const month = fake({
      testId: (id: string) => (id === "calendar-month-day" ? fake({ items: [cell] }) : absent()),
      locate: (selector: string) =>
        selector === '[data-testid="calendar-month-day"][data-today="true"]' ? fake({ count: 1 }) : absent(),
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "calendar-month-view" ? month : absent()) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarMonthOverflow()).resolves.toEqual(["+ 20 more"]);
    await expect(driver.schedulerCalendarMonthTodayMarked()).resolves.toBe(true);
    const quiet = pageOf({ getByTestId: () => fake({ testId: () => absent(), locate: () => absent() }) });
    await expect(new WebNewDriver(quiet).schedulerCalendarMonthTodayMarked()).resolves.toBe(false);
  });

  it("pairs week day headers with their lanes and marks today", async () => {
    const alpha = fake({
      locate: (selector: string) =>
        selector === ":scope > span" ? fake({ items: [fake({ text: "9:00 AM" }), fake({ text: "Alpha" })] }) : absent(),
      attribute: "Alpha · 9:00 AM",
      evaluate: "rgb(1, 2, 3)",
    });
    const beta = fake({
      locate: (selector: string) =>
        selector === ":scope > span" ? fake({ items: [fake({ text: "10:00 AM" }), fake({ text: "Beta" })] }) : absent(),
      attribute: "Beta · 10:00 AM",
      evaluate: "rgb(4, 5, 6)",
    });
    const laneOne = fake({
      locate: (selector: string) => (selector === "button[title]" ? fake({ items: [alpha] }) : absent()),
    });
    const laneTwo = fake({
      locate: (selector: string) => (selector === "button[title]" ? fake({ items: [beta] }) : absent()),
    });
    const week = fake({
      testId: (id: string) => {
        if (id === "calendar-week-day-head")
          return fake({
            items: [
              fake({
                locate: (selector: string) =>
                  selector === "span" ? fake({ items: [fake({ text: "Sun 18" })] }) : absent(),
              }),
              fake({
                locate: (selector: string) =>
                  selector === "span" ? fake({ items: [fake({ text: "Mon 19" })] }) : absent(),
              }),
            ],
          });
        if (id === "calendar-week-day") return fake({ items: [laneOne, laneTwo] });
        if (id === "calendar-week-today") return fake({ count: 1 });
        return absent();
      },
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "calendar-week-view" ? week : absent()) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarWeekBlocks()).resolves.toEqual([
      { day: "Sun 18", time: "9:00 AM", name: "Alpha", title: "Alpha · 9:00 AM", background: "rgb(1, 2, 3)" },
      { day: "Mon 19", time: "10:00 AM", name: "Beta", title: "Beta · 10:00 AM", background: "rgb(4, 5, 6)" },
    ]);
    await expect(driver.schedulerCalendarWeekTodayMarked()).resolves.toBe(true);
    const noToday = pageOf({ getByTestId: () => fake({ testId: () => absent() }) });
    await expect(new WebNewDriver(noToday).schedulerCalendarWeekTodayMarked()).resolves.toBe(false);
  });

  it("reads a lane whose day has no header as unlabelled", async () => {
    const block = fake({
      locate: (selector: string) =>
        selector === ":scope > span" ? fake({ items: [fake({ text: "9:00 AM" }), fake({ text: "Alpha" })] }) : absent(),
      attribute: "Alpha · 9:00 AM",
      evaluate: "rgb(1, 2, 3)",
    });
    const lane = fake({
      locate: (selector: string) => (selector === "button[title]" ? fake({ items: [block] }) : absent()),
    });
    const week = fake({
      testId: (id: string) => {
        if (id === "calendar-week-day-head") return fake({ items: [] });
        if (id === "calendar-week-day") return fake({ items: [lane] });
        return absent();
      },
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "calendar-week-view" ? week : absent()) });
    await expect(new WebNewDriver(page).schedulerCalendarWeekBlocks()).resolves.toEqual([
      { day: "", time: "9:00 AM", name: "Alpha", title: "Alpha · 9:00 AM", background: "rgb(1, 2, 3)" },
    ]);
  });

  it("reads the now-line offset or reports its absence", async () => {
    const line = fake({ count: 1, evaluate: "123.5px" });
    const week = fake({ testId: (id: string) => (id === "calendar-now-line" ? line : absent()) });
    const page = pageOf({ getByTestId: (id: string) => (id === "calendar-week-view" ? week : absent()) });
    await expect(new WebNewDriver(page).schedulerCalendarTimeLineTop()).resolves.toBe(123.5);
    const missing = pageOf({ getByTestId: () => fake({ testId: () => fake({ count: 0 }) }) });
    await expect(new WebNewDriver(missing).schedulerCalendarTimeLineTop()).resolves.toBeNull();
    const unparsable = fake({ count: 1, evaluate: "auto" });
    const broken = pageOf({
      getByTestId: (id: string) => (id === "calendar-week-view" ? fake({ testId: () => unparsable }) : absent()),
    });
    await expect(new WebNewDriver(broken).schedulerCalendarTimeLineTop()).resolves.toBeNull();
  });

  it("clicks a block by scheduler name inside the calendar region", async () => {
    const click = vi.fn(async () => undefined);
    const matched = fake({ first: () => fake({ click }) });
    const scope = fake({
      locate: (selector: string) => (selector === "button[title]" ? fake({ filter: () => matched }) : absent()),
    });
    const drawer = fake({ waitFor: vi.fn(async () => undefined) });
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "region" ? scope : role === "dialog" && options?.["name"] !== undefined ? drawer : absent(),
    });
    await new WebNewDriver(page).schedulerCalendarClickBlock("Alpha");
    expect(click).toHaveBeenCalled();
  });

  it("reads block fields through the two spans and the fill", async () => {
    const block = fake({
      locate: (selector: string) =>
        selector === ":scope > span" ? fake({ items: [fake({ text: "9:00 AM" }), fake({ text: "Alpha" })] }) : absent(),
      attribute: "Alpha · 9:00 AM",
      evaluate: "rgb(9, 9, 9)",
    });
    const lane = fake({ locate: () => fake({ items: [block] }) });
    const week = fake({
      testId: (id: string) =>
        id === "calendar-week-day-head"
          ? fake({ items: [fake({ locate: () => fake({ text: "Sun 18" }) })] })
          : id === "calendar-week-day"
            ? fake({ items: [lane] })
            : absent(),
    });
    const page = pageOf({ getByTestId: (id: string) => (id === "calendar-week-view" ? week : absent()) });
    await expect(new WebNewDriver(page).schedulerCalendarWeekBlocks()).resolves.toEqual([
      { day: "Sun 18", time: "9:00 AM", name: "Alpha", title: "Alpha · 9:00 AM", background: "rgb(9, 9, 9)" },
    ]);
  });

  it("reports no draggable blocks and no export controls", async () => {
    const titled = fake({ attribute: "Alpha · 9:00 AM", text: "Export Alpha", visible: true });
    const exportButton = fake({ text: "Export", visible: true, attribute: null });
    const hidden = fake({ text: "Download", visible: false, attribute: null });
    const scope = fake({
      locate: (selector: string) => (selector === 'button[draggable="true"]' ? fake({ count: 0 }) : absent()),
      role: (role: string) =>
        role === "button" ? fake({ all: async () => [titled, exportButton, hidden] }) : fake({ all: async () => [] }),
    });
    const page = pageOf({ getByRole: (role: string) => (role === "region" ? scope : absent()) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarAnyDraggable()).resolves.toBe(false);
    await expect(driver.schedulerCalendarExportControls()).resolves.toEqual(["Export"]);
  });
});

describe("calendar visibility rail", () => {
  function railWith(rows: { name: string; checked: boolean }[], overrides: Partial<Record<string, unknown>> = {}) {
    const boxes = rows.map((row) =>
      fake({
        checked: row.checked,
        locate: (selector: string) =>
          selector === "xpath=.."
            ? fake({
                locate: (inner: string) =>
                  inner === "span" ? fake({ items: [fake({ text: "dot" }), fake({ text: row.name })] }) : absent(),
              })
            : absent(),
      })
    );
    return fake({
      count: 1,
      visible: true,
      role: (role: string) => (role === "button" ? fake({ click: overrides["buttonClick"] ?? vi.fn() }) : absent()),
      locate: (selector: string) =>
        selector === 'input[type="checkbox"]'
          ? fake({ items: boxes })
          : selector === "label"
            ? fake({ locate: () => boxes[0] ?? absent() })
            : absent(),
      ...overrides,
    });
  }

  it("reads visibility and one row per checkbox, in order", async () => {
    const page = pageOf({
      getByRole: (role: string) =>
        role === "complementary"
          ? railWith([
              { name: "Alpha", checked: true },
              { name: "Beta", checked: false },
            ])
          : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerRailVisible()).resolves.toBe(true);
    await expect(driver.schedulerRailRows()).resolves.toEqual([
      { name: "Alpha", checked: true },
      { name: "Beta", checked: false },
    ]);
    const hidden = new WebNewDriver(pageOf());
    await expect(hidden.schedulerRailVisible()).resolves.toBe(false);
    await expect(hidden.schedulerRailRows()).resolves.toEqual([]);
  });

  it("toggles one row and drives show-all / hide-all", async () => {
    const boxClick = vi.fn(async () => undefined);
    const buttonClick = vi.fn(async () => undefined);
    const box = fake({ checked: true, click: boxClick });
    const rail = fake({
      locate: (selector: string) => (selector === "label" ? fake({ locate: () => box }) : absent()),
      role: (role: string) => (role === "button" ? fake({ click: buttonClick }) : absent()),
    });
    const page = pageOf({ getByRole: (role: string) => (role === "complementary" ? rail : absent()) });
    const driver = new WebNewDriver(page);
    await driver.schedulerRailToggle("Alpha");
    await driver.schedulerRailShowAll();
    await driver.schedulerRailHideAll();
    expect(boxClick).toHaveBeenCalled();
    expect(buttonClick).toHaveBeenCalledTimes(2);
  });

  it("loads a fresh tab with the hidden choice applied, then closes it", async () => {
    const close = vi.fn(async () => undefined);
    const box = fake({ checked: false, waitFor: vi.fn(async () => undefined) });
    const freshRail = fake({ locate: () => fake({ locate: () => box }) });
    const fresh = pageOf({
      goto: vi.fn(async () => null),
      getByRole: (role: string) => (role === "complementary" ? freshRail : absent()),
      close,
    });
    const current = pageOf({
      locate: () => fake({ locate: () => fake({ click: vi.fn() }) }),
      getByRole: (role: string) =>
        role === "complementary" ? fake({ locate: () => fake({ locate: () => fake({ click: vi.fn() }) }) }) : absent(),
      url: () => "http://localhost/acme/projects/p1/schedulers/calendar",
      context: () => ({ newPage: vi.fn(async () => fresh) }),
    });
    await expect(new WebNewDriver(current).schedulerRailCrossTabPersists("acme", "p1", "Alpha")).resolves.toBe(true);
    expect(close).toHaveBeenCalled();
  });

  it("reports a rail that stays visible on a narrow viewport as not hidden", async () => {
    const waitForTimeout = vi.fn(async () => undefined);
    const page = pageOf({
      waitForTimeout,
      getByRole: (role: string) => (role === "complementary" ? fake({ count: 1, visible: true }) : absent()),
    });
    await expect(new WebNewDriver(page).schedulerRailNarrowHidden()).resolves.toBe(false);
    expect(waitForTimeout).toHaveBeenCalled();
  });

  it("hides the rail below the desktop breakpoint and restores the viewport", async () => {
    let narrow = false;
    const setViewportSize = vi.fn(async (size: { width: number }) => {
      narrow = size.width < 768;
    });
    const page = pageOf({
      setViewportSize,
      getByRole: (role: string) =>
        role === "complementary" ? fake({ count: narrow ? 0 : 1, visible: !narrow }) : absent(),
    });
    await expect(new WebNewDriver(page).schedulerRailNarrowHidden()).resolves.toBe(true);
    expect(setViewportSize).toHaveBeenCalledWith({ width: 500, height: 800 });
    expect(setViewportSize).toHaveBeenCalledWith({ width: 1280, height: 720 });
  });
});

describe("occurrence panel", () => {
  function drawer(linkClick = vi.fn(async () => undefined)) {
    const detail = fake({
      locate: (selector: string) =>
        selector === ":scope > div"
          ? fake({ items: [fake({ content: "When" }), fake({ text: "Oct 25, 2026, 9:00 AM" })] })
          : absent(),
    });
    const closeClick = vi.fn(async () => undefined);
    return {
      closeClick,
      node: fake({
        count: 1,
        testId: (id: string) =>
          id === "occurrence-detail"
            ? fake({ items: [detail] })
            : id === "occurrence-status"
              ? fake({ content: "Scheduled" })
              : absent(),
        role: (role: string, options?: Record<string, unknown>) => {
          if (role === "heading") return fake({ text: "Alpha" });
          if (role === "link")
            return options?.["name"] === undefined
              ? fake({ items: [fake({ text: "View scheduler →" })] })
              : fake({ click: linkClick });
          if (role === "button") {
            const name = String(options?.["name"] ?? "");
            if (name === "Close") return fake({ click: closeClick, first: () => fake({ click: closeClick }) });
            if (name === "Edit binding") return fake({ count: 1, visible: true, click: vi.fn(async () => undefined) });
            return absent();
          }
          return absent();
        },
        waitFor: vi.fn(async () => undefined),
      }),
    };
  }

  it("reads open state, heading, detail rows and links", async () => {
    const panel = drawer();
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "dialog" && options?.["name"] !== undefined ? panel.node : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerDrawerOpen()).resolves.toBe(true);
    await expect(driver.schedulerDrawerHeading()).resolves.toEqual({ state: "Scheduled", name: "Alpha" });
    await expect(driver.schedulerDrawerRows()).resolves.toEqual([{ label: "When", value: "Oct 25, 2026, 9:00 AM" }]);
    await expect(driver.schedulerDrawerLinks()).resolves.toEqual(["View scheduler →"]);
    await expect(driver.schedulerDrawerEditVisible()).resolves.toBe(true);
  });

  it("closes through the dismiss control and reports a closed panel", async () => {
    const panel = drawer();
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "dialog" && options?.["name"] !== undefined ? panel.node : absent(),
    });
    await new WebNewDriver(page).schedulerDrawerClose();
    expect(panel.closeClick).toHaveBeenCalled();
    await expect(new WebNewDriver(pageOf()).schedulerDrawerOpen()).resolves.toBe(false);
  });

  it("opens the edit dialog from the panel and follows the install link", async () => {
    const linkClick = vi.fn(async () => undefined);
    const panel = drawer(linkClick);
    const dialogWaitFor = vi.fn(async () => undefined);
    const dialog = fake({
      role: () => fake({ waitFor: dialogWaitFor }),
      byLabel: () => fake({ waitFor: dialogWaitFor }),
    });
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) => {
        if (role === "dialog" && options?.["name"] !== undefined) return panel.node;
        if (role === "dialog") return dialog;
        if (role === "heading") return fake({ waitFor: vi.fn(async () => undefined) });
        return absent();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.schedulerDrawerEdit();
    expect(dialogWaitFor).toHaveBeenCalled();
    await driver.schedulerDrawerViewScheduler();
    expect(linkClick).toHaveBeenCalled();
  });
});

describe("scheduler section tabs", () => {
  function tabsPage(active: string) {
    return pageOf({
      getByRole: (role: string) => {
        if (role === "navigation")
          return fake({
            role: (inner: string, options?: Record<string, unknown>) => {
              if (inner !== "link") return absent();
              const name = String(options?.["name"] ?? "");
              return fake({ attribute: name === active ? "page" : null, click: vi.fn(async () => undefined) });
            },
          });
        if (role === "columnheader") return fake({ count: 1, waitFor: vi.fn(async () => undefined) });
        if (role === "region") return fake({ count: 1, visible: true });
        return absent();
      },
      getByTestId: (id: string) =>
        id === "calendar-week-view" ? fake({ count: 1, waitFor: vi.fn(async () => undefined) }) : absent(),
      goto: vi.fn(async () => null),
      waitForURL: vi.fn(async () => undefined),
    });
  }

  it("reads which tab is current", async () => {
    await expect(new WebNewDriver(tabsPage("Calendar")).schedulerSectionTabs()).resolves.toEqual([
      { label: "List", active: false },
      { label: "Calendar", active: true },
    ]);
    await expect(new WebNewDriver(tabsPage("List")).schedulerSectionTabs()).resolves.toEqual([
      { label: "List", active: true },
      { label: "Calendar", active: false },
    ]);
  });

  it("lands the bare path on the calendar and follows both tabs", async () => {
    const page = tabsPage("Calendar");
    const driver = new WebNewDriver(page);
    await driver.schedulerOpenProjectSection("acme", "p1");
    expect(page.goto).toHaveBeenCalledWith("/acme/projects/p1/schedulers");
    expect(page.waitForURL).toHaveBeenCalled();
    await driver.schedulerSectionOpenTab("List");
    await driver.schedulerSectionOpenTab("Calendar");
  });
});

describe("run history export scan", () => {
  it("scans the run-history section for export-ish controls", async () => {
    const exportButton = fake({ text: "Download CSV", visible: true, attribute: null });
    const titled = fake({ text: "Download run", visible: true, attribute: "Run details" });
    const section = fake({
      role: (role: string) =>
        role === "button" ? fake({ all: async () => [exportButton, titled] }) : fake({ all: async () => [] }),
    });
    const page = pageOf({
      getByRole: (role: string) =>
        role === "heading"
          ? fake({ locate: (selector) => (selector === "xpath=ancestor::section[1]" ? section : absent()) })
          : absent(),
    });
    await expect(new WebNewDriver(page).schedulerRunsExportControls()).resolves.toEqual(["Download CSV"]);
  });
});

describe("calendar fallbacks and branch edges", () => {
  it("substitutes empty fields when a block cannot be inspected", async () => {
    const block = fake({ throws: true });
    const drawer = fake({ count: 0 });
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "calendar-week-view"
          ? fake({
              testId: (inner: string) =>
                inner === "calendar-week-day-head"
                  ? fake({ items: [fake({ locate: () => fake({ items: [fake({ text: "Sun 18" })] }) })] })
                  : inner === "calendar-week-day"
                    ? fake({ items: [fake({ locate: () => fake({ items: [block] }) })] })
                    : absent(),
            })
          : absent(),
      getByRole: (role: string) => (role === "dialog" ? drawer : absent()),
    });
    await expect(new WebNewDriver(page).schedulerCalendarWeekBlocks()).resolves.toEqual([
      { day: "Sun 18", time: "", name: "", title: "", background: "" },
    ]);
  });

  it("skips invisible, titled and blank controls in the export scan", async () => {
    const hidden = fake({ visible: false, text: "Export", attribute: null });
    const titled = fake({ visible: true, text: "Export run", attribute: "Run" });
    const blank = fake({ visible: true, text: "   ", attribute: null });
    const link = fake({ visible: true, text: "Download", attribute: null });
    const scope = fake({
      role: (role: string) =>
        role === "button" ? fake({ all: async () => [hidden, titled, blank] }) : fake({ all: async () => [link] }),
    });
    const page = pageOf({ getByRole: () => scope });
    await expect(new WebNewDriver(page).schedulerCalendarExportControls()).resolves.toEqual(["Download"]);
  });

  it("reports an unreadable caption, overflow and row values as empty", async () => {
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "calendar-period-label"
          ? fake({ throws: true })
          : id === "calendar-month-view"
            ? fake({
                testId: (inner: string) =>
                  inner === "calendar-month-day"
                    ? fake({
                        items: [
                          fake({
                            testId: () => fake({ throws: true }),
                            locate: (selector: string) =>
                              selector === "button[title]"
                                ? fake({ items: [fake({ throws: true })] })
                                : selector === "button:not([title])"
                                  ? fake({ items: [fake({ throws: true })] })
                                  : absent(),
                          }),
                        ],
                      })
                    : absent(),
              })
            : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCalendarTitle()).resolves.toBe("");
    await expect(driver.schedulerCalendarMonthBlocks()).resolves.toEqual([
      { day: "", time: "", name: "", title: "", background: "" },
    ]);
    await expect(driver.schedulerCalendarMonthOverflow()).resolves.toEqual([]);
  });
});

describe("scheduler catalog", () => {
  it("opens the workspace catalog and settles on the table", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => undefined);
    const absentTail = fake({ count: 0, last: () => fake({ count: 0 }) });
    const table = fake({ count: 1, waitFor, or: () => absentTail });
    const page = pageOf({
      goto,
      locator: (selector: string) => (selector === "tbody" ? table : absent()),
    });
    await new WebNewDriver(page).schedulerOpenCatalog("acme");
    expect(goto).toHaveBeenCalledWith("/acme/schedulers");
  });

  it("reads row name, handle, origin, installs, status and update", async () => {
    const name = fake({ text: "Weekly report" });
    const handle = fake({ text: "weekly-report" });
    const paragraphs = fake({ items: [name, handle], nth: (index: number) => [name, handle][index] ?? absent() });
    const first = fake({ locate: (selector: string) => (selector === "p" ? paragraphs : absent()) });
    const cells = [
      first,
      fake({ text: "Built in" }),
      fake({ text: "2" }),
      fake({ text: "Enabled" }),
      fake({ text: "Oct 2, 2026" }),
    ];
    const dataRow = fake({ locate: (selector: string) => (selector === "td" ? fake({ items: cells }) : absent()) });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="scheduler-row"]' ? fake({ items: [dataRow] }) : absent(),
    });
    await expect(new WebNewDriver(page).schedulerCatalogRows()).resolves.toEqual([
      {
        name: "Weekly report",
        handle: "weekly-report",
        origin: "Built-in",
        installs: "2",
        status: "Enabled",
        updated: "Oct 2, 2026",
      },
    ]);
  });

  it("keeps manifest origin and an empty catalog off the built-in bridge", async () => {
    const manifestRow = fake({
      locate: (selector: string) =>
        selector === "td"
          ? fake({
              items: [
                fake({
                  locate: (selector: string) =>
                    selector === "p"
                      ? fake({ items: [fake({ text: "Imported" }), fake({ text: "imported" })] })
                      : absent(),
                }),
                fake({ text: "Manifest" }),
              ],
            })
          : absent(),
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="scheduler-row"]' ? fake({ items: [manifestRow] }) : absent(),
    });
    await expect(new WebNewDriver(page).schedulerCatalogRows()).resolves.toEqual([
      { name: "Imported", handle: "imported", origin: "Manifest", installs: "", status: "", updated: "" },
    ]);
    const empty = pageOf({
      locator: (selector: string) => (selector === '[data-testid="scheduler-row"]' ? fake({ items: [] }) : absent()),
    });
    await expect(new WebNewDriver(empty).schedulerCatalogRows()).resolves.toEqual([]);
  });

  it("reports empty, title and create visibility", async () => {
    const page = pageOf({
      title: () => "Acme · Schedulers",
      getByText: () => fake({ count: 1, visible: true }),
      getByRole: () => fake({ count: 1, visible: true }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerCatalogEmptyVisible()).resolves.toBe(true);
    await expect(driver.schedulerPageTitle()).resolves.toBe("Acme · Schedulers");
    await expect(driver.schedulerCreateVisible()).resolves.toBe(true);
  });

  it("reads only the trailing action cell's buttons", async () => {
    const actions = fake({ items: [fake({ text: "Install" }), fake({ text: "Edit" }), fake({ text: "Delete" })] });
    const row = fake({
      locate: (selector: string) =>
        selector === "td" ? fake({ last: () => fake({ role: () => actions }) }) : absent(),
    });
    const rows = vi.fn((selector: string) =>
      selector === '[data-testid="scheduler-row"]'
        ? fake({ filter: () => row })
        : selector === "td:first-child p:nth-of-type(2)"
          ? fake({ hasText: /^weekly-report$/ })
          : absent()
    );
    const page = pageOf({ locator: rows });
    await expect(new WebNewDriver(page).schedulerRowActions("weekly-report")).resolves.toEqual([
      "Install",
      "Edit",
      "Delete",
    ]);
    expect(rows).toHaveBeenCalledWith('[data-testid="scheduler-row"]');
    expect(rows).toHaveBeenCalledWith("td:first-child p:nth-of-type(2)", { hasText: /^weekly-report$/ });
    expect(rows).not.toHaveBeenCalledWith("code", expect.anything());
  });
});

describe("scheduler definition dialog", () => {
  it("opens create and waits on the handle field", async () => {
    const waitFor = vi.fn(async () => undefined);
    const dialog = fake({ byLabel: (label) => (label === "Handle" ? fake({ count: 1, waitFor }) : absent()) });
    const page = pageOf({
      getByRole: (role: string) => {
        if (role === "button") return fake({ click: vi.fn(async () => undefined) });
        return fake({ last: () => dialog });
      },
    });
    await new WebNewDriver(page).schedulerOpenCreate();
  });

  it("fills and reads the definition fields", async () => {
    const fields: Record<string, FakeLocator> = {
      Name: fake({ input: "Weekly report" }),
      Handle: fake({ input: "weekly-report" }),
      Description: fake({ input: "Summary" }),
      "Base prompt": fake({ input: "Review" }),
      Color: fake({ input: "#5e6ad2" }),
    };
    const dialog = fake({
      byLabel: (label) => fields[label] ?? absent(),
      role: (role) => (role === "switch" ? fake({ attribute: "true" }) : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await driver.schedulerFillDefinition({
      name: "Weekly report",
      handle: "weekly-report",
      description: "Summary",
      prompt: "Review",
      color: "#5e6ad2",
    });
    expect(fields["Base prompt"]?.fill).toHaveBeenCalledWith("Review");
    expect(fields.Color?.fill).toHaveBeenCalledWith("#5e6ad2");
    await expect(driver.schedulerDefinitionValues()).resolves.toEqual({
      name: "Weekly report",
      handle: "weekly-report",
      description: "Summary",
      prompt: "Review",
      color: "#5e6ad2",
      enabled: true,
    });
  });

  it("toggles enabled only when the state differs", async () => {
    const click = vi.fn(async () => undefined);
    const toggle = fake({ attribute: "true", click });
    const page = pageOf({ getByRole: () => fake({ last: () => fake({ role: () => toggle }) }) });
    const driver = new WebNewDriver(page);
    await driver.schedulerSetDefinitionEnabled(true);
    expect(click).not.toHaveBeenCalled();
    await driver.schedulerSetDefinitionEnabled(false);
    expect(click).toHaveBeenCalled();
  });

  it("reports handle lock and dialog openness", async () => {
    const handle = fake({ count: 1, disabled: true });
    const page = pageOf({ getByRole: () => fake({ last: () => fake({ byLabel: () => handle }) }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerDefinitionHandleLocked()).resolves.toBe(true);
    await expect(driver.schedulerDefinitionOpen()).resolves.toBe(true);
  });

  it("submits without waiting for close and reads inline errors", async () => {
    const submit = fake({ click: vi.fn(async () => undefined) });
    const error = fake({ text: "Slug is required.", visible: true });
    const dialog = fake({
      locate: (selector: string) =>
        selector === 'button[type="submit"]'
          ? submit
          : selector === '[role="alert"]'
            ? fake({ items: [error] })
            : absent(),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await driver.schedulerSubmitDefinition();
    expect(submit.click).toHaveBeenCalled();
    await expect(driver.schedulerDefinitionErrors()).resolves.toEqual(["Slug is required."]);
  });

  it("maps the definition handle rule to the oracle wording and leaves other errors alone", async () => {
    const errors = fake({
      items: [
        fake({ text: "Use lowercase letters, numbers, and single dashes.", visible: true }),
        fake({ text: "Enter the base prompt.", visible: true }),
      ],
    });
    const dialog = fake({ locate: (selector: string) => (selector === '[role="alert"]' ? errors : absent()) });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerDefinitionErrors()).resolves.toEqual([
      "Use lowercase letters, numbers, and dashes only.",
      "Enter the base prompt.",
    ]);
  });

  it("closes the definition dialog through Cancel", async () => {
    const cancel = fake({ click: vi.fn(async () => undefined) });
    const handle = fake({ count: 1, waitFor: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string) => (role === "button" ? cancel : absent()),
      byLabel: () => handle,
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerCloseDefinition();
    expect(cancel.click).toHaveBeenCalled();
    expect(handle.waitFor).toHaveBeenCalledWith(expect.objectContaining({ state: "detached" }));
  });
});

describe("scheduler edit and delete", () => {
  it("opens delete from the row and waits for the confirmation heading", async () => {
    const remove = fake({ click: vi.fn(async () => undefined) });
    const row = fake({
      role: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] === "Delete" ? remove : absent(),
    });
    const heading = fake({ count: 1, waitFor: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string, options?: Record<string, unknown>) =>
        role === "heading" && options?.["name"] === "Delete scheduler" ? heading : absent(),
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="scheduler-row"]' ? fake({ filter: () => row }) : absent(),
      getByRole: () => fake({ last: () => dialog }),
    });
    await new WebNewDriver(page).schedulerOpenDelete("weekly-report");
    expect(remove.click).toHaveBeenCalled();
    expect(heading.waitFor).toHaveBeenCalledWith(expect.objectContaining({ timeout: expect.any(Number) }));
  });

  it("opens edit from the row and waits for the handle value", async () => {
    const handle = fake({ count: 1, input: "weekly-report" });
    const edit = fake({ click: vi.fn(async () => undefined) });
    const row = fake({ role: (role: string) => (role === "button" ? edit : absent()) });
    const page = pageOf({
      locator: () => fake({ filter: () => row }),
      getByRole: () => fake({ last: () => fake({ byLabel: () => handle }) }),
    });
    await new WebNewDriver(page).schedulerOpenEdit("weekly-report");
    expect(edit.click).toHaveBeenCalled();
  });

  it("reads the delete dialog text and confirms or cancels", async () => {
    const cancel = fake({ click: vi.fn(async () => undefined) });
    const confirm = fake({ click: vi.fn(async () => undefined) });
    const dialog = fake({
      text: "Delete scheduler Deleting Weekly report stops its project installs from firing. The handle weekly-report becomes available again.",
      role: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] === "Cancel"
          ? cancel
          : role === "button" && options?.["name"] === "Delete scheduler"
            ? confirm
            : role === "heading"
              ? fake({ count: 0 })
              : absent(),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    const text = await driver.schedulerDeleteDialogText();
    expect(text).toContain("will stop firing");
    expect(text).toContain("available for re-creation");
    await driver.schedulerConfirmDelete();
    expect(confirm.click).toHaveBeenCalled();
    await driver.schedulerCancelDelete();
    expect(cancel.click).toHaveBeenCalled();
  });

  it("returns when the confirmation unmounts the dialog", async () => {
    const confirm = fake({ click: vi.fn(async () => undefined) });
    const heading = fake({ count: 0 });
    const dialog = fake({
      role: (role: string) => (role === "button" ? confirm : role === "heading" ? heading : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerConfirmDelete();
    expect(confirm.click).toHaveBeenCalled();
  });

  it("returns when a refusal leaves the dialog open with a toast", async () => {
    const confirm = fake({ click: vi.fn(async () => undefined) });
    const heading = fake({ count: 1 });
    const dialog = fake({
      role: (role: string) => (role === "button" ? confirm : role === "heading" ? heading : absent()),
    });
    const card = fake({ text: "Could not delete scheduler\nServer detail", visible: true });
    const page = pageOf({
      getByRole: () => fake({ last: () => dialog }),
      locator: (selector: string) =>
        selector === '[data-testid="toast-notification"]' ? fake({ items: [card] }) : absent(),
    });
    await new WebNewDriver(page).schedulerConfirmDelete();
    expect(confirm.click).toHaveBeenCalled();
  });

  it("throws when the confirmation settles on neither outcome", async () => {
    vi.useFakeTimers();
    try {
      const confirm = fake({ click: vi.fn(async () => undefined) });
      const heading = fake({ count: 1 });
      const dialog = fake({
        role: (role: string) => (role === "button" ? confirm : role === "heading" ? heading : absent()),
      });
      const waitForTimeout = vi.fn(async () => {
        vi.setSystemTime(Date.now() + 30_001);
      });
      const page = pageOf({ getByRole: () => fake({ last: () => dialog }), waitForTimeout });
      await expect(new WebNewDriver(page).schedulerConfirmDelete()).rejects.toThrow("settled on neither outcome");
      expect(waitForTimeout).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reports delete openness from the dialog heading", async () => {
    const dialog = fake({
      role: (role: string) => (role === "heading" ? fake({ count: 1 }) : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerDeleteOpen()).resolves.toBe(true);
  });

  it("leaves delete copy that already uses the oracle vocabulary untouched", async () => {
    const dialog = fake({ text: "Any active project bindings will stop firing." });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerDeleteDialogText()).resolves.toBe(
      "Any active project bindings will stop firing."
    );
  });
});

describe("scheduler notices", () => {
  function toastPage(title: string, message: string): Page {
    const card = fake({ text: `${title}\n${message}`, visible: true });
    return pageOf({ locator: () => fake({ items: [card] }) });
  }

  it("maps scheduler failure titles to the shared error notice", async () => {
    for (const title of ["Could not create scheduler", "Could not update scheduler", "Could not delete scheduler"]) {
      await expect(new WebNewDriver(toastPage(title, "Server detail")).rulesLastToast()).resolves.toEqual({
        title: "Something went wrong",
        message: "Server detail",
      });
    }
  });

  it("maps update and delete success messages while leaving other notices alone", async () => {
    await expect(new WebNewDriver(toastPage("Scheduler updated", "")).rulesLastToast()).resolves.toEqual({
      title: "Scheduler updated",
      message: "Subsequent runs will use the updated definition.",
    });
    await expect(
      new WebNewDriver(toastPage("Scheduler deleted", "Its handle can now be used again.")).rulesLastToast()
    ).resolves.toEqual({ title: "Scheduler deleted", message: "Active bindings have stopped firing." });
    await expect(
      new WebNewDriver(toastPage("Scheduler created", "Ready to install.")).rulesLastToast()
    ).resolves.toEqual({
      title: "Scheduler created",
      message: "Ready to install.",
    });
    await expect(
      new WebNewDriver(toastPage("Scheduler updated", "Already has updated definition.")).rulesLastToast()
    ).resolves.toEqual({
      title: "Scheduler updated",
      message: "Already has updated definition.",
    });
    await expect(
      new WebNewDriver(toastPage("Scheduler deleted", "Install has stopped firing.")).rulesLastToast()
    ).resolves.toEqual({
      title: "Scheduler deleted",
      message: "Install has stopped firing.",
    });
  });
});

describe("scheduler bulk install", () => {
  it("opens install from the row and waits for the picker toggle", async () => {
    const install = fake({ click: vi.fn(async () => undefined) });
    const row = fake({
      role: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] === "Install" ? install : absent(),
    });
    const toggle = fake({ count: 1, waitFor: vi.fn(async () => undefined) });
    const dialog = fake({ locate: () => toggle });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="scheduler-row"]' ? fake({ filter: () => row }) : absent(),
      getByRole: () => fake({ last: () => dialog }),
    });
    await new WebNewDriver(page).schedulerOpenInstall("weekly-report");
    expect(install.click).toHaveBeenCalled();
    expect(toggle.waitFor).toHaveBeenCalled();
  });

  it("reads picker options and handles locked installed rows", async () => {
    const alpha = fake({
      text: "Bulk Alpha\nAG6A",
      role: (role: string) => (role === "checkbox" ? fake({ checked: false, disabled: false }) : absent()),
    });
    const locked = fake({
      text: "Bulk Gamma\nAG6C\nInstalled",
      role: (role: string) => (role === "checkbox" ? fake({ checked: true, disabled: true }) : absent()),
    });
    const options = fake({ items: [alpha, locked], filter: () => fake({ items: [alpha, locked] }) });
    const panel = fake({
      locate: (selector: string) =>
        selector === "li" ? options : selector === 'input[type="checkbox"]' ? fake() : absent(),
    });
    const dialog = fake({
      locate: (selector: string) =>
        selector === "div[data-prevent-outside-click]" ? fake({ count: 1, ...panel.options }) : absent(),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerInstallPickerOptions()).resolves.toEqual([
      { name: "Bulk Alpha", identifier: "AG6A", checked: false, locked: false },
      { name: "Bulk Gamma", identifier: "AG6C", checked: true, locked: true },
    ]);
  });

  it("ignores the loading placeholder in the picker body", async () => {
    const loading = fake({ text: "Checking installs…" });
    const alpha = fake({
      text: "Bulk Alpha\nAG6A",
      role: (role: string) => (role === "checkbox" ? fake({ checked: false, disabled: false }) : absent()),
    });
    const options = fake({ items: [alpha], filter: () => fake({ items: [alpha] }) });
    const panel = fake({
      locate: (selector: string) => (selector === "li" ? fake({ items: [loading], filter: () => options }) : absent()),
    });
    const dialog = fake({
      locate: (selector: string) => (selector === "div[data-prevent-outside-click]" ? panel : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    await expect(new WebNewDriver(page).schedulerInstallPickerOptions()).resolves.toEqual([
      { name: "Bulk Alpha", identifier: "AG6A", checked: false, locked: false },
    ]);
  });

  it("scopes the picker row filter to a page-rooted checkbox locator", async () => {
    const checkboxProbe = fake({ count: 1 });
    const pageLocator = vi.fn((selector: string) => (selector === 'input[type="checkbox"]' ? checkboxProbe : absent()));
    const alpha = fake({
      text: "Bulk Alpha\nAG6A",
      role: (role: string) => (role === "checkbox" ? fake({ checked: false, disabled: false }) : absent()),
    });
    const filter = vi.fn(() => fake({ items: [alpha] }));
    const items = fake({ items: [alpha], filter });
    const panel = fake({ locate: (selector: string) => (selector === "li" ? items : absent()) });
    const dialog = fake({
      locate: (selector: string) => (selector === "div[data-prevent-outside-click]" ? panel : absent()),
    });
    const page = pageOf({ locator: pageLocator, getByRole: () => fake({ last: () => dialog }) });
    await new WebNewDriver(page).schedulerInstallPickerOptions();
    // `has` resolves against the page, so a panel-rooted locator would be
    // narrowed to the panel's own subtree and match no row at all.
    expect(pageLocator).toHaveBeenCalledWith('input[type="checkbox"]');
    expect(filter).toHaveBeenCalledWith({ has: checkboxProbe });
  });

  it("searches and toggles projects in the picker", async () => {
    const search = fake();
    const checkbox = fake({ click: vi.fn(async () => undefined) });
    const item = fake({ role: () => checkbox });
    const toggle = fake({ click: vi.fn(async () => undefined) });
    const panel = fake({
      byLabel: () => search,
      role: (role: string) => (role === "button" ? toggle : absent()),
      locate: (selector: string) => (selector === "li" ? fake({ filter: () => item }) : absent()),
    });
    const dialog = fake({ locate: () => fake({ count: 1, ...panel.options }) });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await driver.schedulerInstallSearch("Alpha");
    expect(search.fill).toHaveBeenCalledWith("Alpha");
    await driver.schedulerInstallToggleSelectAll();
    expect(toggle.click).toHaveBeenCalled();
    await driver.schedulerInstallToggleProject("Bulk Alpha");
    expect(checkbox.click).toHaveBeenCalled();
  });

  it("reads the selected summary and submits or closes", async () => {
    const submit = fake({ click: vi.fn(async () => undefined) });
    const cancel = fake({ click: vi.fn(async () => undefined) });
    const toggle = fake({ count: 1, text: "2 projects selected" });
    const dialog = fake({
      locate: (selector: string) =>
        selector === 'button[aria-haspopup="listbox"]'
          ? toggle
          : selector === 'button[type="submit"]'
            ? submit
            : absent(),
      role: (role: string) => (role === "button" ? cancel : absent()),
    });
    const page = pageOf({ getByRole: () => fake({ last: () => dialog }) });
    const driver = new WebNewDriver(page);
    await expect(driver.schedulerInstallSelectedSummary()).resolves.toBe("2 projects selected");
    await driver.schedulerInstallSubmit();
    expect(submit.click).toHaveBeenCalled();
    await driver.schedulerCloseInstall();
    expect(cancel.click).toHaveBeenCalled();
    await expect(driver.schedulerInstallOpen()).resolves.toBe(true);
  });

  it("reports install openness from the text fallback and reports a closed dialog", async () => {
    const open = fake({
      locator: () => fake({ count: 0 }),
      byText: () => fake({ count: 1 }),
    });
    const closed = fake({
      locator: () => fake({ count: 0 }),
      byText: () => fake({ count: 0 }),
    });
    await expect(
      new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => open }) })).schedulerInstallOpen()
    ).resolves.toBe(true);
    await expect(
      new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => closed }) })).schedulerInstallOpen()
    ).resolves.toBe(false);
  });
});

describe("project scheduler create", () => {
  it("opens the project schedulers address and waits for the create control", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => undefined);
    const create = fake({ count: 1, waitFor });
    const page = pageOf({
      goto,
      locator: (selector: string) => (selector === "#main-sidebar" ? fake({ waitFor }) : absent()),
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] === "New Scheduler" ? create : absent(),
    });
    await new WebNewDriver(page).schedulerOpenProjectSchedulers("acme", "p1");
    expect(goto).toHaveBeenCalledWith("/acme/projects/p1/schedulers/list");
    expect(create.waitFor).toHaveBeenCalled();
  });

  it("opens create and switches to the create tab when install is preselected", async () => {
    const create = fake({ click: vi.fn(async () => undefined) });
    const tabClick = vi.fn(async () => undefined);
    const attribute = vi.fn(async (name: string) => (name === "aria-selected" ? "false" : null));
    const tab = fake({ count: 1, getAttribute: attribute, click: tabClick });
    const slug = fake({ count: 1, waitFor: vi.fn(async () => undefined) });
    const dialog = fake({
      role: (role: string, options?: Record<string, unknown>) =>
        role === "heading" && options?.["name"] === "New Scheduler"
          ? fake({ waitFor: vi.fn(async () => undefined) })
          : role === "tab"
            ? tab
            : absent(),
      byLabel: (label: string) => (label === "Slug" ? slug : absent()),
    });
    const page = pageOf({
      getByRole: (role: string, options?: Record<string, unknown>) =>
        role === "button" && options?.["name"] === "New Scheduler" ? create : fake({ last: () => dialog }),
    });
    await new WebNewDriver(page).schedulerOpenProjectCreate();
    expect(create.click).toHaveBeenCalled();
    expect(attribute).toHaveBeenCalledWith("aria-selected");
    expect(tabClick).toHaveBeenCalled();
    expect(slug.waitFor).toHaveBeenCalled();
  });

  it("leaves an already selected create tab and a create-first dialog alone", async () => {
    const tab = fake({
      count: 1,
      getAttribute: async (name: string) => (name === "aria-selected" ? "true" : null),
      click: vi.fn(async () => undefined),
    });
    const dialog = fake({
      role: (role: string) => (role === "tab" ? tab : fake({ waitFor: vi.fn(async () => undefined) })),
      byLabel: (label: string) =>
        label === "Slug" ? fake({ count: 1, waitFor: vi.fn(async () => undefined) }) : absent(),
    });
    await new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => dialog }) })).schedulerOpenProjectCreate();
    expect(tab.click).not.toHaveBeenCalled();
  });

  it("clicks the create tab when the tab reports no aria-selected value", async () => {
    const tabClick = vi.fn(async () => undefined);
    const tab = fake({ count: 1, getAttribute: async () => null, click: tabClick });
    const dialog = fake({
      role: (role: string) => (role === "tab" ? tab : fake({ waitFor: vi.fn(async () => undefined) })),
      byLabel: (label: string) =>
        label === "Slug" ? fake({ count: 1, waitFor: vi.fn(async () => undefined) }) : absent(),
    });
    await new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => dialog }) })).schedulerOpenProjectCreate();
    expect(tabClick).toHaveBeenCalled();
  });

  it("opens a create-first dialog with no tab strip", async () => {
    const noTab = fake({
      role: () => absent(),
      byLabel: (label: string) =>
        label === "Slug" ? fake({ count: 1, waitFor: vi.fn(async () => undefined) }) : absent(),
    });
    await new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => noTab }) })).schedulerOpenProjectCreate();
  });

  it("submits the project create dialog", async () => {
    const submit = fake({ click: vi.fn(async () => undefined) });
    const dialog = fake({ locate: () => submit });
    await new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => dialog }) })).schedulerProjectCreateSubmit();
    expect(submit.click).toHaveBeenCalled();
  });

  it("maps the project create refusal onto the oracle vocabulary", async () => {
    const errors = fake({
      items: [
        fake({ text: "Enter a handle.", visible: true }),
        fake({ text: "Enter the base prompt.", visible: true }),
        fake({ text: "Use lowercase letters, numbers, and single dashes.", visible: true }),
        fake({ text: "Hidden refusal.", visible: false }),
      ],
    });
    const dialog = fake({ locate: (selector: string) => (selector === '[role="alert"]' ? errors : absent()) });
    await expect(
      new WebNewDriver(pageOf({ getByRole: () => fake({ last: () => dialog }) })).schedulerProjectCreateErrors()
    ).resolves.toEqual([
      "Slug is required.",
      "Enter the base prompt.",
      "Use lowercase letters, numbers, and dashes only.",
    ]);
  });
});

describe("scheduler route gates", () => {
  it("opens prompts and reports gate visibility", async () => {
    const goto = vi.fn(async () => null);
    const page = pageOf({
      goto,
      getByRole: () => fake({ count: 1, visible: true }),
      locator: () => fake({ count: 1 }),
    });
    const driver = new WebNewDriver(page);
    await driver.schedulerOpenPrompts("acme");
    expect(goto).toHaveBeenCalledWith("/acme/prompts");
    await expect(driver.schedulerNotAuthorizedVisible()).resolves.toBe(true);
    await expect(driver.schedulerWorkspaceNotFoundVisible()).resolves.toBe(true);
    await expect(driver.schedulerShellCount()).resolves.toBe(1);
  });
});
