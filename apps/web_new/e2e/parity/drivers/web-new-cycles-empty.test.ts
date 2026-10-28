// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycles empty-state methods (NEWFRONT-716
// gate, CYC-009–CYC-016): the list opener and visible names, the skeleton
// reader and its slow-fetch transition, the no-match heading and hint, the
// search and filter delegation, the feature-off view and its admin-gated
// shortcut, the zero-state and its member-only shortcut, the gone-away
// view with its way back, and the create submit's stale-toast guard. A
// recording fake stands in for Playwright: each
// locator remembers the path of calls that built it, scripted answers
// stand in for the page, and a test asserts both which selector the
// driver used and what it does with the answer. Never about a real browser.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "locator", "filter"];
const ACTIONS = ["count", "click", "fill", "innerText", "getAttribute", "waitFor", "isVisible", "isDisabled"];

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

const ROWS_PATH = `page.locator("div[data-testid=\\"cycle-row\\"]")`;
const SCREEN_PATH = `page.getByTestId("cycles-list-screen")`;
const SEARCH_BOX_PATH = `page.getByTestId("cycles-search-box")`;
const SEARCH_TOGGLE_PATH = `page.getByTestId("cycles-search-toggle")`;
const SEARCH_INPUT_PATH = `page.getByRole("textbox",{"name":"Search cycles"})`;
const SEARCH_CLOSE_PATH = `page.getByRole("button",{"name":"Close search"})`;
const FILTER_TRIGGER_PATH = `page.getByTestId("cycles-filters-trigger")`;
const FILTER_PANEL_PATH = `page.getByTestId("cycles-filters-panel")`;
const FILTER_SECTIONS_PATH = `${FILTER_PANEL_PATH}.locator("[data-section-title]")`;
const FILTER_OPTIONS_PATH = `${FILTER_PANEL_PATH}.locator("button[role=\\"checkbox\\"],button[role=\\"radio\\"]")`;
const CHIP_ROW_PATH = `page.getByTestId("cycles-filter-chips")`;
const SKELETON_PATH = `page.getByTestId("cycles-list-skeleton")`;
const NOMATCH_PATH = `page.getByTestId("cycles-no-match")`;
const NOMATCH_H5_PATH = `${NOMATCH_PATH}.locator("h5").first()`;
const NOMATCH_P_PATH = `${NOMATCH_PATH}.locator("p").first()`;
const GATE_PATH = `page.getByTestId("cycles-feature-gate")`;
const MANAGE_PATH = `page.getByRole("button",{"name":"Manage features"}).first()`;
const ZERO_PATH = `page.getByTestId("cycles-zero-state")`;
const FIRST_CYCLE_PATH = `page.getByRole("button",{"name":"Set your first cycle"}).first()`;
const GONE_PATH = `page.getByTestId("cycles-gone-away")`;
const BACK_PATH = `page.getByRole("button",{"name":"View other cycles"}).first()`;
const TOAST_PATH = `page.locator("[data-testid=\\"toast-notification\\"]")`;
// Dialog-scoped paths use substring matching: the dialog root renders
// through .filter({has}) whose recording is too brittle to pin exactly.
const CREATE_INPUT_SUFFIX = `.locator("input[name=\\"name\\"]")`;
const CREATE_ALERT_SUFFIX = `.getByRole("alert")`;
const CREATE_BUTTON_SUBSTRING = `getByRole("button",{"name":"Create cycle"})`;
const DISMISS_SUBSTRING = `{"name":"Dismiss notification"}`;

function nthIndex(path: string): number | null {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match === null ? null : Number(match[1]);
}

