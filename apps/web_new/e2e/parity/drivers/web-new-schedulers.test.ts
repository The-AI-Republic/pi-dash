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
  }

  count(): Promise<number> {
    return Promise.resolve(this.options.items?.length ?? this.options.count ?? 0);
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
  textContent(): Promise<string | null> {
    return Promise.resolve(this.options.content ?? this.options.text ?? "");
  }
  inputValue(): Promise<string> {
    return Promise.resolve(this.options.input ?? "");
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
    route: vi.fn(async () => undefined),
    unroute: vi.fn(async () => undefined),
    getByRole: vi.fn(() => absent()),
    getByText: vi.fn(() => absent()),
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
