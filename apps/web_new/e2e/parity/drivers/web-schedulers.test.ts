// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebDriver recurrence-builder methods (NEWFRONT-192,
// AGT-065): the old app's main#561 widget read through the shared driver
// contract. Playwright is replaced by a small Locator/Page double so the
// assertions pin the addresses, selectors and read shapes without a browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebDriver } from "./web";

interface Options {
  count?: number;
  visible?: boolean;
  text?: string;
  input?: string;
  checked?: boolean;
  attribute?: string | null;
  attributes?: Record<string, string | null>;
  items?: FakeLocator[];
  locate?: (selector: string) => FakeLocator;
  role?: (role: string, options?: Record<string, unknown>) => FakeLocator;
  byLabel?: (label: string) => FakeLocator;
  last?: () => FakeLocator;
  click?: () => Promise<void>;
  fill?: (value: string) => Promise<void>;
  selectOption?: (value: string) => Promise<void>;
  evaluate?: unknown;
  evaluateNode?: unknown;
  evaluateAll?: (fn: (nodes: { value: string }[]) => unknown) => Promise<unknown>;
  isChecked?: () => Promise<boolean>;
  innerText?: () => Promise<string>;
  throws?: boolean;
}

class FakeLocator {
  readonly options: Options;
  readonly click: ReturnType<typeof vi.fn>;
  readonly fill: ReturnType<typeof vi.fn>;
  readonly selectOption: ReturnType<typeof vi.fn>;
  readonly waitFor = vi.fn(async () => undefined);

  constructor(options: Options = {}) {
    this.options = options;
    this.click = vi.fn(options.click ?? (async () => undefined));
    this.fill = vi.fn(options.fill ?? (async () => undefined));
    this.selectOption = vi.fn(options.selectOption ?? (async () => undefined));
  }

  private fail<T>(value: T): Promise<T> {
    return this.options.throws === true ? Promise.reject(new Error("read failed")) : Promise.resolve(value);
  }
  count(): Promise<number> {
    return this.fail(this.options.items?.length ?? this.options.count ?? 0);
  }
  isVisible(): Promise<boolean> {
    return this.fail(this.options.visible ?? true);
  }
  isChecked(): Promise<boolean> {
    if (this.options.isChecked !== undefined) return this.options.isChecked();
    return this.fail(this.options.checked ?? false);
  }
  innerText(): Promise<string> {
    if (this.options.innerText !== undefined) return this.options.innerText();
    return this.fail(this.options.text ?? "");
  }
  inputValue(): Promise<string> {
    return this.fail(this.options.input ?? "");
  }
  getAttribute(name: string): Promise<string | null> {
    if (this.options.attributes !== undefined) return Promise.resolve(this.options.attributes[name] ?? null);
    return Promise.resolve(this.options.attribute ?? null);
  }
  first(): FakeLocator {
    return this.options.items?.[0] ?? this;
  }
  last(): FakeLocator {
    if (this.options.last !== undefined) return this.options.last();
    return this.options.items?.at(-1) ?? this;
  }
  nth(index: number): FakeLocator {
    return this.options.items?.[index] ?? absent();
  }
  getByRole(role: string, roleOptions?: Record<string, unknown>): FakeLocator {
    return this.options.role?.(role, roleOptions) ?? this;
  }
  getByLabel(label: string): FakeLocator {
    return this.options.byLabel?.(label) ?? absent();
  }
  evaluate(pageFunction?: (node: unknown) => unknown): Promise<unknown> {
    if (this.options.throws === true) return Promise.reject(new Error("evaluate failed"));
    if (pageFunction !== undefined && this.options.evaluateNode !== undefined)
      return Promise.resolve(pageFunction(this.options.evaluateNode));
    return Promise.resolve(this.options.evaluate ?? "");
  }
  evaluateAll(fn: (nodes: { value: string }[]) => unknown): Promise<unknown> {
    if (this.options.evaluateAll !== undefined) return this.options.evaluateAll(fn);
    return Promise.resolve([]);
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
    url: vi.fn(() => "http://localhost/acme/projects/p1/schedulers/list"),
    getByRole: vi.fn(() => absent()),
    getByText: vi.fn(() => absent()),
    locator: vi.fn(() => absent()),
    ...overrides,
  } as unknown as Page;
}

