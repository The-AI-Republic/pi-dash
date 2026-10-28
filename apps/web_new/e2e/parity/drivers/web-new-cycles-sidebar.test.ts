// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycle sidebar methods (NEWFRONT-713,
// CYC-035–CYC-040): the panel readers, the collapse writers, the inline
// date controls, the lead and description readers, the progress and
// breakdown readers, the entry clicks, and the skeleton helpers. A
// recording fake stands in for Playwright: each locator remembers the
// path of calls that built it, scripted answers stand in for the page,
// and a test asserts both which selector the driver used and what it
// does with the answer. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "getByLabel", "getByText", "locator", "filter"];
const ACTIONS = [
  "count",
  "click",
  "innerText",
  "allInnerTexts",
  "isVisible",
  "isDisabled",
  "waitFor",
  "selectOption",
  "scrollIntoViewIfNeeded",
  "getAttribute",
  "evaluate",
];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "function") return "<fn>";
      if (arg instanceof RegExp) return String(arg);
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") return JSON.stringify(arg);
      return String(arg);
    })
    .join(",");
}

const SIDEBAR = `page.getByTestId("cycle-sidebar")`;
const SIDEBAR_NAME = `${SIDEBAR}.getByTestId("cycle-sidebar-name")`;
const TOGGLE = `page.getByTestId("cycle-sidebar-toggle")`;
const TOGGLE_NARROW = `page.getByTestId("cycle-sidebar-toggle-narrow")`;
const PANEL_CLOSE = `${SIDEBAR}.getByTestId("cycle-sidebar-close")`;
const DATE_TRIGGER = `${SIDEBAR}.getByTestId("cycle-sidebar-dates-trigger")`;
const DATE_CALENDAR = `page.getByTestId("cycle-sidebar-dates-picker-calendar")`;
const DESCRIPTION = `${SIDEBAR}.getByTestId("cycle-sidebar-description")`;
const LEAD = `${SIDEBAR}.getByTestId("cycle-sidebar-lead")`;
const LEAD_NAME = `${LEAD}.getByTestId("cycle-sidebar-lead-name")`;
const LEAD_IMAGE = `${LEAD}.locator("img").first()`;
const LEAD_GLYPH = `${LEAD}.getByRole("img").first()`;
const WORK_ITEMS = `${SIDEBAR}.getByTestId("cycle-progress-work-items")`;
const POINTS = `${SIDEBAR}.getByTestId("cycle-progress-points")`;
const CHART = `${SIDEBAR}.getByTestId("cycle-progress-chart")`;
const MEASURE = `${SIDEBAR}.getByTestId("cycle-progress-measure")`;
const OPTIONS = `page.getByRole("option")`;
const ENTRIES = `${SIDEBAR}.getByTestId("cycle-progress-entries")`;
const ENTRY_ROWS = `${ENTRIES}.locator("[data-testid^=\\"cycle-progress-entry-\\"]")`;
const EMPTY_NOTICE = `${SIDEBAR}.getByText("No Data yet",{"exact":true})`;
const SWITCHER_TRIGGER = `page.getByTestId("cycle-switcher-trigger")`;
const SIDEBAR_SKELETON = `page.getByTestId("cycle-sidebar-skeleton")`;
const PEEK_SKELETON = `page.getByTestId("cycle-peek-skeleton")`;
const PEEK_HEADING = `page.getByTestId("cycle-preview").locator("h4").first()`;