interface ScriptedRoute {
  predicate: (url: URL) => boolean;
  handler: (route: Route) => Promise<void>;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly gotoUrls: string[] = [];
  readonly waitedUrls: unknown[] = [];
  readonly keyboardPresses: string[] = [];
  readonly fills: { path: string; value: string }[] = [];
  readonly routes: ScriptedRoute[] = [];
  unrouteAllCalls = 0;
  unrouteAllThrows = false;
  gotoThrowsLeft = 0;
  rowCount = 0;
  rowTexts: string[] = [];
  searchBoxCount = 0;
  panelCount = 1;
  sectionTitles: (string | null)[] = [];
  optionLabels: (string | null)[] = [];
  skeletonReads: boolean[] = [];
  noMatchCount = 0;
  noMatchVisible = true;
  noMatchHeadingCount = 0;
  noMatchHeadingVisible = true;
  noMatchHeadingText = "";
  noMatchHintCount = 0;
  noMatchHintText = "";
  gateCount = 0;
  gateVisible = true;
  manageDisabled = false;
  zeroCount = 0;
  zeroVisible = true;
  firstCycleDisabled = false;
  goneCount = 0;
  goneVisible = true;
  toastCount = 0;
  toastTexts: string[] = [];
  dialogAlertCount = 0;

  /** Dismiss clicks recorded so far: dismissed cards read as gone. */
  private dismissClicks(): number {
    return this.calls.filter(
      (call) => call.method === "click" && call.path.startsWith(TOAST_PATH) && call.path.includes(DISMISS_SUBSTRING)
    ).length;
  }

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
    const { path, method, args } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText") return this.text(path);
    if (method === "isVisible") return this.visible(path);
    if (method === "isDisabled") return this.disabled(path);
    if (method === "getAttribute") return this.attribute(path);
    if (method === "fill") {
      this.fills.push({ path, value: String(args[0]) });
      return undefined;
    }
    if (method === "waitFor" || method === "click") return undefined;
    throw new Error(`unexpected action ${method} at ${path} with ${JSON.stringify(args)}`);
  }

  private count(path: string): number {
    if (path === ROWS_PATH) return this.rowCount;
    if (path === SEARCH_BOX_PATH) return this.searchBoxCount;
    if (path === FILTER_PANEL_PATH) return this.panelCount;
    if (path === FILTER_SECTIONS_PATH) return this.sectionTitles.length;
    if (path === FILTER_OPTIONS_PATH) return this.optionLabels.length;
    if (path === NOMATCH_PATH) return this.noMatchCount;
    if (path === NOMATCH_H5_PATH) return this.noMatchHeadingCount;
    if (path === NOMATCH_P_PATH) return this.noMatchHintCount;
    if (path === GATE_PATH) return this.gateCount;
    if (path === ZERO_PATH) return this.zeroCount;
    if (path === GONE_PATH) return this.goneCount;
    if (path === TOAST_PATH) return Math.max(0, this.toastCount - this.dismissClicks());
    if (path.endsWith(CREATE_INPUT_SUFFIX)) return 1;
    if (path.endsWith(CREATE_ALERT_SUFFIX)) return this.dialogAlertCount;
    return 1;
  }

  private text(path: string): string {
    const rowIndex = path.startsWith(ROWS_PATH) ? nthIndex(path) : null;
    if (rowIndex !== null) return this.rowTexts[rowIndex] ?? "";
    if (path === NOMATCH_H5_PATH) return this.noMatchHeadingText;
    if (path === NOMATCH_P_PATH) return this.noMatchHintText;
    if (path.startsWith(`${TOAST_PATH}.nth(`)) {
      const toastIndex = nthIndex(path);
      return toastIndex === null ? "" : (this.toastTexts[toastIndex] ?? "");
    }
    return "";
  }

  private visible(path: string): boolean {
    if (path === `${SKELETON_PATH}.first()`) {
      return this.skeletonReads.length > 1 ? (this.skeletonReads.shift() ?? false) : (this.skeletonReads[0] ?? false);
    }
    if (path === `${NOMATCH_PATH}.first()`) return this.noMatchVisible;
    if (path === `${NOMATCH_H5_PATH}.first()`) return this.noMatchHeadingVisible;
    if (path === `${GATE_PATH}.first()`) return this.gateVisible;
    if (path === `${ZERO_PATH}.first()`) return this.zeroVisible;
    if (path === `${GONE_PATH}.first()`) return this.goneVisible;
    if (path.startsWith(`${TOAST_PATH}.nth(`)) {
      const toastIndex = nthIndex(path);
      return toastIndex !== null && toastIndex < this.toastCount - this.dismissClicks();
    }
    return true;
  }

  private disabled(path: string): boolean {
    if (path === MANAGE_PATH) return this.manageDisabled;
    if (path === FIRST_CYCLE_PATH) return this.firstCycleDisabled;
    return false;
  }

  private attribute(path: string): string | null {
    if (path.startsWith(FILTER_SECTIONS_PATH)) {
      const index = nthIndex(path);
      return index === null ? null : (this.sectionTitles[index] ?? null);
    }
    if (path.startsWith(FILTER_OPTIONS_PATH)) {
      const index = nthIndex(path);
      return index === null ? null : (this.optionLabels[index] ?? null);
    }
    return null;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByTestId", "locator"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.keyboard = {
      press: async (key: string) => {
        this.keyboardPresses.push(key);
        return undefined;
      },
    };
    node.goto = async (url: string) => {
      if (this.gotoThrowsLeft > 0) {
        this.gotoThrowsLeft -= 1;
        throw new Error("stalled render");
      }
      this.gotoUrls.push(url);
      return undefined;
    };
    node.waitForLoadState = async () => undefined;
    node.waitForTimeout = async () => undefined;
    node.waitForURL = async (pattern: unknown) => {
      this.waitedUrls.push(pattern);
      return undefined;
    };
    node.route = async (predicate: (url: URL) => boolean, handler: (route: Route) => Promise<void>) => {
      this.routes.push({ predicate, handler });
      return undefined;
    };
    node.unrouteAll = async () => {
      this.unrouteAllCalls += 1;
      if (this.unrouteAllThrows) throw new Error("already handled");
      return undefined;
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

describe("cyclesEmptyOpenList", () => {
  it("opens the live list address", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyOpenList("acme", "pid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles"]);
  });
});