/** A page whose scheduler dialog is `dialog`. */
function dialogPage(dialog: FakeLocator): Page {
  return pageOf({ locator: () => fake({ last: () => dialog }) });
}

/** The humanizer help-line chain resolving `sentence` ("" when unwired). */
function humanizer(sentence: string): (selector: string) => FakeLocator {
  return (selector: string) => {
    if (selector !== 'label:has-text("Recurrence")') return absent();
    const line = new FakeLocator({ text: sentence });
    line.options.locate = () => {
      const span = new FakeLocator({ text: sentence });
      span.options.locate = () => span;
      return span;
    };
    return line;
  };
}

function builderDialog(overrides: {
  unit?: FakeLocator;
  raw?: FakeLocator;
  openToggle?: FakeLocator;
  closeToggle?: FakeLocator;
  labels?: Record<string, FakeLocator>;
  roles?: (role: string, options?: Record<string, unknown>) => FakeLocator;
  sentence?: string;
}): FakeLocator {
  return new FakeLocator({
    byLabel: (label: string) => {
      if (label === "Repeat every") return overrides.unit ?? absent();
      if (label === "Recurrence") return overrides.raw ?? absent();
      return overrides.labels?.[label] ?? absent();
    },
    role: (role: string, options?: Record<string, unknown>) => {
      const name = options?.["name"];
      if (role === "button" && name === "Advanced: edit raw RRULE") return overrides.openToggle ?? absent();
      if (role === "button" && name === "Hide raw RRULE") return overrides.closeToggle ?? absent();
      return overrides.roles?.(role, options) ?? absent();
    },
    locate: humanizer(overrides.sentence ?? ""),
  });
}

function unitSelect(input: string, optionValues: string[]): FakeLocator {
  return new FakeLocator({
    count: 1,
    visible: true,
    input,
    locate: (selector: string) =>
      selector === "option"
        ? new FakeLocator({
            evaluateAll: async (fn: (nodes: { value: string }[]) => unknown) =>
              fn(optionValues.map((value) => ({ value }))),
          })
        : absent(),
  });
}

function chip(name: string, pressed: boolean): FakeLocator {
  return new FakeLocator({
    attributes: { "aria-label": name, "aria-pressed": pressed ? "true" : "false" },
  });
}

function endsRadio(labelText: string, checked: boolean): FakeLocator {
  const label = new FakeLocator({ text: labelText });
  return new FakeLocator({
    checked,
    locate: (selector: string) => (selector === "xpath=ancestor::label[1]" ? label : absent()),
  });
}

