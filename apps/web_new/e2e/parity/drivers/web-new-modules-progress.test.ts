// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver module progress methods (NEWFRONT-804,
// MOD-034): the disclosure readers, the header percent, the notices, the
// chart and measure readers, the refetch counter, the breakdown tabs,
// the entry rows and clicks, and the cross-cutting sidebar check. A
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

const SIDEBAR = `page.getByTestId("modules-analytics-sidebar")`;
const TABS = `${SIDEBAR}.getByRole("tab")`;
const BADGE = `${SIDEBAR}.getByTestId("module-progress-percent")`;
const NOTICE_ACTIVE = `${SIDEBAR}.getByText("This module isn't active yet.",{"exact":true})`;
const NOTICE_INVALID = `${SIDEBAR}.getByText("Invalid date. Please enter valid date.",{"exact":true})`;
const CHART = `${SIDEBAR}.getByTestId("module-progress-chart")`;
const MEASURE = `${SIDEBAR}.getByTestId("module-progress-measure")`;
const OPTIONS = `page.getByRole("option")`;
const ENTRIES = `${SIDEBAR}.getByTestId("module-progress-entries")`;
const ENTRY_ROWS = `${ENTRIES}.locator("[data-testid^=\\"module-progress-entry-\\"]")`;
const ACTIVE_TAB = `${SIDEBAR}.locator("[role='tab'][aria-selected='true']").first()`;

interface ScriptedResponse {
  request: () => { method: () => string };
  url: () => string;
}

function scriptedResponse(method: string, url: string): ScriptedResponse {
  return { request: () => ({ method: () => method }), url: () => url };
}

const MODULE_GET = "https://api.test/api/workspaces/acme/projects/p/modules/123e4567-e89b-12d3-a456-426614174002/";

class Recorder {
  readonly calls: Call[] = [];
  readonly presses: string[] = [];
  readonly waits: number[] = [];
  readonly onCalls: { event: string; fn: unknown }[] = [];
  readonly offCalls: { event: string; fn: unknown }[] = [];
  readonly listeners: { event: string; fn: (response: ScriptedResponse) => void }[] = [];
  rowTexts: string[] = [];
  optionTexts: string[] = [];
  responses: ScriptedResponse[] = [];
  readonly counts = new Map<string, number>();
  readonly texts = new Map<string, string>();
  readonly visibles = new Map<string, boolean>();
  readonly attributes = new Map<string, string | null>();
  readonly waitForThrows = new Set<string>();

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
    if (method === "allInnerTexts") return path === OPTIONS ? this.optionTexts : [];
    if (method === "isVisible") return this.visibles.get(path) ?? true;
    if (method === "getAttribute") return this.attributes.has(path) ? (this.attributes.get(path) ?? null) : "true";
    if (method === "waitFor") {
      if (this.waitForThrows.has(path)) throw new Error(`still hidden: ${path}`);
      return undefined;
    }
    if (method === "click" || method === "selectOption" || method === "scrollIntoViewIfNeeded") {
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
    node.on = (event: string, fn: (response: ScriptedResponse) => void) => {
      this.onCalls.push({ event, fn });
      this.listeners.push({ event, fn });
    };
    node.off = (event: string, fn: (response: ScriptedResponse) => void) => {
      this.offCalls.push({ event, fn });
      const index = this.listeners.findIndex((entry) => entry.event === event && entry.fn === fn);
      if (index !== -1) this.listeners.splice(index, 1);
    };
    node.waitForTimeout = async (ms: number) => {
      this.waits.push(ms);
      for (const response of this.responses) {
        for (const { event, fn } of [...this.listeners]) {
          if (event === "response") fn(response);
        }
      }
    };
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }
}

describe("modulesDetailProgressOpen", () => {
  it("reads open from the visible tabs", async () => {
    const script = new Recorder();
    expect(await script.driver().modulesDetailProgressOpen()).toBe(true);
    expect(script.callsFor(TABS, "count")).toHaveLength(1);
  });

  it("reads closed when tabs are absent or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(TABS, 0);
    expect(await gone.driver().modulesDetailProgressOpen()).toBe(false);
    const hidden = new Recorder();
    hidden.visibles.set(`${TABS}.first()`, false);
    expect(await hidden.driver().modulesDetailProgressOpen()).toBe(false);
  });
});

describe("modulesDetailProgressHeaderPercent", () => {
  it("parses the badge digits", async () => {
    const script = new Recorder();
    script.texts.set(`${BADGE}.first()`, "50%");
    expect(await script.driver().modulesDetailProgressHeaderPercent()).toBe(50);
  });

  it("reads null when the badge is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(BADGE, 0);
    expect(await gone.driver().modulesDetailProgressHeaderPercent()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(`${BADGE}.first()`, false);
    expect(await hidden.driver().modulesDetailProgressHeaderPercent()).toBeNull();
  });

  it("reads zero for an unreadable badge", async () => {
    const script = new Recorder();
    script.texts.set(`${BADGE}.first()`, "—");
    expect(await script.driver().modulesDetailProgressHeaderPercent()).toBe(0);
  });
});