describe("cyclesEmptyVisibleNames", () => {
  it("reads through the shared list reader", async () => {
    const script = new Recorder();
    script.rowCount = 1;
    script.rowTexts = ["Upcoming cycle"];
    expect(await script.driver().cyclesEmptyVisibleNames()).toEqual(["Upcoming cycle"]);
  });
});

describe("cyclesEmptySkeletonVisible", () => {
  it("reads the skeleton's presence", async () => {
    const script = new Recorder();
    script.skeletonReads = [true];
    expect(await script.driver().cyclesEmptySkeletonVisible()).toBe(true);
    script.skeletonReads = [false];
    expect(await script.driver().cyclesEmptySkeletonVisible()).toBe(false);
  });
});

describe("cyclesEmptyNoMatchHeading", () => {
  it("reads the no-match title", async () => {
    const script = new Recorder();
    script.noMatchHeadingCount = 1;
    script.noMatchHeadingText = "  No matching cycles ";
    expect(await script.driver().cyclesEmptyNoMatchHeading()).toBe("No matching cycles");
  });

  it("reads null without a title, while hidden, and on a blank title", async () => {
    const script = new Recorder();
    script.noMatchHeadingCount = 0;
    expect(await script.driver().cyclesEmptyNoMatchHeading()).toBeNull();
    script.noMatchHeadingCount = 1;
    script.noMatchHeadingVisible = false;
    expect(await script.driver().cyclesEmptyNoMatchHeading()).toBeNull();
    script.noMatchHeadingVisible = true;
    script.noMatchHeadingText = "  ";
    expect(await script.driver().cyclesEmptyNoMatchHeading()).toBeNull();
  });
});

