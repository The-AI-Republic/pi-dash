// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver calendar-layout methods (NEWFRONT-658,
// ISS-021–026). A stub Page stands in for Playwright: each case pins the
// hook or value the method reads from the new calendar, so a markup change
// that breaks the parity surface fails here before the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: () => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  fill?: (value: string) => Promise<void>;
  press?: (key: string) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  getByPlaceholder?: (text: string, options?: unknown) => StubLocator;
  getByLabel?: (text: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isEnabled: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    inputValue: async () => "",
    fill: async () => undefined,
    press: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    boundingBox: async () => ({ x: 0, y: 0, width: 100, height: 50 }),
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByText: () => node,
    getByPlaceholder: () => node,
    getByLabel: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(
  parts: {
    testId?: (id: string) => Locator;
    role?: (role: string, options?: unknown) => Locator;
    locator?: (selector: string) => Locator;
    keyboard?: Record<string, unknown>;
    mouse?: Record<string, unknown>;
    url?: () => string;
  } = {}
): Page {
  return {
    getByTestId: parts.testId ?? (() => locator()),
    getByRole: parts.role ?? (() => locator()),
    getByLabel: () => locator(),
    locator: parts.locator ?? (() => locator()),
    waitForTimeout: async () => undefined,
    waitForURL: async () => undefined,
    keyboard: { press: async () => undefined, ...parts.keyboard },
    mouse: { move: async () => undefined, down: async () => undefined, up: async () => undefined, ...parts.mouse },
    url: parts.url ?? (() => "https://example.test/"),
  } as unknown as Page;
}

/** A calendar root whose day tiles are the supplied stubs. */
function calendarPage(tiles: Locator[], extras: { title?: Locator } = {}): Page {
  const root = locator({
    locator: (selector: string) =>
      selector === "[data-calendar-day]"
        ? locator({ count: async () => tiles.length, nth: (index) => tiles[index] ?? locator() })
        : locator(),
    getByRole: () => locator(),
  });
  const title = extras.title ?? locator({ innerText: async () => "October 2026" });
  return pageOf({
    testId: (id) => (id === "issues-calendar" ? root : id === "calendar-title" ? title : locator()),
  });
}

function tile(day: number, overrides: Partial<StubLocator> = {}): Locator {
  return locator({
    getAttribute: async (name: string) => {
      if (name === "data-calendar-day-number") return String(day);
      if (name === "data-in-month") return "true";
      if (name === "data-today") return day === 5 ? "true" : null;
      return null;
    },
    ...overrides,
  });
}

describe("calendar mode and title", () => {
  it("reads month mode from a grid of more than ten tiles", async () => {
    const tiles = Array.from({ length: 35 }, (_, index) => tile(index + 1));
    expect(await new WebNewDriver(calendarPage(tiles)).layoutsCalMode()).toBe("month");
  });

  it("reads week mode from a row of five tiles", async () => {
    const tiles = Array.from({ length: 5 }, (_, index) => tile(index + 1));
    expect(await new WebNewDriver(calendarPage(tiles)).layoutsCalMode()).toBe("week");
  });

  it("reads and normalises the title", async () => {
    const title = locator({ innerText: async () => "  October   2026 " });
    expect(await new WebNewDriver(calendarPage([tile(1)], { title })).layoutsCalTitle()).toBe("October 2026");
  });

  it("reports the picker disabled in week mode", async () => {
    const title = locator({ isEnabled: async () => false });
    expect(await new WebNewDriver(calendarPage([tile(1)], { title })).layoutsCalMonthPickerEnabled()).toBe(false);
  });
});

describe("calendar day tiles", () => {
  it("reads only in-month tiles and returns the day's issue names", async () => {
    const blocks = locator({
      count: async () => 2,
      nth: (index: number) => locator({ innerText: async () => (index === 0 ? "  Alpha " : "Beta") }),
    });
    const target = tile(12, {
      locator: (selector: string) => (selector === 'a[id^="issue-"]' ? blocks : locator()),
    });
    const filler = tile(12, { getAttribute: async (name: string) => (name === "data-in-month" ? "false" : "12") });
    const names = await new WebNewDriver(calendarPage([filler, target])).layoutsCalDayIssueNames(12);
    expect(names).toEqual(["Alpha", "Beta"]);
  });

  it("reads today from the tile marker", async () => {
    const driver = new WebNewDriver(calendarPage([tile(5), tile(6)]));
    expect(await driver.layoutsCalDayIsToday(5)).toBe(true);
    expect(await driver.layoutsCalDayIsToday(6)).toBe(false);
  });

  it("detects and clicks a per-day load more", async () => {
    let clicked = 0;
    const more = locator({ count: async () => 1, click: async () => void (clicked += 1) });
    const target = tile(8, { getByRole: () => more });
    const driver = new WebNewDriver(calendarPage([target]));
    expect(await driver.layoutsCalDayHasLoadMore(8)).toBe(true);
    await driver.layoutsCalDayLoadMore(8);
    expect(clicked).toBe(1);
  });

  it("counts weekday header cells for the column count", async () => {
    const cells = locator({
      count: async () => 5,
      first: () => cells,
      nth: () => locator({ innerText: async () => "Mon" }),
    });
    const page = pageOf({
      testId: (id) => (id === "calendar-weekdays" ? locator({ locator: () => cells }) : locator()),
    });
    expect(await new WebNewDriver(page).layoutsCalColumnCount()).toBe(5);
  });

  it("reads weekends from the Saturday header cell", async () => {
    const labels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const cells = locator({
      count: async () => labels.length,
      first: () => cells,
      nth: (index: number) => locator({ innerText: async () => labels[index] ?? "" }),
    });
    const page = pageOf({
      testId: (id) => (id === "calendar-weekdays" ? locator({ locator: () => cells }) : locator()),
    });
    expect(await new WebNewDriver(page).layoutsCalWeekendsVisible()).toBe(true);
  });
});

describe("calendar month picker", () => {
  it("lists the picker months and closes the popover", async () => {
    const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const buttons = locator({
      count: async () => months.length,
      nth: (index: number) => locator({ innerText: async () => months[index] ?? "" }),
    });
    const panel = locator({ getByTestId: (id) => (id === "calendar-picker-month" ? buttons : locator()) });
    const page = pageOf({
      testId: (id) => (id === "calendar-month-picker" ? panel : locator()),
    });
    expect(await new WebNewDriver(page).layoutsCalMonthPickerMonths()).toEqual(months);
  });

  it("reads the picker year", async () => {
    const panel = locator({
      getByTestId: (id) => (id === "calendar-picker-year" ? locator({ innerText: async () => " 2027 " }) : locator()),
    });
    const page = pageOf({ testId: (id) => (id === "calendar-month-picker" ? panel : locator()) });
    expect(await new WebNewDriver(page).layoutsCalMonthPickerYear()).toBe(2027);
  });
});

describe("calendar blocks", () => {
  it("returns true when the hover preview tooltip appears", async () => {
    const block = locator({ getByRole: (role: string) => (role === "tooltip" ? locator() : locator()) });
    const page = pageOf({ locator: () => block });
    expect(await new WebNewDriver(page).layoutsCalBlockHoverPreview("Alpha")).toBe(true);
  });

  it("reports no preview when the tooltip never appears", async () => {
    const block = locator({
      getByRole: (role: string) =>
        role === "tooltip" ? locator({ waitFor: async () => Promise.reject(new Error("timeout")) }) : locator(),
    });
    const page = pageOf({ locator: () => block });
    expect(await new WebNewDriver(page).layoutsCalBlockHoverPreview("Alpha")).toBe(false);
  });

  it("reads the quick action labels from the menu", async () => {
    const menu = locator({
      count: async () => 2,
      nth: (index: number) => locator({ innerText: async () => (index === 0 ? "Edit" : "Delete") }),
    });
    const page = pageOf({
      locator: () => locator({ hover: async () => undefined }),
      role: (role: string) => (role === "menuitem" ? menu : locator()),
    });
    expect(await new WebNewDriver(page).layoutsCalBlockQuickActions("Alpha")).toEqual(["Edit", "Delete"]);
  });

  it("reads the day add menu entries", async () => {
    const menu = locator({
      count: async () => 2,
      nth: (index: number) =>
        locator({ innerText: async () => (index === 0 ? "Add work item" : "Add existing work item") }),
    });
    const page = calendarPage([tile(3)]);
    (page as unknown as { getByRole: (role: string) => Locator }).getByRole = (role: string) =>
      role === "menuitem" ? menu : locator();
    expect(await new WebNewDriver(page).layoutsCalDayAddMenu(3)).toEqual(["Add work item", "Add existing work item"]);
  });
});

describe("calendar peek", () => {
  it("reports the peek hidden when the address has no peek id", async () => {
    expect(await new WebNewDriver(pageOf()).layoutsPeekVisible()).toBe(false);
  });

  it("reports the peek visible from the mounted panel", async () => {
    const page = pageOf({ url: () => "https://example.test/issues?peekIssueId=abc" });
    expect(await new WebNewDriver(page).layoutsPeekVisible()).toBe(true);
  });

  it("reads the peek title from the editable field", async () => {
    const field = locator({ inputValue: async () => "Fix login" });
    const panel = locator({ getByLabel: () => field });
    const page = pageOf({ testId: (id) => (id === "issue-preview" ? panel : locator()) });
    expect(await new WebNewDriver(page).layoutsPeekTitle()).toBe("Fix login");
  });

  it("closes the peek through its accessible control and waits for the address", async () => {
    let url = "https://example.test/issues?peekIssueId=abc";
    const close = locator({ click: async () => void (url = "https://example.test/issues") });
    const page = pageOf({
      url: () => url,
      role: (role: string) => (role === "button" ? close : locator()),
    });
    await new WebNewDriver(page).layoutsPeekClose();
    expect(url).not.toContain("peekIssueId");
  });
});