describe("modulesDetailProgressNotice", () => {
  it("returns the visible pinned notice", async () => {
    const inactive = new Recorder();
    inactive.counts.set(NOTICE_INVALID, 0);
    expect(await inactive.driver().modulesDetailProgressNotice()).toBe("This module isn't active yet.");
    const invalid = new Recorder();
    invalid.counts.set(NOTICE_ACTIVE, 0);
    expect(await invalid.driver().modulesDetailProgressNotice()).toBe("Invalid date. Please enter valid date.");
  });

  it("skips hidden notices and reads null when neither shows", async () => {
    const script = new Recorder();
    script.visibles.set(`${NOTICE_ACTIVE}.first()`, false);
    expect(await script.driver().modulesDetailProgressNotice()).toBe("Invalid date. Please enter valid date.");
    const none = new Recorder();
    none.counts.set(NOTICE_ACTIVE, 0);
    none.counts.set(NOTICE_INVALID, 0);
    expect(await none.driver().modulesDetailProgressNotice()).toBeNull();
  });
});

describe("modulesDetailProgressChartVisible", () => {
  it("reads the chart container", async () => {
    expect(await new Recorder().driver().modulesDetailProgressChartVisible()).toBe(true);
    const gone = new Recorder();
    gone.counts.set(CHART, 0);
    expect(await gone.driver().modulesDetailProgressChartVisible()).toBe(false);
    const hidden = new Recorder();
    hidden.visibles.set(CHART, false);
    expect(await hidden.driver().modulesDetailProgressChartVisible()).toBe(false);
  });
});

describe("modulesDetailProgressMeasureValue", () => {
  it("reads the trigger label", async () => {
    const script = new Recorder();
    script.texts.set(MEASURE, "  Work   items\n");
    expect(await script.driver().modulesDetailProgressMeasureValue()).toBe("Work items");
  });

  it("reads null when the trigger is gone or hidden", async () => {
    const gone = new Recorder();
    gone.counts.set(MEASURE, 0);
    expect(await gone.driver().modulesDetailProgressMeasureValue()).toBeNull();
    const hidden = new Recorder();
    hidden.visibles.set(MEASURE, false);
    expect(await hidden.driver().modulesDetailProgressMeasureValue()).toBeNull();
  });
});

describe("modulesDetailProgressMeasureOptions", () => {
  it("opens the trigger, reads the options and escapes", async () => {
    const script = new Recorder();
    script.optionTexts = ["Work items", "Points", " "];
    expect(await script.driver().modulesDetailProgressMeasureOptions()).toEqual(["Work items", "Points"]);
    expect(script.callsFor(`${MEASURE}.first()`, "click")).toHaveLength(1);
    expect(script.callsFor(`${OPTIONS}.first()`, "waitFor")).toHaveLength(1);
    expect(script.presses).toEqual(["Escape"]);
  });

  it("reads empty when no trigger mounts", async () => {
    const script = new Recorder();
    script.counts.set(MEASURE, 0);
    expect(await script.driver().modulesDetailProgressMeasureOptions()).toEqual([]);
    expect(script.callsFor(`${MEASURE}.first()`, "click")).toHaveLength(0);
  });
});

describe("modulesDetailMeasureSwitchGetCount", () => {
  it("switches the measure and counts only module-detail GETs", async () => {
    const script = new Recorder();
    script.texts.set(MEASURE, "Points");
    script.responses = [
      scriptedResponse("GET", MODULE_GET),
      scriptedResponse("GET", MODULE_GET),
      scriptedResponse("POST", MODULE_GET),
      scriptedResponse("GET", "https://api.test/api/workspaces/acme/projects/p/modules/"),
      scriptedResponse("GET", ":::not a url:::"),
    ];
    expect(await script.driver().modulesDetailMeasureSwitchGetCount("Points")).toBe(2);
    expect(script.callsFor(`${MEASURE}.first()`, "click")).toHaveLength(1);
    const options = script.calls.filter((call) => call.path === "page" && call.method === "getByRole");
    expect(options).toHaveLength(1);
    expect(options[0]?.args).toEqual(["option", { name: "Points" }]);
    expect(script.waits).toEqual([3_000]);
    expect(script.onCalls).toHaveLength(1);
    expect(script.offCalls).toHaveLength(1);
    expect(script.offCalls[0]).toEqual(script.onCalls[0]);
    expect(script.listeners).toHaveLength(0);
  });

  it("recovery-clicks when the option never appears", async () => {
    const script = new Recorder();
    script.texts.set(MEASURE, "Points");
    script.waitForThrows.add(`page.getByRole("option",{"name":"Points"}).first()`);
    expect(await script.driver().modulesDetailMeasureSwitchGetCount("Points")).toBe(0);
    expect(script.callsFor(`${MEASURE}.first()`, "click")).toHaveLength(2);
    expect(script.offCalls).toHaveLength(1);
  });
});