describe("recurrence facts", () => {
  it("reads a daily builder with the raw field closed", async () => {
    const dialog = builderDialog({
      unit: unitSelect("DAILY", ["DAILY", "WEEKLY", "MONTHLY", "YEARLY", "NONE"]),
      raw: fake({ count: 0 }),
      openToggle: fake({ count: 1, visible: true }),
      labels: {
        Interval: fake({ count: 1, visible: true, input: "1" }),
      },
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const radios = new FakeLocator({ items: [endsRadio("Never", true)] });
          radios.options.role = () => radios;
          return radios;
        }
        return absent();
      },
      sentence: "every day",
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts).toEqual({
      visible: true,
      interval: "1",
      unit: "day",
      unitOptions: ["day", "week", "month", "year", "none"],
      weekdays: [],
      monthlyPattern: null,
      dayOfMonth: null,
      ordinal: null,
      weekdayOfMonth: null,
      ends: "never",
      endDate: null,
      occurrences: null,
      rawVisible: false,
      rawValue: null,
      advancedVisible: true,
      sentence: "every day",
    });
  });

  it("reads chips, ends values and an open raw field", async () => {
    const dialog = builderDialog({
      unit: unitSelect("WEEKLY", ["DAILY", "WEEKLY", "MONTHLY", "YEARLY", "NONE"]),
      raw: fake({ count: 1, visible: true, input: "FREQ=WEEKLY;BYDAY=MO,FR" }),
      closeToggle: fake({ count: 1, visible: true }),
      labels: {
        Interval: fake({ count: 1, visible: true, input: "2" }),
        "End date (UTC)": fake({ count: 1, visible: true, input: "2027-01-05" }),
        "Number of occurrences": fake({ count: 1, visible: true, input: "13" }),
      },
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Repeat on") {
          const chips = [chip("Monday", true), chip("Friday", true)];
          const group = new FakeLocator({ items: chips });
          group.options.role = (inner: string) => (inner === "button" ? new FakeLocator({ items: chips }) : absent());
          return group;
        }
        if (role === "group" && options?.["name"] === "Ends") {
          const radios = [endsRadio("Never", false), endsRadio("On (UTC, inclusive)", true)];
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: radios });
          return group;
        }
        return absent();
      },
      sentence: "every 2 weeks on Monday, Friday",
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.unit).toBe("week");
    expect(facts.interval).toBe("2");
    expect(facts.weekdays).toEqual([
      { name: "Monday", pressed: true },
      { name: "Friday", pressed: true },
    ]);
    expect(facts.ends).toBe("on");
    expect(facts.endDate).toBe("2027-01-05");
    expect(facts.occurrences).toBeNull();
    expect(facts.rawVisible).toBe(true);
    expect(facts.rawValue).toBe("FREQ=WEEKLY;BYDAY=MO,FR");
    expect(facts.advancedVisible).toBe(true);
  });

  it("reports occurrences only for the after mode", async () => {
    const dialog = builderDialog({
      unit: unitSelect("DAILY", ["DAILY"]),
      labels: {
        "Number of occurrences": fake({ count: 1, visible: true, input: "4" }),
      },
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: [endsRadio("After occurrences", true)] });
          return group;
        }
        return absent();
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.ends).toBe("after");
    expect(facts.occurrences).toBe("4");
    expect(facts.endDate).toBeNull();
  });

  it("treats an unreadable ends radio as unchecked", async () => {
    const flakyLabel = new FakeLocator({ text: "Never" });
    const flaky = new FakeLocator({
      isChecked: async () => {
        throw new Error("detached");
      },
      locate: (selector: string) => (selector === "xpath=ancestor::label[1]" ? flakyLabel : absent()),
    });
    const dialog = builderDialog({
      unit: unitSelect("DAILY", ["DAILY"]),
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: [flaky, endsRadio("On (UTC, inclusive)", true)] });
          return group;
        }
        return absent();
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.ends).toBe("on");
  });

  it("reads the monthly selection through the option-text probe", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["MONTHLY"]),
      labels: {
        "Monthly repeat mode": fake({
          count: 1,
          visible: true,
          input: "monthday",
          evaluateNode: { selectedOptions: [{ text: "Monthly on day 9" }] },
        }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("day");
    expect(facts.dayOfMonth).toBe("9");
  });

  it("reads no monthly day when nothing is selected", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["MONTHLY"]),
      labels: {
        "Monthly repeat mode": fake({
          count: 1,
          visible: true,
          input: "monthday",
          evaluateNode: { selectedOptions: [] },
        }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("day");
    expect(facts.dayOfMonth).toBeNull();
  });

  it("reads chips missing accessible names as blank", async () => {
    const nameless = new FakeLocator({ attributes: { "aria-pressed": "true" } });
    const dialog = builderDialog({
      unit: unitSelect("WEEKLY", ["WEEKLY"]),
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Repeat on") {
          const group = new FakeLocator({ items: [nameless] });
          group.options.role = (inner: string) =>
            inner === "button" ? new FakeLocator({ items: [nameless] }) : absent();
          return group;
        }
        return absent();
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.weekdays).toEqual([{ name: "", pressed: true }]);
  });

  it("reads no monthly ordinal out of an unrecognized selection", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["MONTHLY"]),
      labels: {
        "Monthly repeat mode": fake({ count: 1, visible: true, input: "weekday", evaluate: "Every fortnight" }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("weekday");
    expect(facts.ordinal).toBeNull();
    expect(facts.weekdayOfMonth).toBeNull();
  });

  it("reads no monthly ordinal out of unknown schedule words", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["MONTHLY"]),
      labels: {
        "Monthly repeat mode": fake({
          count: 1,
          visible: true,
          input: "weekday",
          evaluate: "Monthly on the zeroth Funday",
        }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("weekday");
    expect(facts.ordinal).toBeNull();
    expect(facts.weekdayOfMonth).toBeNull();
  });

  it("skips an ends radio whose label reads blank", async () => {
    const blankLabel = new FakeLocator({ innerText: async () => null as unknown as string });
    const blank = new FakeLocator({
      checked: true,
      locate: (selector: string) => (selector === "xpath=ancestor::label[1]" ? blankLabel : absent()),
    });
    const dialog = builderDialog({
      unit: unitSelect("DAILY", ["DAILY"]),
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: [blank, endsRadio("Never", false)] });
          return group;
        }
        return absent();
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.ends).toBeNull();
  });

  it("reads the monthly day out of the selected option", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["DAILY", "WEEKLY", "MONTHLY", "YEARLY", "NONE"]),
      labels: {
        "Monthly repeat mode": fake({ count: 1, visible: true, input: "monthday", evaluate: "Monthly on day 9" }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.unit).toBe("month");
    expect(facts.monthlyPattern).toBe("day");
    expect(facts.dayOfMonth).toBe("9");
    expect(facts.ordinal).toBeNull();
    expect(facts.weekdayOfMonth).toBeNull();
  });

  it("reads the monthly nth weekday out of the selected option", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["DAILY", "WEEKLY", "MONTHLY", "YEARLY", "NONE"]),
      labels: {
        "Monthly repeat mode": fake({
          count: 1,
          visible: true,
          input: "weekday",
          evaluate: "Monthly on the second Monday",
        }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("weekday");
    expect(facts.dayOfMonth).toBeNull();
    expect(facts.ordinal).toBe("2");
    expect(facts.weekdayOfMonth).toBe("MO");
  });

  it("reads the last-weekday monthly option", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["DAILY"]),
      labels: {
        "Monthly repeat mode": fake({
          count: 1,
          visible: true,
          input: "weekday",
          evaluate: "Monthly on the last Friday",
        }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.ordinal).toBe("-1");
    expect(facts.weekdayOfMonth).toBe("FR");
  });

  it("leaves monthly fields null when the option words do not parse", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["DAILY"]),
      labels: {
        "Monthly repeat mode": fake({ count: 1, visible: true, input: "monthday", evaluate: "Monthly on day" }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBe("day");
    expect(facts.dayOfMonth).toBeNull();
  });

  it("leaves monthly fields null for an unknown mode", async () => {
    const dialog = builderDialog({
      unit: unitSelect("MONTHLY", ["DAILY"]),
      labels: {
        "Monthly repeat mode": fake({ count: 1, visible: true, input: "fortnight", evaluate: "Fortnightly" }),
      },
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.monthlyPattern).toBeNull();
    expect(facts.dayOfMonth).toBeNull();
  });

  it("passes unknown units through instead of inventing names", async () => {
    const dialog = builderDialog({
      unit: unitSelect("HOURLY", ["HOURLY", "DAILY"]),
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.unit).toBeNull();
    expect(facts.unitOptions).toEqual(["HOURLY", "day"]);
  });

  it("reports a forced raw rule with no builder", async () => {
    const dialog = builderDialog({
      raw: fake({ count: 1, visible: true, input: "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9" }),
      sentence: "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9",
    });
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts).toMatchObject({
      visible: false,
      unit: null,
      unitOptions: [],
      weekdays: [],
      ends: null,
      rawVisible: true,
      rawValue: "FREQ=WEEKLY;BYDAY=MO;BYHOUR=9",
      advancedVisible: false,
    });
  });

  it("ignores unreadable radios and a failing sentence", async () => {
    const dialog = builderDialog({
      unit: unitSelect("DAILY", ["DAILY"]),
      roles: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const broken = new FakeLocator({
            checked: true,
            innerText: async () => {
              throw new Error("label gone");
            },
          });
          broken.options.locate = () => broken;
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: [broken] });
          return group;
        }
        return absent();
      },
    });
    dialog.options.locate = () => {
      throw new Error("no help line");
    };
    const facts = await new WebDriver(dialogPage(dialog)).schedulerRecurrenceFacts();
    expect(facts.ends).toBeNull();
    expect(facts.sentence).toBe("");
  });
});