interface ScriptedRoute {
  pattern: string;
  predicate: ((url: URL) => boolean) | null;
  handler: (route: unknown) => Promise<void>;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly routes: ScriptedRoute[] = [];
  readonly presses: string[] = [];
  readonly navigations: string[] = [];
  readonly reloads: number[] = [];
  collapsedStored = false;
  rowTexts: string[] = [];
  optionTexts: string[] = [];
  readonly counts = new Map<string, number>();
  readonly texts = new Map<string, string>();
  readonly visibles = new Map<string, boolean>();
  readonly disableds = new Map<string, boolean>();
  readonly attributes = new Map<string, string | null>();
  selectedAria = "true";

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path, method, args });
        return this.locator(`${path}.${method}(${render(args)})`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        const call = { path, method, args };
        this.calls.push(call);
        return this.respond(call);
      };
    }
    return node as unknown as Locator;
  }

  respond(call: Call): unknown {
    const { path, method } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText") return this.text(path);
    if (method === "allInnerTexts") return path === OPTIONS || path === `${OPTIONS}.first()` ? this.optionTexts : [];
    if (method === "isVisible") return this.visible(path);
    if (method === "isDisabled") return this.disableds.get(path) ?? false;
    if (method === "getAttribute") return this.attributes.get(path) ?? this.selectedAria;
    if (
      method === "waitFor" ||
      method === "click" ||
      method === "selectOption" ||
      method === "scrollIntoViewIfNeeded"
    ) {
      return undefined;
    }
    if (method === "evaluate") throw new Error(`unexpected evaluate at ${path}`);
    throw new Error(`unexpected action ${method} at ${path}`);
  }

  private count(path: string): number {
    if (this.counts.has(path)) return this.counts.get(path) ?? 0;
    if (path === ENTRY_ROWS) return this.rowTexts.length;
    return 1;
  }

  private text(path: string): string {
    if (this.texts.has(path)) return this.texts.get(path) ?? "";
    const nth = /nth\((\d+)\)$/.exec(path);
    if (nth !== null && path.startsWith(ENTRY_ROWS)) return this.rowTexts[Number(nth[1])] ?? "";
    return "";
  }

  private visible(path: string): boolean {
    return this.visibles.get(path) ?? true;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByTestId", "getByLabel", "getByText", "locator"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.keyboard = {
      press: async (key: string) => {
        this.presses.push(key);
      },
    };
    node.evaluate = async (fn: unknown) => {
      this.calls.push({ path: "page", method: "evaluate", args: [fn] });
      return this.collapsedStored;
    };
    node.route = async (pattern: unknown, handler: (route: unknown) => Promise<void>) => {
      this.calls.push({ path: "page", method: "route", args: [pattern] });
      this.routes.push({
        pattern: typeof pattern === "function" ? "<fn>" : String(pattern),
        predicate: typeof pattern === "function" ? (pattern as (url: URL) => boolean) : null,
        handler,
      });
    };
    node.goto = async (url: string) => {
      this.navigations.push(url);
    };
    node.reload = async () => {
      this.reloads.push(1);
    };
    node.waitForLoadState = async () => undefined;
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }

  entryButtonCall(): Call {
    const found = this.calls.find((call) => call.path === ENTRIES && call.method === "getByRole");
    if (found === undefined) throw new Error("no entry button lookup recorded");
    return found;
  }
}