describe("modulesDetailProgressStatsTab", () => {
  it("clicks the tab and waits for selection", async () => {
    const script = new Recorder();
    await script.driver().modulesDetailProgressStatsTab("Labels");
    const path = `${SIDEBAR}.getByRole("tab",{"name":"Labels"})`;
    expect(script.callsFor(path, "scrollIntoViewIfNeeded")).toHaveLength(1);
    expect(script.callsFor(path, "click")).toHaveLength(1);
    expect(script.callsFor(path, "getAttribute")).toHaveLength(1);
  });
});

describe("modulesDetailActiveStatsTab", () => {
  it("reads the selected tab title", async () => {
    const script = new Recorder();
    script.texts.set(ACTIVE_TAB, "  Labels\n");
    expect(await script.driver().modulesDetailActiveStatsTab()).toBe("Labels");
    expect(script.callsFor(ACTIVE_TAB, "waitFor")).toHaveLength(1);
  });
});

describe("modulesDetailProgressStatsRows", () => {
  it("parses every shaped row and skips the rest", async () => {
    const script = new Recorder();
    script.rowTexts = ["Backlog 33% of 3", "No assignee 50% of 2", "100% coverage 25% of 4", "garbage", "   "];
    expect(await script.driver().modulesDetailProgressStatsRows()).toEqual([
      { title: "Backlog", percent: 33, total: 3 },
      { title: "No assignee", percent: 50, total: 2 },
      { title: "100% coverage", percent: 25, total: 4 },
    ]);
  });

  it("tolerates newline-joined row text and reads empty with no rows", async () => {
    const script = new Recorder();
    script.rowTexts = ["Backlog\n33%\nof 3"];
    expect(await script.driver().modulesDetailProgressStatsRows()).toEqual([
      { title: "Backlog", percent: 33, total: 3 },
    ]);
    expect(await new Recorder().driver().modulesDetailProgressStatsRows()).toEqual([]);
  });
});

describe("modulesDetailProgressEntryClick", () => {
  it("clicks the exactly-titled row", async () => {
    const script = new Recorder();
    script.rowTexts = ["Bug 0% of 1", "Buggy 0% of 1"];
    await script.driver().modulesDetailProgressEntryClick("Bug");
    expect(script.callsFor(`${ENTRY_ROWS}.nth(0)`, "scrollIntoViewIfNeeded")).toHaveLength(1);
    expect(script.callsFor(`${ENTRY_ROWS}.nth(0)`, "click")).toHaveLength(1);
    expect(script.callsFor(`${ENTRY_ROWS}.nth(1)`, "click")).toHaveLength(0);
  });

  it("refuses an unknown title", async () => {
    const script = new Recorder();
    script.rowTexts = ["Bug 0% of 1"];
    await expect(script.driver().modulesDetailProgressEntryClick("Missing")).rejects.toThrow(
      "[parity] unknown module progress entry: Missing"
    );
  });
});

describe("modulesDetailProgressEntryClickable", () => {
  it("reads buttons clickable and divs inert", async () => {
    const script = new Recorder();
    script.rowTexts = ["Ada Lovelace 0% of 1", "No assignee 50% of 2"];
    script.attributes.set(`${ENTRY_ROWS}.nth(0)`, "false");
    script.attributes.set(`${ENTRY_ROWS}.nth(1)`, null);
    expect(await script.driver().modulesDetailProgressEntryClickable("Ada Lovelace")).toBe(true);
    expect(await script.driver().modulesDetailProgressEntryClickable("No assignee")).toBe(false);
  });

  it("reads unknown titles unclickable", async () => {
    const script = new Recorder();
    script.rowTexts = ["Bug 0% of 1"];
    expect(await script.driver().modulesDetailProgressEntryClickable("Missing")).toBe(false);
  });
});

describe("modulesDetailProgressEntrySelected", () => {
  it("reads the pressed marker", async () => {
    const script = new Recorder();
    script.rowTexts = ["Backlog 33% of 3", "Started 0% of 3", "No assignee 50% of 2"];
    script.attributes.set(`${ENTRY_ROWS}.nth(0)`, "true");
    script.attributes.set(`${ENTRY_ROWS}.nth(1)`, "false");
    script.attributes.set(`${ENTRY_ROWS}.nth(2)`, null);
    expect(await script.driver().modulesDetailProgressEntrySelected("Backlog")).toBe(true);
    expect(await script.driver().modulesDetailProgressEntrySelected("Started")).toBe(false);
    expect(await script.driver().modulesDetailProgressEntrySelected("No assignee")).toBe(false);
  });

  it("reads unknown titles unselected", async () => {
    const script = new Recorder();
    script.rowTexts = ["Bug 0% of 1"];
    expect(await script.driver().modulesDetailProgressEntrySelected("Missing")).toBe(false);
  });
});

describe("modulesXAnalyticsSidebarVisible", () => {
  it("reads the sidebar presence", async () => {
    expect(await new Recorder().driver().modulesXAnalyticsSidebarVisible()).toBe(true);
    const gone = new Recorder();
    gone.counts.set(SIDEBAR, 0);
    expect(await gone.driver().modulesXAnalyticsSidebarVisible()).toBe(false);
    const hidden = new Recorder();
    hidden.visibles.set(SIDEBAR, false);
    expect(await hidden.driver().modulesXAnalyticsSidebarVisible()).toBe(false);
  });
});
