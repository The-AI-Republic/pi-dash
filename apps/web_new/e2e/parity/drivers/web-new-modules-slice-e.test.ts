// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the modules slice-E driver hooks (NEWFRONT-797,
// MOD-013/014/015/016): the row and card date triggers and their shared
// range calendar, the status pill and its menu, the lead mark and its
// tooltip, the favorite star, the sidebar favorites toggle, and the
// toast reads the scenarios await on. A Playwright-shaped fake answers
// from a chain-keyed table, so every case pins the selector, the
// attribute or the gesture the hook reads — no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  disabled?: boolean;
  text?: string;
  value?: string;
  attrs?: Record<string, string | null>;
}

type Table = Record<string, Answer>;

interface RecordedSelect {
  id: string;
  option: unknown;
}

class FakePage {
  readonly clicks: string[] = [];
  readonly hovers: string[] = [];
  readonly waits: string[] = [];
  readonly selects: RecordedSelect[] = [];
  private readonly table: Table;

  constructor(table: Table = {}) {
    this.table = table;
  }

  private answer(id: string): Answer & { count: number; visible: boolean } {
    // Keys name the tail of a locator chain, so a case reads as the element
    // it describes ("testid(modules-row-status)") and still answers when
    // the driver wraps it in another `.filter()` or `.first()`.
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.endsWith(` ${key}`) || id.endsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    return { count: 1, visible: true, ...(candidates[0]?.[1] ?? {}) };
  }