describe("cyclesEmptyNoMatchHint", () => {
  it("reads the cause hint", async () => {
    const script = new Recorder();
    script.noMatchCount = 1;
    script.noMatchHintCount = 1;
    script.noMatchHintText = "  Remove the filters to see all cycles ";
    expect(await script.driver().cyclesEmptyNoMatchHint()).toBe("Remove the filters to see all cycles");
  });

  it("reads null without the view, while hidden, without a hint, and on a blank hint", async () => {
    const script = new Recorder();
    script.noMatchCount = 0;
    expect(await script.driver().cyclesEmptyNoMatchHint()).toBeNull();
    script.noMatchCount = 1;
    script.noMatchVisible = false;
    expect(await script.driver().cyclesEmptyNoMatchHint()).toBeNull();
    script.noMatchVisible = true;
    script.noMatchHintCount = 0;
    expect(await script.driver().cyclesEmptyNoMatchHint()).toBeNull();
    script.noMatchHintCount = 1;
    script.noMatchHintText = "   ";
    expect(await script.driver().cyclesEmptyNoMatchHint()).toBeNull();
  });
});

describe("cyclesEmptySearchOpen", () => {
  it("opens through the shared search box", async () => {
    const script = new Recorder();
    script.searchBoxCount = 0;
    await script.driver().cyclesEmptySearchOpen();
    expect(script.callsFor(SEARCH_TOGGLE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptySearchFill", () => {
  it("fills through the shared search box", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptySearchFill("zzz");
    expect(script.fills).toEqual([{ path: SEARCH_INPUT_PATH, value: "zzz" }]);
  });
});

describe("cyclesEmptySearchClear", () => {
  it("clears through the shared search box", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptySearchClear();
    expect(script.callsFor(SEARCH_CLOSE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptyFiltersOpen", () => {
  it("opens through the shared filters menu", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyFiltersOpen();
    expect(script.callsFor(FILTER_TRIGGER_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptyFiltersClose", () => {
  it("closes through the shared filters menu", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyFiltersClose();
    expect(script.keyboardPresses).toEqual(["Escape"]);
  });
});

describe("cyclesEmptyFilterSections", () => {
  it("reads through the shared section reader", async () => {
    const script = new Recorder();
    script.sectionTitles = ["Status of the cycle", null, ""];
    expect(await script.driver().cyclesEmptyFilterSections()).toEqual(["Status of the cycle"]);
  });
});

describe("cyclesEmptyFilterOptionNames", () => {
  it("reads through the shared option reader", async () => {
    const script = new Recorder();
    script.optionLabels = ["Completed", null, ""];
    expect(await script.driver().cyclesEmptyFilterOptionNames()).toEqual(["Completed"]);
  });
});

describe("cyclesEmptyFilterPick", () => {
  it("picks through the shared section picker", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyFilterPick("Status of the cycle", "Completed");
    const block = `${FILTER_PANEL_PATH}.locator("[data-section-title*=\\"Status of the cycle\\"]")`;
    expect(script.callsFor(`${block}.getByRole("checkbox",{"name":"Completed"})`, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptyFiltersClearAll", () => {
  it("clears through the shared clear-all", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyFiltersClearAll();
    const clears = script.calls.filter((call) => call.path === CHIP_ROW_PATH && call.method === "getByRole");
    expect(clears).toHaveLength(1);
  });
});

describe("cyclesEmptySkeletonShownOnSlowFetch", () => {
  it("holds the list fetch, sees the skeleton, and settles", async () => {
    const script = new Recorder();
    script.skeletonReads = [true, false];
    expect(await script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid")).toBe(true);
    expect(script.routes).toHaveLength(1);
    const predicate = script.routes[0]?.predicate ?? (() => false);
    expect(predicate(new URL("http://localhost:3116/api/workspaces/acme/projects/pid/cycles/"))).toBe(true);
    expect(predicate(new URL("http://localhost:3116/api/workspaces/acme/projects/pid/cycles/cid/"))).toBe(false);
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles"]);
    expect(script.unrouteAllCalls).toBe(1);
  });

  it("retries a stalled first pass", async () => {
    const script = new Recorder();
    script.gotoThrowsLeft = 1;
    script.skeletonReads = [false, true, false];
    expect(await script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid")).toBe(true);
    expect(script.routes).toHaveLength(2);
    expect(script.gotoUrls).toHaveLength(1);
    expect(script.unrouteAllCalls).toBe(2);
  });

  it("reports a missed skeleton and an unsettled list on tiny budgets", async () => {
    const script = new Recorder();
    const driver = script.driver() as unknown as {
      cyclesSkeletonSeen: (timeoutMs: number) => Promise<boolean>;
      cyclesSkeletonSettled: (timeoutMs: number) => Promise<void>;
    };
    script.skeletonReads = [false];
    await expect(driver.cyclesSkeletonSeen(30)).resolves.toBe(false);
    script.skeletonReads = [true];
    await expect(driver.cyclesSkeletonSettled(30)).rejects.toThrow("never settled");
  });

  it("throws after two stalled rounds", async () => {
    const script = new Recorder();
    script.gotoThrowsLeft = 5;
    await expect(script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid")).rejects.toThrow("stalled render");
    expect(script.unrouteAllCalls).toBe(2);
  });

  it("swallows an already-handled unroute on both paths", async () => {
    const script = new Recorder();
    script.unrouteAllThrows = true;
    script.skeletonReads = [true, false];
    expect(await script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid")).toBe(true);
    script.gotoThrowsLeft = 5;
    await expect(script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid")).rejects.toThrow("stalled render");
    expect(script.unrouteAllCalls).toBe(3);
  });

  it("holds two fetches, then lets the rest through, swallowing handled continuations", async () => {
    const script = new Recorder();
    script.skeletonReads = [true, false];
    await script.driver().cyclesEmptySkeletonShownOnSlowFetch("acme", "pid");
    const handler = script.routes[0]?.handler;
    if (handler === undefined) throw new Error("expected a recorded route handler");
    let continued = 0;
    let rejectNext = false;
    const route = {
      continue: async () => {
        continued += 1;
        if (rejectNext) throw new Error("already handled");
      },
    } as unknown as Route;
    vi.useFakeTimers();
    try {
      const first = handler(route);
      await vi.advanceTimersByTimeAsync(5000);
      await first;
      const second = handler(route);
      await vi.advanceTimersByTimeAsync(5000);
      await second;
      rejectNext = true;
      await handler(route);
    } finally {
      vi.useRealTimers();
    }
    expect(continued).toBe(3);
  });
});

describe("cyclesEmptyFeatureOffVisible", () => {
  it("reads the gate's presence", async () => {
    const script = new Recorder();
    script.gateCount = 1;
    script.skeletonReads = [true, false];
    expect(await script.driver().cyclesEmptyFeatureOffVisible()).toBe(true);
    expect(script.callsFor(SCREEN_PATH, "waitFor")).toHaveLength(1);
    script.gateCount = 0;
    expect(await script.driver().cyclesEmptyFeatureOffVisible()).toBe(false);
    script.gateCount = 1;
    script.gateVisible = false;
    expect(await script.driver().cyclesEmptyFeatureOffVisible()).toBe(false);
  });
});

describe("cyclesEmptyFeatureOffActionDisabled", () => {
  it("reads the shortcut's disabled state", async () => {
    const script = new Recorder();
    script.manageDisabled = true;
    expect(await script.driver().cyclesEmptyFeatureOffActionDisabled()).toBe(true);
    script.manageDisabled = false;
    expect(await script.driver().cyclesEmptyFeatureOffActionDisabled()).toBe(false);
  });
});

describe("cyclesEmptyFeatureOffActionOpen", () => {
  it("clicks the management shortcut", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyFeatureOffActionOpen();
    expect(script.callsFor(MANAGE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptyZeroStateVisible", () => {
  it("reads the zero-state's presence", async () => {
    const script = new Recorder();
    script.zeroCount = 1;
    script.skeletonReads = [true, false];
    expect(await script.driver().cyclesEmptyZeroStateVisible()).toBe(true);
    expect(script.callsFor(SCREEN_PATH, "waitFor")).toHaveLength(1);
    script.zeroCount = 0;
    expect(await script.driver().cyclesEmptyZeroStateVisible()).toBe(false);
    script.zeroCount = 1;
    script.zeroVisible = false;
    expect(await script.driver().cyclesEmptyZeroStateVisible()).toBe(false);
  });
});

describe("cyclesEmptyZeroStateCreateDisabled", () => {
  it("reads the shortcut's disabled state", async () => {
    const script = new Recorder();
    script.firstCycleDisabled = true;
    expect(await script.driver().cyclesEmptyZeroStateCreateDisabled()).toBe(true);
    script.firstCycleDisabled = false;
    expect(await script.driver().cyclesEmptyZeroStateCreateDisabled()).toBe(false);
  });
});

describe("cyclesEmptyZeroStateCreateOpen", () => {
  it("clicks the shortcut and waits for the dialog field", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyZeroStateCreateOpen();
    expect(script.callsFor(FIRST_CYCLE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesEmptyOpenDetail", () => {
  it("opens the detail address and waits for the gone-away action", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyOpenDetail("acme", "pid", "cid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles/cid"]);
    expect(script.callsFor(BACK_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesEmptyGoneAwayVisible", () => {
  it("reads the gone-away view's presence", async () => {
    const script = new Recorder();
    script.goneCount = 1;
    expect(await script.driver().cyclesEmptyGoneAwayVisible()).toBe(true);
    script.goneCount = 0;
    expect(await script.driver().cyclesEmptyGoneAwayVisible()).toBe(false);
    script.goneCount = 1;
    script.goneVisible = false;
    expect(await script.driver().cyclesEmptyGoneAwayVisible()).toBe(false);
  });
});

describe("cyclesEmptyGoneAwayBack", () => {
  it("clicks back and waits for the list address", async () => {
    const script = new Recorder();
    await script.driver().cyclesEmptyGoneAwayBack();
    expect(script.callsFor(BACK_PATH, "click")).toHaveLength(1);
    expect(script.waitedUrls).toHaveLength(1);
    const pattern = script.waitedUrls[0] as RegExp;
    expect(pattern.test("/acme/projects/pid/cycles/")).toBe(true);
    expect(pattern.test("/acme/projects/pid/cycles/cid")).toBe(false);
  });
});

describe("cyclesCreateSubmit", () => {
  function clickIndex(script: Recorder, substring: string): number {
    return script.calls.findIndex((call) => call.method === "click" && call.path.includes(substring));
  }

  it("dismisses a stale toast before submitting so the settle sees only the new outcome", async () => {
    const script = new Recorder();
    script.toastCount = 1;
    script.toastTexts = ["Error! These dates overlap another cycle."];
    script.dialogAlertCount = 1;
    await script.driver().cyclesCreateSubmit();
    const dismissAt = clickIndex(script, DISMISS_SUBSTRING);
    const createAt = clickIndex(script, CREATE_BUTTON_SUBSTRING);
    expect(dismissAt).toBeGreaterThanOrEqual(0);
    expect(createAt).toBeGreaterThan(dismissAt);
  });

  it("submits directly when no toast is visible", async () => {
    const script = new Recorder();
    script.dialogAlertCount = 1;
    await script.driver().cyclesCreateSubmit();
    expect(
      script.calls.filter((call) => call.method === "click" && call.path.includes(DISMISS_SUBSTRING))
    ).toHaveLength(0);
    expect(
      script.calls.filter((call) => call.method === "click" && call.path.includes(CREATE_BUTTON_SUBSTRING))
    ).toHaveLength(1);
  });
});