describe("recurrence actions", () => {
  it("acts on each supported control", async () => {
    const fill = vi.fn(async () => undefined);
    const selectOption = vi.fn(async () => undefined);
    const click = vi.fn(async () => undefined);
    const control = fake({ count: 1, fill, selectOption, click });
    const dialog = new FakeLocator({
      byLabel: () => control,
      role: (role: string) => {
        if (role === "group") {
          const group = new FakeLocator({});
          group.options.role = () => control;
          return group;
        }
        return absent();
      },
    });
    const driver = new WebDriver(dialogPage(dialog));
    await driver.schedulerRecurrenceAct({ kind: "interval", value: 3 });
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "month" });
    await driver.schedulerRecurrenceAct({ kind: "weekday", value: "Friday" });
    await driver.schedulerRecurrenceAct({ kind: "monthly", value: "weekday" });
    await driver.schedulerRecurrenceAct({ kind: "endDate", value: "2027-01-01" });
    await driver.schedulerRecurrenceAct({ kind: "occurrences", value: 12 });
    expect(fill.mock.calls.map((call) => call[0])).toEqual(["3", "2027-01-01", "12"]);
    expect(selectOption.mock.calls.map((call) => call[0])).toEqual(["MONTHLY", "weekday"]);
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("maps every unit onto the old select value", async () => {
    const seen: string[] = [];
    const dialog = new FakeLocator({
      byLabel: (label: string) =>
        label === "Repeat every"
          ? fake({ count: 1, selectOption: async (value: string) => void seen.push(value) })
          : absent(),
    });
    const driver = new WebDriver(dialogPage(dialog));
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "none" });
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "day" });
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "week" });
    await driver.schedulerRecurrenceAct({ kind: "unit", value: "year" });
    expect(seen).toEqual(["NONE", "DAILY", "WEEKLY", "YEARLY"]);
  });

  it("maps each monthly pattern onto the old select value", async () => {
    const seen: string[] = [];
    const dialog = new FakeLocator({
      byLabel: (label: string) =>
        label === "Monthly repeat mode"
          ? fake({ count: 1, selectOption: async (value: string) => void seen.push(value) })
          : absent(),
    });
    const driver = new WebDriver(dialogPage(dialog));
    await driver.schedulerRecurrenceAct({ kind: "monthly", value: "day" });
    await driver.schedulerRecurrenceAct({ kind: "monthly", value: "weekday" });
    expect(seen).toEqual(["monthday", "weekday"]);
  });

  it("clicks the ends radio whose label matches", async () => {
    const click = vi.fn(async () => undefined);
    const label = new FakeLocator({ text: "On (UTC, inclusive)" });
    const picked = new FakeLocator({
      checked: false,
      click,
      locate: (selector: string) => (selector === "xpath=ancestor::label[1]" ? label : absent()),
    });
    const dialog = new FakeLocator({
      role: (role: string, options?: Record<string, unknown>) => {
        if (role === "group" && options?.["name"] === "Ends") {
          const group = new FakeLocator({ count: 1 });
          group.options.role = () => new FakeLocator({ items: [endsRadio("Never", true), picked] });
          return group;
        }
        return absent();
      },
    });
    await new WebDriver(dialogPage(dialog)).schedulerRecurrenceAct({ kind: "ends", value: "on" });
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("throws when the ends mode is missing", async () => {
    const dialog = new FakeLocator({ role: () => absent() });
    await expect(
      new WebDriver(dialogPage(dialog)).schedulerRecurrenceAct({ kind: "ends", value: "after" })
    ).rejects.toThrow("offers no after radio");
  });

  it("throws for the monthly edits the old widget lacks", async () => {
    const driver = new WebDriver(dialogPage(new FakeLocator({})));
    await expect(driver.schedulerRecurrenceAct({ kind: "day", value: 15 })).rejects.toThrow("no day control");
    await expect(driver.schedulerRecurrenceAct({ kind: "ordinal", value: "-1" })).rejects.toThrow("no ordinal control");
    await expect(driver.schedulerRecurrenceAct({ kind: "weekdayOfMonth", value: "MO" })).rejects.toThrow(
      "no weekdayOfMonth control"
    );
  });
});