  locator(id: string, _options?: unknown): Locator {
    const self = this;
    void _options;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: (index: number) => self.locator(`${id} nth(${index})`),
      filter: (options: { has?: unknown; hasText?: string | RegExp }) =>
        self.locator(`${id} filter(${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      getByLabel: (label: string) => self.locator(`${id} label(${label})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      selectOption: async (option: unknown) => {
        self.selects.push({ id, option });
      },
      click: async () => {
        self.clicks.push(id);
      },
      hover: async () => {
        self.hovers.push(id);
      },
      waitFor: async () => {
        self.waits.push(id);
      },
    };
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    return {
      getByTestId: (testId: string) => self.locator(`page testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`page role(${role},${String(options?.name ?? "")})`),
      locator: (selector: string) => self.locator(`page css(${selector})`),
    } as unknown as Page;
  }
}

function driverWith(table: Table): { driver: WebNewDriver; page: FakePage } {
  const page = new FakePage(table);
  return { driver: new WebNewDriver(page.asPage()), page };
}

/** The calendar locator tail both the row and card calendars share. */
const CALENDAR_KEY =
  'css([data-testid="modules-row-dates-calendar"], [data-testid="modules-card-dates-control-calendar"])';

describe("modules slice-E drivers", () => {
  it("reads the row date trigger label whitespace-collapsed", async () => {
    const { driver, page } = driverWith({ "testid(modules-row-dates-trigger)": { text: "  Start date\nEnd date " } });
    await expect(driver.modulesRowDateText("Dated Module")).resolves.toBe("Start date End date");
    expect(page.waits).toHaveLength(1);
    expect(page.waits[0]).toContain("testid(modules-row-dates-trigger)");
  });

  it("reads whether the row date trigger disables", async () => {
    const enabled = driverWith({ "testid(modules-row-dates-trigger)": { disabled: false } });
    await expect(enabled.driver.modulesRowDateDisabled("Live Module")).resolves.toBe(false);
    const disabled = driverWith({ "testid(modules-row-dates-trigger)": { disabled: true } });
    await expect(disabled.driver.modulesRowDateDisabled("Live Module")).resolves.toBe(true);
  });

  it("opens the row calendar through the trigger when shut", async () => {
    const { driver, page } = driverWith({ [CALENDAR_KEY]: { count: 0 } });
    await driver.modulesRowOpenDates("Dated Module");
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain("testid(modules-row-dates-trigger)");
  });

  it("leaves an already-open row calendar alone", async () => {
    const { driver, page } = driverWith({ [CALENDAR_KEY]: { count: 1, visible: true } });
    await driver.modulesRowOpenDates("Dated Module");
    expect(page.clicks).toHaveLength(0);
  });

  it("reports the calendar missing, open, and present-but-hidden", async () => {
    const missing = driverWith({ [CALENDAR_KEY]: { count: 0 } });
    await expect(missing.driver.modulesCalendarVisible()).resolves.toBe(false);
    const open = driverWith({ [CALENDAR_KEY]: { count: 1, visible: true } });
    await expect(open.driver.modulesCalendarVisible()).resolves.toBe(true);
    const hidden = driverWith({ [CALENDAR_KEY]: { count: 1, visible: false } });
    await expect(hidden.driver.modulesCalendarVisible()).resolves.toBe(false);
  });

  it("picks a day cell from the captioned month", async () => {
    const { driver, page } = driverWith({
      [CALENDAR_KEY]: { count: 1, visible: true },
      "label(Month)": { value: "9" },
      "label(Year)": { value: "2026" },
    });
    await driver.modulesCalendarPickDay(4);
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain('button[data-day="2026-10-04"]');
  });

  it("drives the caption selects to the wanted month and verifies the read-back", async () => {
    const { driver, page } = driverWith({
      [CALENDAR_KEY]: { count: 1, visible: true },
      "label(Month) css(option:checked)": { text: "November" },
      "label(Year) css(option:checked)": { text: "2026" },
    });
    await driver.modulesCalendarGotoMonth("November", "2026");
    expect(page.selects).toHaveLength(2);
    expect(page.selects[0]).toMatchObject({ option: { label: "November" } });
    expect(page.selects[0]?.id).toContain("label(Month)");
    expect(page.selects[1]).toMatchObject({ option: { label: "2026" } });
    expect(page.selects[1]?.id).toContain("label(Year)");
  });

  it("reads the card date trigger label whitespace-collapsed", async () => {
    const { driver } = driverWith({
      "testid(modules-card-dates-control-trigger)": { text: "Start date  End date" },
    });
    await expect(driver.modulesCardDateText("Card Dated")).resolves.toBe("Start date End date");
  });

  it("reads whether the card date trigger disables", async () => {
    const enabled = driverWith({ "testid(modules-card-dates-control-trigger)": { disabled: false } });
    await expect(enabled.driver.modulesCardDateDisabled("Card Dated")).resolves.toBe(false);
    const disabled = driverWith({ "testid(modules-card-dates-control-trigger)": { disabled: true } });
    await expect(disabled.driver.modulesCardDateDisabled("Card Dated")).resolves.toBe(true);
  });

  it("opens the card calendar through the trigger when shut", async () => {
    const { driver, page } = driverWith({ [CALENDAR_KEY]: { count: 0 } });
    await driver.modulesCardOpenDates("Card Dated");
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain("testid(modules-card-dates-control-trigger)");
  });

  it("leaves an already-open card calendar alone", async () => {
    const { driver, page } = driverWith({ [CALENDAR_KEY]: { count: 1, visible: true } });
    await driver.modulesCardOpenDates("Card Dated");
    expect(page.clicks).toHaveLength(0);
  });

  it("reads the newest toast newest-first", async () => {
    const { driver } = driverWith({
      'css([data-testid="toast-notification"])': { count: 1 },
      "testid(toast-title)": { text: "Success!" },
      "testid(toast-description)": { text: "  Module updated\nsuccessfully. " },
    });
    await expect(driver.modulesLastToast()).resolves.toEqual({
      title: "Success!",
      message: "Module updated successfully.",
    });
  });

  it("reads no toast when the stack is empty or mid-dismiss", async () => {
    const empty = driverWith({ 'css([data-testid="toast-notification"])': { count: 0 } });
    await expect(empty.driver.modulesLastToast()).resolves.toBeNull();
    const blank = driverWith({
      'css([data-testid="toast-notification"])': { count: 1 },
      "testid(toast-title)": { text: "  " },
      "testid(toast-description)": { text: "" },
    });
    await expect(blank.driver.modulesLastToast()).resolves.toBeNull();
  });

  it("keeps a toast whose title is blank but whose message shows", async () => {
    const { driver } = driverWith({
      'css([data-testid="toast-notification"])': { count: 1 },
      "testid(toast-title)": { text: "" },
      "testid(toast-description)": { text: "Module added to favorites." },
    });
    await expect(driver.modulesLastToast()).resolves.toEqual({
      title: "",
      message: "Module added to favorites.",
    });
  });

  it("awaits a toast carrying the wanted text", async () => {
    const { driver } = driverWith({
      'css([data-testid="toast-notification"])': { count: 1 },
      "testid(toast-title)": { text: "Success!" },
      "testid(toast-description)": { text: "Module updated successfully." },
    });
    await driver.modulesAwaitToast("Module updated successfully.");
  });

  it("reads the pill label trimmed", async () => {
    const { driver } = driverWith({ "testid(modules-row-status)": { text: "  Planned " } });
    await expect(driver.modulesPillText("State Module")).resolves.toBe("Planned");
  });

  it("reads the pill color attribute, blank when absent", async () => {
    const painted = driverWith({ "testid(modules-row-status)": { attrs: { "data-status-color": "#3f76ff" } } });
    await expect(painted.driver.modulesPillColor("State Module")).resolves.toBe("#3f76ff");
    const bare = driverWith({ "testid(modules-row-status)": {} });
    await expect(bare.driver.modulesPillColor("State Module")).resolves.toBe("");
  });

  it("reads whether the status pill disables", async () => {
    const enabled = driverWith({ "testid(modules-row-status)": { disabled: false } });
    await expect(enabled.driver.modulesStatusDisabled("State Module")).resolves.toBe(false);
    const disabled = driverWith({ "testid(modules-row-status)": { disabled: true } });
    await expect(disabled.driver.modulesStatusDisabled("State Module")).resolves.toBe(true);
  });

  it("opens the status menu through the pill when shut", async () => {
    const { driver, page } = driverWith({ "role(menuitem,)": { count: 0 } });
    await driver.modulesOpenStatus("State Module");
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain("testid(modules-row-status)");
  });

  it("keeps an already-open status menu", async () => {
    const { driver, page } = driverWith({ "role(menuitem,)": { count: 6 } });
    await driver.modulesOpenStatus("State Module");
    expect(page.clicks).toHaveLength(0);
  });

  it("reads the status options in display order", async () => {
    const { driver } = driverWith({
      "role(menuitem,)": { count: 2 },
      "nth(0)": { text: "Backlog" },
      "nth(1)": { text: "  Planned\n" },
    });
    await expect(driver.modulesStatusOptions()).resolves.toEqual(["Backlog", "Planned"]);
  });

  it("picks a status option, then waits for the pill to show it", async () => {
    const { driver, page } = driverWith({});
    await driver.modulesPickStatus("Planned");
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain("role(menuitem,Planned)");
    expect(page.waits).toHaveLength(1);
    expect(page.waits[0]).toContain("filter(Planned)");
  });

  it("reads the lead slot avatar versus marker", async () => {
    const avatar = driverWith({ "testid(modules-row-lead)": { attrs: { "data-lead-kind": "avatar" } } });
    await expect(avatar.driver.modulesLeadState("Owned Module")).resolves.toBe("avatar");
    const none = driverWith({ "testid(modules-row-lead)": { attrs: { "data-lead-kind": "none" } } });
    await expect(none.driver.modulesLeadState("Led Module")).resolves.toBe("none");
  });

  it("skips the tooltip when a lead shows", async () => {
    const { driver, page } = driverWith({ "testid(modules-row-lead)": { attrs: { "data-lead-kind": "avatar" } } });
    await expect(driver.modulesLeadTooltip("Owned Module")).resolves.toBeNull();
    expect(page.hovers).toHaveLength(0);
  });

  it("hovers the marker to read the no-lead explanation", async () => {
    const { driver, page } = driverWith({
      "testid(modules-row-lead)": { attrs: { "data-lead-kind": "none" } },
      "role(tooltip,)": { text: "No lead" },
    });
    await expect(driver.modulesLeadTooltip("Led Module")).resolves.toBe("No lead");
    expect(page.hovers).toHaveLength(1);
    expect(page.hovers[0]).toContain("testid(modules-row-lead)");
  });

  it("reads a blank tooltip as no explanation", async () => {
    const { driver } = driverWith({
      "testid(modules-row-lead)": { attrs: { "data-lead-kind": "none" } },
      "role(tooltip,)": { text: "  " },
    });
    await expect(driver.modulesLeadTooltip("Led Module")).resolves.toBeNull();
  });

  it("reads whether the row offers the star", async () => {
    const offered = driverWith({ "testid(modules-row-star)": { count: 1 } });
    await expect(offered.driver.modulesStarVisible("Starred Module")).resolves.toBe(true);
    const hidden = driverWith({ "testid(modules-row-star)": { count: 0 } });
    await expect(hidden.driver.modulesStarVisible("Guest Module")).resolves.toBe(false);
  });

  it("reads the star pressed state", async () => {
    const selected = driverWith({ "testid(modules-row-star)": { attrs: { "aria-pressed": "true" } } });
    await expect(selected.driver.modulesStarSelected("Starred Module")).resolves.toBe(true);
    const cleared = driverWith({ "testid(modules-row-star)": { attrs: { "aria-pressed": "false" } } });
    await expect(cleared.driver.modulesStarSelected("Starred Module")).resolves.toBe(false);
  });

  it("toggles the star, resolving once a toast shows", async () => {
    const { driver, page } = driverWith({
      'css([data-testid="toast-notification"])': { count: 1 },
      "testid(toast-title)": { text: "Success!" },
      "testid(toast-description)": { text: "Module added to favorites." },
    });
    await driver.modulesToggleStar("Starred Module");
    expect(page.clicks).toHaveLength(1);
    expect(page.clicks[0]).toContain("testid(modules-row-star)");
  });

  it("reads the sidebar favorites toggle expanded, collapsed, and absent", async () => {
    const open = driverWith({ "testid(favorites-section-toggle)": { count: 1, attrs: { "aria-expanded": "true" } } });
    await expect(open.driver.modulesFavoritesExpanded()).resolves.toBe(true);
    const shut = driverWith({ "testid(favorites-section-toggle)": { count: 1, attrs: { "aria-expanded": "false" } } });
    await expect(shut.driver.modulesFavoritesExpanded()).resolves.toBe(false);
    const absent = driverWith({ "testid(favorites-section-toggle)": { count: 0 } });
    await expect(absent.driver.modulesFavoritesExpanded()).resolves.toBe(false);
  });
});
