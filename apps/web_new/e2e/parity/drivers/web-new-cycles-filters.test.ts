// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycles filter methods (NEWFRONT-709,
// CYC-006..008). A recording fake stands in for Playwright: each
// locator remembers the path of calls that built it, scripted answers
// stand in for the page, and a test asserts both which selector the
// driver used and what it does with the answer. Never about a real
// browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "last", "nth", "getByRole", "locator", "getByTestId", "or"];
const ACTIONS = ["count", "click", "innerText", "getAttribute", "isVisible", "waitFor"];

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

const TRIGGER_PATH = `page.getByTestId("cycles-filters-trigger")`;
const PANEL_PATH = `page.getByTestId("cycles-filters-panel")`;
const ROW_PATH = `page.getByTestId("cycles-filter-chips")`;
const BADGE_PATH = `page.getByTestId("cycles-filters-count")`;
const SECTIONS_PATH = `${PANEL_PATH}.locator("[data-section-title]")`;
const OPTIONS_PATH = `${PANEL_PATH}.locator("button[role=\\"checkbox\\"],button[role=\\"radio\\"]")`;
const GROUPS_PATH = `${ROW_PATH}.locator("[data-testid^=\\"cycles-filter-chip-\\"]")`;

function pickBlockPath(section: string): string {
  return `${PANEL_PATH}.locator("[data-section-title*=\\"${section}\\"]")`;
}

function nthIndexOf(path: string): number {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match?.[1] === undefined ? 0 : Number(match[1]);
}