describe("cyclesDetailSidebarName", () => {
  it("reads the panel heading", async () => {
    const script = new Recorder();
    script.texts.set(SIDEBAR_NAME, "Cycle 7");
    expect(await script.driver().cyclesDetailSidebarName()).toBe("Cycle 7");
    expect(script.callsFor(SIDEBAR_NAME, "innerText")).toHaveLength(1);
  });

  it("reads null when the panel is gone, hidden, or blank", async () => {
    const gone = new Recorder();
    gone.counts.set(SIDEBAR_NAME, 0);
    expect(await gone.driver().cyclesDetailSidebarName()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(SIDEBAR_NAME, false);
    expect(await hidden.driver().cyclesDetailSidebarName()).toBeNull();
    const blank = new Recorder();
    blank.texts.set(SIDEBAR_NAME, "   ");
    expect(await blank.driver().cyclesDetailSidebarName()).toBeNull();
  });
});

describe("cyclesSwitcherPick", () => {
  it("clicks the option and settles on the trigger showing the pick", async () => {
    const script = new Recorder();
    script.texts.set(SWITCHER_TRIGGER, "Cycle B");
    await script.driver().cyclesSwitcherPick("Cycle B");
    expect(script.callsFor(`page.getByRole("option",{"name":"Cycle B"}).first()`, "click")).toHaveLength(1);
    expect(script.callsFor(SWITCHER_TRIGGER, "innerText").length).toBeGreaterThan(0);
  });
});

describe("cyclesToggleSidebarViaHeader", () => {
  it("clicks the header toggle on wide screens", async () => {
    const script = new Recorder();
    await script.driver().cyclesToggleSidebarViaHeader();
    expect(script.callsFor(`${TOGGLE}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(TOGGLE_NARROW, "click")).toHaveLength(0);
  });

  it("falls back to the narrow toggle", async () => {
    const script = new Recorder();
    script.visibles.set(`${TOGGLE}.first()`, false);
    await script.driver().cyclesToggleSidebarViaHeader();
    expect(script.callsFor(TOGGLE_NARROW, "click")).toHaveLength(1);
  });

  it("falls back when no desktop toggle mounts", async () => {
    const script = new Recorder();
    script.counts.set(TOGGLE, 0);
    await script.driver().cyclesToggleSidebarViaHeader();
    expect(script.callsFor(TOGGLE_NARROW, "click")).toHaveLength(1);
  });
});

describe("cyclesCloseSidebarViaPanel", () => {
  it("settles the panel and clicks its close", async () => {
    const script = new Recorder();
    await script.driver().cyclesCloseSidebarViaPanel();
    expect(script.callsFor(SIDEBAR_NAME, "waitFor")).toHaveLength(1);
    expect(script.callsFor(PANEL_CLOSE, "click")).toHaveLength(1);
  });
});

describe("cyclesSidebarCollapsedStored", () => {
  it("reads the stored flag from the page", async () => {
    const script = new Recorder();
    script.collapsedStored = true;
    expect(await script.driver().cyclesSidebarCollapsedStored()).toBe(true);
    expect(script.callsFor("page", "evaluate")).toHaveLength(1);
    script.collapsedStored = false;
    expect(await script.driver().cyclesSidebarCollapsedStored()).toBe(false);
  });
});

describe("cyclesSidebarDateText", () => {
  it("reads the trigger span", async () => {
    const script = new Recorder();
    script.texts.set(DATE_TRIGGER, "Sep 01 - Oct 18, 2026");
    expect(await script.driver().cyclesSidebarDateText()).toBe("Sep 01 - Oct 18, 2026");
  });
});

describe("cyclesSidebarDateDisabled", () => {
  it("reports the trigger state", async () => {
    const script = new Recorder();
    script.disableds.set(DATE_TRIGGER, true);
    expect(await script.driver().cyclesSidebarDateDisabled()).toBe(true);
    const editable = new Recorder();
    expect(await editable.driver().cyclesSidebarDateDisabled()).toBe(false);
  });
});

describe("cyclesSidebarOpenDatePicker", () => {
  it("opens the trigger and waits for the calendar", async () => {
    const script = new Recorder();
    await script.driver().cyclesSidebarOpenDatePicker();
    expect(script.callsFor(DATE_TRIGGER, "click")).toHaveLength(1);
    expect(script.callsFor(DATE_CALENDAR, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesPickDateDay", () => {
  it.each([
    ["October 15th, 2026", "2026-10-15"],
    ["September 1st, 2026", "2026-09-01"],
    ["June 2nd, 2026", "2026-06-02"],
    ["March 3rd, 2026", "2026-03-03"],
  ])("picks %s as %s", async (label, iso) => {
    const script = new Recorder();
    await script.driver().cyclesPickDateDay(label);
    const months = script.callsFor(`${DATE_CALENDAR}.getByLabel("Month",{"exact":true})`, "selectOption");
    expect(months).toHaveLength(1);
    expect(months[0]?.args[0]).toEqual({
      label: new Date(`${iso}T12:00:00`).toLocaleString("en-US", { month: "long" }),
    });
    const years = script.callsFor(`${DATE_CALENDAR}.getByLabel("Year",{"exact":true})`, "selectOption");
    expect(years).toHaveLength(1);
    expect(years[0]?.args[0]).toEqual({ label: iso.slice(0, 4) });
    expect(script.callsFor(`${DATE_CALENDAR}.locator("button[data-day=\\"${iso}\\"]")`, "click")).toHaveLength(1);
  });

  it("refuses an unreadable label", async () => {
    const script = new Recorder();
    await expect(script.driver().cyclesPickDateDay("someday")).rejects.toThrow("cannot read the day label");
    await expect(script.driver().cyclesPickDateDay("Smarch 1st, 2026")).rejects.toThrow("cannot read the day label");
  });
});

describe("cyclesDismissDatePicker", () => {
  it("escapes and waits for the calendar to detach", async () => {
    const script = new Recorder();
    await script.driver().cyclesDismissDatePicker();
    expect(script.presses).toEqual(["Escape"]);
    const waits = script.callsFor(DATE_CALENDAR, "waitFor");
    expect(waits).toHaveLength(1);
    expect(waits[0]?.args[0]).toMatchObject({ state: "detached" });
  });
});

describe("cyclesCreatePastDayDisabled", () => {
  /** Local `yyyy-MM-dd` for a day offset from now. */
  function localIso(offset: number): string {
    const at = new Date();
    at.setDate(at.getDate() + offset);
    return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`;
  }

  /** The recorded path of one day cell in the dialog's range calendar. */
  function cellPath(iso: string): string {
    return `page.getByTestId("cycle-date-range-calendar").locator(${JSON.stringify(`button[data-day="${iso}"]`)})`;
  }

  it("probes the viewer's local today, not the project zone", async () => {
    const script = new Recorder();
    const yesterday = localIso(-1);
    const today = localIso(0);
    script.disableds.set(cellPath(yesterday), true);
    script.disableds.set(cellPath(today), false);
    expect(await script.driver().cyclesCreatePastDayDisabled()).toBe(true);
    // Both boundary days were read as cells, each after turning the grid.
    expect(script.callsFor(cellPath(yesterday), "isDisabled")).toHaveLength(1);
    expect(script.callsFor(cellPath(today), "isDisabled")).toHaveLength(1);
    const monthSelects = script.calls.filter(
      (call) => call.path.includes('getByLabel("Month"') && call.method === "selectOption"
    );
    expect(monthSelects).toHaveLength(2);
    expect(script.presses).toEqual(["Escape"]);
  });

  it("keeps the same rule in a zone ahead of the project's", async () => {
    const tz = process.env["TZ"];
    process.env["TZ"] = "Pacific/Auckland";
    try {
      const script = new Recorder();
      const yesterday = localIso(-1);
      const today = localIso(0);
      script.disableds.set(cellPath(yesterday), true);
      script.disableds.set(cellPath(today), false);
      expect(await script.driver().cyclesCreatePastDayDisabled()).toBe(true);
      expect(script.callsFor(cellPath(yesterday), "isDisabled")).toHaveLength(1);
      expect(script.callsFor(cellPath(today), "isDisabled")).toHaveLength(1);
    } finally {
      if (tz === undefined) delete process.env["TZ"];
      else process.env["TZ"] = tz;
    }
  });

  it("reports false when today itself is disabled", async () => {
    const script = new Recorder();
    script.disableds.set(cellPath(localIso(0)), true);
    expect(await script.driver().cyclesCreatePastDayDisabled()).toBe(false);
  });
});

describe("cyclesSidebarDescription", () => {
  it("reads the description body", async () => {
    const script = new Recorder();
    script.texts.set(DESCRIPTION, "Drives the release train.");
    expect(await script.driver().cyclesSidebarDescription()).toBe("Drives the release train.");
  });

  it("reads null when the description is gone, hidden, or blank", async () => {
    const gone = new Recorder();
    gone.counts.set(DESCRIPTION, 0);
    expect(await gone.driver().cyclesSidebarDescription()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(DESCRIPTION, false);
    expect(await hidden.driver().cyclesSidebarDescription()).toBeNull();
    const blank = new Recorder();
    blank.texts.set(DESCRIPTION, "  ");
    expect(await blank.driver().cyclesSidebarDescription()).toBeNull();
  });
});

describe("cyclesSidebarLead", () => {
  it("reads the name with the avatar image", async () => {
    const script = new Recorder();
    script.texts.set(LEAD_NAME, "Ada Lovelace");
    script.attributes.set(LEAD_IMAGE, "https://cdn.test/ada.png");
    expect(await script.driver().cyclesSidebarLead()).toEqual({
      name: "Ada Lovelace",
      avatar: "https://cdn.test/ada.png",
    });
  });

  it("reads the fallback initial without an image", async () => {
    const script = new Recorder();
    script.texts.set(LEAD_NAME, "Ada Lovelace");
    script.counts.set(LEAD_IMAGE, 0);
    script.texts.set(LEAD_GLYPH, "A");
    expect(await script.driver().cyclesSidebarLead()).toEqual({ name: "Ada Lovelace", avatar: "A" });
  });
});

describe("cyclesProgressWorkItemsText", () => {
  it("reads the work-items pair", async () => {
    const script = new Recorder();
    script.texts.set(WORK_ITEMS, "1/3");
    expect(await script.driver().cyclesProgressWorkItemsText()).toBe("1/3");
  });
});

describe("cyclesProgressPointsText", () => {
  it("reads the points pair", async () => {
    const script = new Recorder();
    script.texts.set(POINTS, "0/2");
    expect(await script.driver().cyclesProgressPointsText()).toBe("0/2");
  });

  it("reads null when the row is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(POINTS, 0);
    expect(await gone.driver().cyclesProgressPointsText()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(POINTS, false);
    expect(await hidden.driver().cyclesProgressPointsText()).toBeNull();
  });
});

describe("cyclesProgressChartVisible", () => {
  it("reports the chart mount", async () => {
    expect(await new Recorder().driver().cyclesProgressChartVisible()).toBe(true);
    const gone = new Recorder();
    gone.counts.set(CHART, 0);
    expect(await gone.driver().cyclesProgressChartVisible()).toBe(false);
    const hidden = new Recorder();
    hidden.visibles.set(CHART, false);
    expect(await hidden.driver().cyclesProgressChartVisible()).toBe(false);
  });
});

describe("cyclesProgressMeasureValue", () => {
  it("reads the active measure", async () => {
    const script = new Recorder();
    script.texts.set(MEASURE, "Estimates");
    expect(await script.driver().cyclesProgressMeasureValue()).toBe("Estimates");
  });

  it("reads null when the picker is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(MEASURE, 0);
    expect(await gone.driver().cyclesProgressMeasureValue()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(MEASURE, false);
    expect(await hidden.driver().cyclesProgressMeasureValue()).toBeNull();
  });
});

describe("cyclesProgressMeasureOptions", () => {
  it("opens the picker and reads the options", async () => {
    const script = new Recorder();
    script.optionTexts = ["Work items", "Estimates"];
    expect(await script.driver().cyclesProgressMeasureOptions()).toEqual(["Work items", "Estimates"]);
    expect(script.callsFor(`${MEASURE}.first()`, "scrollIntoViewIfNeeded")).toHaveLength(1);
    expect(script.callsFor(`${MEASURE}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(`${OPTIONS}.first()`, "waitFor")).toHaveLength(1);
    expect(script.presses).toEqual(["Escape"]);
  });

  it("reads no options without a picker", async () => {
    const script = new Recorder();
    script.counts.set(MEASURE, 0);
    expect(await script.driver().cyclesProgressMeasureOptions()).toEqual([]);
  });
});

describe("cyclesProgressPickMeasure", () => {
  it("picks the named option and waits for the trigger to follow", async () => {
    const script = new Recorder();
    script.texts.set(MEASURE, "Estimates");
    await script.driver().cyclesProgressPickMeasure("Estimates");
    expect(script.callsFor(MEASURE, "click")).toHaveLength(1);
    const options = script.calls.filter((call) => call.path === "page" && call.method === "getByRole");
    expect(options).toHaveLength(1);
    expect(options[0]?.args).toEqual(["option", { name: "Estimates" }]);
  });
});

describe("cyclesProgressStatsTab", () => {
  it("clicks the tab and waits for selection", async () => {
    const script = new Recorder();
    await script.driver().cyclesProgressStatsTab("Labels");
    const path = `${SIDEBAR}.getByRole("tab",{"name":"Labels"})`;
    expect(script.callsFor(path, "scrollIntoViewIfNeeded")).toHaveLength(1);
    expect(script.callsFor(path, "click")).toHaveLength(1);
    expect(script.callsFor(path, "getAttribute")).not.toHaveLength(0);
  });
});

describe("cyclesProgressStatsRows", () => {
  it("parses the entry rows", async () => {
    const script = new Recorder();
    script.rowTexts = ["Backlog 33% of 3", "Ada Lovelace 67% of 3", "Unlabeled 0% of 3"];
    expect(await script.driver().cyclesProgressStatsRows()).toEqual([
      { title: "Backlog", percent: 33, total: 3 },
      { title: "Ada Lovelace", percent: 67, total: 3 },
      { title: "Unlabeled", percent: 0, total: 3 },
    ]);
  });

  it("skips rows without a title or shape", async () => {
    const script = new Recorder();
    script.rowTexts = ["33% of 3", "just words", " Bug  100%  of  2 "];
    expect(await script.driver().cyclesProgressStatsRows()).toEqual([{ title: "Bug", percent: 100, total: 2 }]);
  });
});

describe("cyclesProgressEmptyText", () => {
  it("reads the undated notice", async () => {
    expect(await new Recorder().driver().cyclesProgressEmptyText()).toBe("No Data yet");
  });

  it("reads null when the notice is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(EMPTY_NOTICE, 0);
    expect(await gone.driver().cyclesProgressEmptyText()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(`${EMPTY_NOTICE}.first()`, false);
    expect(await hidden.driver().cyclesProgressEmptyText()).toBeNull();
  });
});

describe("cyclesProgressEntryClick", () => {
  it("clicks the matching entry case-insensitively", async () => {
    const script = new Recorder();
    await script.driver().cyclesProgressEntryClick("backlog");
    const lookup = script.entryButtonCall();
    expect(lookup.args[0]).toBe("button");
    const pattern = (lookup.args[1] as { name: RegExp }).name;
    expect(pattern).toBeInstanceOf(RegExp);
    expect(pattern.test("Backlog 33% of 3")).toBe(true);
    expect(pattern.test("Completed 33% of 3")).toBe(false);
    const path = `${ENTRIES}.getByRole("button",${JSON.stringify({ name: pattern })})`;
    expect(script.callsFor(`${path}.first()`, "scrollIntoViewIfNeeded")).toHaveLength(1);
    expect(script.callsFor(`${path}.first()`, "click")).toHaveLength(1);
  });
});

describe("cyclesHeroOpenDetail", () => {
  it("opens the detail address", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenDetail("acme", "pid", "cid");
    expect(script.navigations).toEqual(["/acme/projects/pid/cycles/cid"]);
  });
});

describe("cyclesHeroOpenPeekRaw", () => {
  it("opens the list with the peek query", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroOpenPeekRaw("acme", "pid", "cid");
    expect(script.navigations).toEqual(["/acme/projects/pid/cycles?peekCycle=cid"]);
  });
});

describe("cyclesHeroReload", () => {
  it("reloads the page", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroReload();
    expect(script.reloads).toHaveLength(1);
  });
});

describe("cyclesHeroDelayDetailReads", () => {
  it("holds cycle GETs and passes the rest through", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroDelayDetailReads(5);
    expect(script.routes).toHaveLength(1);
    const route = script.routes[0];
    if (!route?.predicate) throw new Error("delay predicate missing");
    const predicate = route.predicate;
    expect(predicate(new URL("https://api.test/api/workspaces/w/projects/p/cycles/"))).toBe(true);
    expect(predicate(new URL("https://api.test/api/workspaces/w/projects/p/cycles/abc/progress/"))).toBe(true);
    expect(predicate(new URL("https://api.test/api/workspaces/w/projects/p/issues/"))).toBe(false);
    let continued = 0;
    const started = Date.now();
    await route.handler({
      request: () => ({ method: () => "GET" }),
      continue: async () => {
        continued += 1;
      },
    });
    expect(Date.now() - started).toBeGreaterThanOrEqual(4);
    expect(continued).toBe(1);
    await route.handler({
      request: () => ({ method: () => "POST" }),
      continue: async () => {
        continued += 1;
      },
    });
    expect(continued).toBe(2);
  });
});

describe("cyclesHeroHideCycleFromListReads", () => {
  it("strips the cycle from bare-array lists", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroHideCycleFromListReads("cid");
    expect(script.routes).toHaveLength(1);
    const route = script.routes[0];
    if (!route?.predicate) throw new Error("hide predicate missing");
    const predicate = route.predicate;
    expect(predicate(new URL("https://api.test/api/workspaces/w/projects/p/cycles/"))).toBe(true);
    expect(predicate(new URL("https://api.test/api/workspaces/w/projects/p/cycles/cid/"))).toBe(false);
    let fulfilled: unknown = null;
    await route.handler({
      fetch: async () => ({ json: async () => [{ id: "cid" }, { id: "other" }] }),
      fulfill: async (body: unknown) => {
        fulfilled = body;
      },
    });
    expect(fulfilled).toMatchObject({ json: [{ id: "other" }] });
  });

  it("passes non-list reads through", async () => {
    const script = new Recorder();
    await script.driver().cyclesHeroHideCycleFromListReads("cid");
    const route = script.routes[0];
    if (!route) throw new Error("hide route missing");
    let fulfilled: unknown = null;
    const response = { status: () => 200 };
    await route.handler({
      fetch: async () => ({ ...response, json: async () => ({ id: "cid" }) }),
      fulfill: async (body: unknown) => {
        fulfilled = body;
      },
    });
    expect(fulfilled).toMatchObject({ response: expect.objectContaining({ status: expect.any(Function) }) });
  });
});

describe("cyclesHeroSidebarSkeletonVisible", () => {
  it("reports the sidebar skeleton", async () => {
    expect(await new Recorder().driver().cyclesHeroSidebarSkeletonVisible()).toBe(true);
    const gone = new Recorder();
    gone.counts.set(SIDEBAR_SKELETON, 0);
    expect(await gone.driver().cyclesHeroSidebarSkeletonVisible()).toBe(false);
  });
});

describe("cyclesHeroSidebarName", () => {
  it("reads through the sidebar name", async () => {
    const script = new Recorder();
    script.texts.set(SIDEBAR_NAME, "Cycle 7");
    expect(await script.driver().cyclesHeroSidebarName()).toBe("Cycle 7");
  });
});

describe("cyclesHeroPeekSkeletonVisible", () => {
  it("reports the peek skeleton", async () => {
    expect(await new Recorder().driver().cyclesHeroPeekSkeletonVisible()).toBe(true);
    const gone = new Recorder();
    gone.counts.set(PEEK_SKELETON, 0);
    expect(await gone.driver().cyclesHeroPeekSkeletonVisible()).toBe(false);
  });
});

describe("cyclesHeroPeekName", () => {
  it("reads the peek heading", async () => {
    const script = new Recorder();
    script.texts.set(PEEK_HEADING, "Cycle 7");
    expect(await script.driver().cyclesHeroPeekName()).toBe("Cycle 7");
  });

  it("reads null when the peek is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(PEEK_HEADING, 0);
    expect(await gone.driver().cyclesHeroPeekName()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(PEEK_HEADING, false);
    expect(await hidden.driver().cyclesHeroPeekName()).toBeNull();
  });
});

describe("cycle sidebar driver timeouts", () => {
  it("leaves the shared wait budget untouched", () => {
    const budget = (WebNewDriver as unknown as Record<string, unknown>)["WAIT_MS"];
    expect(budget).toBe(30_000);
  });
});