describe("raw disclosure", () => {
  it("opens a closed disclosure and reports it open", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const dialog = builderDialog({
      raw: fake({ count: 0 }),
      openToggle: toggle,
    });
    await expect(new WebDriver(dialogPage(dialog)).schedulerRecurrenceAdvanced()).resolves.toBe(true);
    expect(toggle.click).toHaveBeenCalledTimes(1);
  });

  it("closes an open disclosure and reports it closed", async () => {
    let open = false;
    const dialog = builderDialog({
      raw: fake({ count: 1, visible: true }),
      openToggle: fake({
        get count() {
          return open ? 1 : 0;
        },
      }),
      closeToggle: fake({
        count: 1,
        click: async () => {
          open = true;
        },
      }),
    });
    await expect(new WebDriver(dialogPage(dialog)).schedulerRecurrenceAdvanced()).resolves.toBe(false);
  });

  it("treats a forced raw rule as already advanced", async () => {
    const dialog = builderDialog({
      raw: fake({ count: 1, visible: true, input: "FREQ=DAILY" }),
    });
    await expect(new WebDriver(dialogPage(dialog)).schedulerRecurrenceAdvanced()).resolves.toBe(true);
  });

  it("reveals the raw field before typing into it", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 0, fill: vi.fn(async () => undefined) });
    const dialog = builderDialog({ raw, openToggle: toggle });
    await new WebDriver(dialogPage(dialog)).schedulerRecurrenceRaw("FREQ=DAILY;COUNT=2");
    expect(toggle.click).toHaveBeenCalledTimes(1);
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY;COUNT=2");
  });

  it("does not touch the disclosure when the raw field is already open", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 1, visible: true, fill: vi.fn(async () => undefined) });
    const dialog = builderDialog({ raw, openToggle: toggle });
    await new WebDriver(dialogPage(dialog)).schedulerRecurrenceRaw("FREQ=DAILY");
    expect(toggle.click).not.toHaveBeenCalled();
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY");
  });

  it("leaves a missing toggle alone so the fill fails honestly", async () => {
    const raw = fake({ count: 0, fill: vi.fn(async () => undefined) });
    const dialog = builderDialog({ raw });
    await new WebDriver(dialogPage(dialog)).schedulerRecurrenceRaw("FREQ=DAILY");
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY");
  });
});