class Recorder {
  readonly calls: Call[] = [];
  readonly pressedKeys: string[] = [];
  panelAttached = true;
  panelAttachThrows = false;
  rowAttached = true;
  rowVisible = true;
  badgePresent = false;
  sections: (string | null)[] = [];
  options: (string | null)[] = [];
  chips: string[] = [];

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
    if (method === "innerText") return this.chips[nthIndexOf(path)] ?? "";
    if (method === "getAttribute") return this.attribute(path, args[0]);
    if (method === "isVisible") return path === ROW_PATH ? this.rowVisible : true;
    if (method === "waitFor") {
      const state = (args[0] as { state?: string } | undefined)?.state;
      if (path === PANEL_PATH && state === "attached" && this.panelAttachThrows) {
        throw new Error("panel never attached");
      }
      return undefined;
    }
    return undefined;
  }

  private count(path: string): number {
    if (path === PANEL_PATH) return this.panelAttached ? 1 : 0;
    if (path === ROW_PATH) return this.rowAttached ? 1 : 0;
    if (path === BADGE_PATH) return this.badgePresent ? 1 : 0;
    if (path === SECTIONS_PATH) return this.sections.length;
    if (path === OPTIONS_PATH) return this.options.length;
    if (path === GROUPS_PATH) return this.chips.length;
    return 0;
  }

  private attribute(path: string, name: unknown): string | null {
    if (path.startsWith(`${SECTIONS_PATH}.nth(`) && name === "data-section-title") {
      return this.sections[nthIndexOf(path)] ?? null;
    }
    if (path.startsWith(`${OPTIONS_PATH}.nth(`) && name === "aria-label") {
      return this.options[nthIndexOf(path)] ?? null;
    }
    return null;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "locator", "getByTestId"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.keyboard = {
      press: async (key: string) => {
        this.pressedKeys.push(key);
      },
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

describe("cyclesFiltersOpen", () => {
  it("clicks the trigger and waits for the visible panel", async () => {
    const script = new Recorder();
    await script.driver().cyclesFiltersOpen();
    expect(script.callsFor(TRIGGER_PATH, "click")).toHaveLength(1);
    const waits = script.callsFor(PANEL_PATH, "waitFor");
    expect(waits).toHaveLength(1);
    expect(waits[0]?.args[0]).toMatchObject({ state: "visible" });
  });
});

describe("cyclesFiltersClose", () => {
  it("escapes an attached panel and waits for it to detach", async () => {
    const script = new Recorder();
    await script.driver().cyclesFiltersClose();
    expect(script.pressedKeys).toEqual(["Escape"]);
    const waits = script.callsFor(PANEL_PATH, "waitFor");
    expect(waits.map((call) => (call.args[0] as { state?: string }).state)).toEqual(["attached", "detached"]);
  });

  it("skips the escape when the panel never attached", async () => {
    const script = new Recorder();
    script.panelAttachThrows = true;
    script.panelAttached = false;
    await script.driver().cyclesFiltersClose();
    expect(script.pressedKeys).toEqual([]);
  });

  it("skips the escape when the panel already detached", async () => {
    const script = new Recorder();
    script.panelAttached = false;
    await script.driver().cyclesFiltersClose();
    expect(script.pressedKeys).toEqual([]);
  });
});

describe("cyclesFilterSections", () => {
  it("reads section titles in display order and skips blanks", async () => {
    const script = new Recorder();
    script.sections = ["Status of the cycle", "", "Start date", null, "Due date"];
    expect(await script.driver().cyclesFilterSections()).toEqual(["Status of the cycle", "Start date", "Due date"]);
    expect(script.callsFor(SECTIONS_PATH, "count")).toHaveLength(1);
  });
});

describe("cyclesFilterOptionNames", () => {
  it("reads option labels in display order and skips blanks", async () => {
    const script = new Recorder();
    script.options = ["Yet to start", "Draft", "", null, "Custom", "After"];
    expect(await script.driver().cyclesFilterOptionNames()).toEqual(["Yet to start", "Draft", "Custom", "After"]);
    expect(script.callsFor(OPTIONS_PATH, "count")).toHaveLength(1);
  });
});

describe("cyclesFilterPick", () => {
  it("clicks the named option inside the title-matched section", async () => {
    const script = new Recorder();
    await script.driver().cyclesFilterPick("Status", "Yet to start");
    const block = pickBlockPath("Status");
    const picks = script.calls.filter((call) => call.path === block && call.method === "getByRole");
    expect(picks.map((call) => call.args)).toEqual([
      ["checkbox", { name: "Yet to start" }],
      ["radio", { name: "Yet to start" }],
    ]);
    const checked = `${block}.getByRole("checkbox",{"name":"Yet to start"})`;
    // The checkbox attempt is widened with the radio fallback, and the
    // click lands on that combined locator rather than either alone.
    expect(script.calls.filter((call) => call.path === checked && call.method === "or")).toHaveLength(1);
    expect(script.callsFor(`${checked}.or({})`, "click")).toHaveLength(1);
  });
});

describe("cyclesFilterChipTexts", () => {
  it("reads nothing without a chip row", async () => {
    const script = new Recorder();
    script.rowAttached = false;
    expect(await script.driver().cyclesFilterChipTexts()).toEqual([]);
  });

  it("reads nothing while the row stays hidden", async () => {
    const script = new Recorder();
    script.rowVisible = false;
    expect(await script.driver().cyclesFilterChipTexts()).toEqual([]);
  });

  it("normalizes group texts and skips blanks", async () => {
    const script = new Recorder();
    script.chips = ["Status  Yet to start\nDraft", "   ", "Start date 1 week from now"];
    expect(await script.driver().cyclesFilterChipTexts()).toEqual([
      "Status Yet to start Draft",
      "Start date 1 week from now",
    ]);
  });
});

describe("cyclesFilterRemoveChip", () => {
  it("clicks the last button of the matching group", async () => {
    const script = new Recorder();
    script.chips = ["Status Yet to start", "Start date 1 week from now"];
    await script.driver().cyclesFilterRemoveChip("Status Yet to start");
    const group = `${GROUPS_PATH}.nth(0)`;
    expect(script.callsFor(`${group}.getByRole("button").last()`, "click")).toHaveLength(1);
  });

  it("throws when no group carries the text", async () => {
    const script = new Recorder();
    script.chips = ["Start date 1 week from now"];
    await expect(script.driver().cyclesFilterRemoveChip("Status Yet to start")).rejects.toThrow(
      "[parity] live-cycles chip not found: Status Yet to start"
    );
  });
});

describe("cyclesFiltersClearAll", () => {
  it("clicks the clear-all button in the chip row", async () => {
    const script = new Recorder();
    await script.driver().cyclesFiltersClearAll();
    const clears = script.calls.filter((call) => call.path === ROW_PATH && call.method === "getByRole");
    expect(clears).toHaveLength(1);
    // A name option holding a regex collapses in the rendered path, so
    // the expression itself is asserted from the recorded chain call.
    expect(String((clears[0]?.args[1] as { name?: unknown })?.name)).toBe("/clear all/i");
    const clicks = script.calls.filter((call) => call.method === "click");
    expect(clicks).toHaveLength(1);
    expect(clicks[0]?.path.startsWith(`${ROW_PATH}.getByRole(`)).toBe(true);
  });
});

describe("cyclesFiltersActive", () => {
  it("reads the trigger badge", async () => {
    const script = new Recorder();
    script.badgePresent = false;
    expect(await script.driver().cyclesFiltersActive()).toBe(false);
    script.badgePresent = true;
    expect(await script.driver().cyclesFiltersActive()).toBe(true);
    expect(script.callsFor(BADGE_PATH, "count")).toHaveLength(2);
  });
});