describe("schedule reads and fills", () => {
  function scheduleDialog(raw: FakeLocator, toggle: FakeLocator): FakeLocator {
    return new FakeLocator({
      byLabel: (label: string) => {
        if (label === "Recurrence") return raw;
        if (label === "Starts at") return fake({ count: 1, input: "2026-11-09T09:30" });
        if (label === "Time zone") return fake({ count: 1, input: "UTC" });
        if (label === "Project context (optional)") return fake({ count: 1, input: "" });
        return absent();
      },
      role: (role: string, options?: Record<string, unknown>) => {
        if (role === "switch") return fake({ count: 1, attribute: "true" });
        if (role === "button" && options?.["name"] === "Advanced: edit raw RRULE") return toggle;
        return absent();
      },
    });
  }

  it("reveals the rule before reading the schedule values", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const dialog = scheduleDialog(fake({ count: 0, input: "FREQ=DAILY" }), toggle);
    const values = await new WebDriver(dialogPage(dialog)).schedulerProjectEditValues();
    expect(toggle.click).toHaveBeenCalledTimes(1);
    expect(values).toMatchObject({ dtstart: "2026-11-09T09:30", tzid: "UTC", rrule: "FREQ=DAILY", enabled: true });
  });

  it("reveals the rule before filling it", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 0, fill: vi.fn(async () => undefined) });
    const dialog = scheduleDialog(raw, toggle);
    await new WebDriver(dialogPage(dialog)).schedulerProjectEditFill({ rrule: "FREQ=WEEKLY" });
    expect(toggle.click).toHaveBeenCalledTimes(1);
    expect(raw.fill).toHaveBeenCalledWith("FREQ=WEEKLY");
  });

  it("fills the rule directly when the disclosure is already open", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const raw = fake({ count: 1, visible: true, fill: vi.fn(async () => undefined) });
    const dialog = scheduleDialog(raw, toggle);
    await new WebDriver(dialogPage(dialog)).schedulerProjectEditFill({ rrule: "FREQ=DAILY" });
    expect(toggle.click).not.toHaveBeenCalled();
    expect(raw.fill).toHaveBeenCalledWith("FREQ=DAILY");
  });

  it("leaves the disclosure alone when no rule is filled", async () => {
    const toggle = fake({ count: 1, click: vi.fn(async () => undefined) });
    const dialog = scheduleDialog(fake({ count: 0 }), toggle);
    await new WebDriver(dialogPage(dialog)).schedulerProjectEditFill({ dtstart: "2026-11-10T09:30" });
    expect(toggle.click).not.toHaveBeenCalled();
  });
});
