// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycle detail, favorite, transfer and
// hero-name methods (NEWFRONT-716 gate, CYC-019, CYC-029-031, CYC-046):
// the detail opener, the exact-text issue probe, the transfer entry,
// picker, search and empty-state readers, the row star state and drive,
// and the hero cycle name. A recording fake stands in for Playwright:
// each locator remembers the path of calls that built it, scripted
// answers stand in for the page, and a test asserts both which selector
// the driver used and what it does with the answer. The converge and
// settle drives replay scripted row/server timelines instead, so the
// swallowed clicks, stale reads and pending servers they guard against
// replay deterministically without any clock. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "getByText", "getByPlaceholder", "locator", "filter"];

function nthIndex(path: string): number | null {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match === null ? null : Number(match[1]);
}
const ACTIONS = [
  "count",
  "click",
  "fill",
  "innerText",
  "allTextContents",
  "getAttribute",
  "hover",
  "waitFor",
  "isVisible",
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

const ROWS_PATH = `page.locator("div[data-testid=\\"cycle-row\\"]")`;
const TRANSFER_BUTTON_PATH = `page.getByRole("button",${JSON.stringify({ name: "Transfer work items", exact: true })})`;
const TRANSFER_SEARCH_PATH = `page.getByPlaceholder("Search for a cycle...")`;
const TRANSFER_LISTBOX_PATH = `page.getByRole("listbox",${JSON.stringify({ name: "Cycles" })})`;
const TRANSFER_OPTIONS_PATH = `${TRANSFER_LISTBOX_PATH}.getByRole("option")`;
const TRANSFER_NOTE_PATH = `${TRANSFER_LISTBOX_PATH}.locator("p").first()`;
const HERO_PATH = `page.getByTestId("cycle-hero")`;
const HERO_ROW_PATH = `${HERO_PATH}.locator("div[data-testid=\\"cycle-row\\"]").first()`;
const HERO_NAME_PATH = `${HERO_ROW_PATH}.locator("a span.truncate").first()`;

function rowPath(name: string): string {
  return `${ROWS_PATH}.filter(${JSON.stringify({ hasText: name })}).first()`;
}

function starPath(name: string): string {
  return `${rowPath(name)}.getByTestId("cycle-favorite-marker").first()`;
}

function issueTextPath(name: string): string {
  return `page.getByText(${JSON.stringify(name)},${JSON.stringify({ exact: true })})`;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly gotoUrls: string[] = [];
  readonly fills: { path: string; value: string }[] = [];
  readonly keyboardPresses: string[] = [];
  transferButtons = 1;
  optionTexts: string[] = [];
  notePresent = false;
  noteText = "";
  starCount = 1;
  starVisible = true;
  starAttr: string | null = "false";
  heroRowCount = 1;
  heroNameCount = 1;
  heroNameText = "Current cycle";
  issueTextCount = 1;

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
    if (method === "allTextContents") return path === TRANSFER_OPTIONS_PATH ? [...this.optionTexts] : [];
    if (method === "isVisible") return this.visible(path);
    if (method === "getAttribute")
      return path === starPath("Cycle 7") && args[0] === "data-favorite" ? this.starAttr : null;
    if (method === "fill") {
      this.fills.push({ path, value: String(args[0]) });
      return undefined;
    }
    if (method === "waitFor" || method === "click" || method === "hover") return undefined;
    throw new Error(`unexpected action ${method} at ${path} with ${JSON.stringify(args)}`);
  }

  private count(path: string): number {
    if (path === TRANSFER_BUTTON_PATH) return this.transferButtons;
    if (path === TRANSFER_OPTIONS_PATH) return this.optionTexts.length;
    if (path === TRANSFER_NOTE_PATH) return this.notePresent ? 1 : 0;
    if (path === starPath("Cycle 7")) return this.starCount;
    if (path === HERO_ROW_PATH) return this.heroRowCount;
    if (path === HERO_NAME_PATH) return this.heroNameCount;
    if (path.startsWith("page.getByText(")) return this.issueTextCount;
    return 1;
  }

  private text(path: string): string {
    if (path === TRANSFER_NOTE_PATH) return this.noteText;
    if (path === HERO_NAME_PATH) return this.heroNameText;
    if (path.startsWith(`${TRANSFER_OPTIONS_PATH}.nth(`) && path.endsWith(`.locator("span").first()`)) {
      const index = nthIndex(path);
      return index === null ? "" : (this.optionTexts[index] ?? "");
    }
    return "";
  }

  private visible(path: string): boolean {
    // isShown reads through one more .first(), so the visibility probe
    // lands there rather than on the star path itself.
    if (path === `${starPath("Cycle 7")}.first()`) return this.starVisible;
    return true;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByTestId", "getByText", "getByPlaceholder", "locator"]) {
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
      this.gotoUrls.push(url);
      return undefined;
    };
    node.waitForLoadState = async () => undefined;
    node.waitForTimeout = async () => undefined;
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }

  callsIncluding(substring: string, method: string): Call[] {
    return this.calls.filter((call) => call.method === method && call.path.includes(substring));
  }
}

describe("archivesCyclesOpenRowMenu", () => {
  it("dismisses any open menu, then hovers and clicks the row trigger", async () => {
    const script = new Recorder();
    await script.driver().archivesCyclesOpenRowMenu("Cycle 7");
    expect(script.keyboardPresses).toEqual(["Escape"]);
    expect(script.callsFor(rowPath("Cycle 7"), "hover")).toHaveLength(1);
    const trigger = `${rowPath("Cycle 7")}.getByRole("button",${JSON.stringify({ name: "Cycle actions for Cycle 7" })})`;
    expect(script.callsFor(trigger, "click")).toHaveLength(1);
  });
});

describe("openCycleIssues", () => {
  it("opens the cycle detail address and settles the document", async () => {
    const script = new Recorder();
    await script.driver().openCycleIssues("acme", "pid", "cid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles/cid"]);
  });
});

describe("globalViewIssueVisible", () => {
  it("matches the issue name exactly anywhere on the page", async () => {
    const script = new Recorder();
    script.issueTextCount = 1;
    expect(await script.driver().globalViewIssueVisible("Cycle issue")).toBe(true);
    expect(script.callsFor(issueTextPath("Cycle issue"), "count")).toHaveLength(1);
  });

  it("reports false when no exact match renders", async () => {
    const script = new Recorder();
    script.issueTextCount = 0;
    expect(await script.driver().globalViewIssueVisible("Cycle issue")).toBe(false);
  });
});

describe("cycleTransferButtonVisible", () => {
  it("reads the finished-detail banner entry exactly", async () => {
    const script = new Recorder();
    script.transferButtons = 1;
    expect(await script.driver().cycleTransferButtonVisible()).toBe(true);
    expect(script.callsFor(TRANSFER_BUTTON_PATH, "count")).toHaveLength(1);
  });

  it("reports false when the banner entry is absent", async () => {
    const script = new Recorder();
    script.transferButtons = 0;
    expect(await script.driver().cycleTransferButtonVisible()).toBe(false);
  });
});

describe("cycleTransferOpen", () => {
  it("clicks the banner entry and waits for the picker search", async () => {
    const script = new Recorder();
    await script.driver().cycleTransferOpen();
    expect(script.callsFor(TRANSFER_BUTTON_PATH, "click")).toHaveLength(1);
    expect(script.callsFor(TRANSFER_SEARCH_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cycleTransferOptionNames", () => {
  it("reads the name span of each option, normalizes, and drops blanks", async () => {
    const script = new Recorder();
    script.optionTexts = ["  Target A\n ", "", "Target B\t"];
    expect(await script.driver().cycleTransferOptionNames()).toEqual(["Target A", "Target B"]);
    expect(script.callsFor(TRANSFER_OPTIONS_PATH, "count")).toHaveLength(1);
    expect(script.callsIncluding(`.locator("span").first()`, "innerText")).toHaveLength(3);
  });
});

describe("cycleTransferPick", () => {
  it("clicks the option by substring and waits for the dialog to close", async () => {
    const script = new Recorder();
    await script.driver().cycleTransferPick("Target B");
    const lookups = script.callsFor(TRANSFER_LISTBOX_PATH, "getByRole");
    expect(lookups).toHaveLength(1);
    const options = lookups[0]?.args[1] as { name: RegExp };
    expect(lookups[0]?.args[0]).toBe("option");
    expect(options.name).toBeInstanceOf(RegExp);
    expect(options.name.test("Target B current")).toBe(true);
    expect(options.name.test("Target A upcoming")).toBe(false);
    const waits = script.callsFor(TRANSFER_SEARCH_PATH, "waitFor");
    expect(waits).toHaveLength(1);
    expect(waits[0]?.args[0]).toMatchObject({ state: "detached" });
  });

  it("escapes option names that carry regex syntax", async () => {
    const script = new Recorder();
    await script.driver().cycleTransferPick("Target (B)+");
    const lookups = script.callsFor(TRANSFER_LISTBOX_PATH, "getByRole");
    const options = lookups[0]?.args[1] as { name: RegExp };
    expect(options.name.source).toBe("Target \\(B\\)\\+");
    expect(options.name.test("Target (B)+ current")).toBe(true);
  });
});

describe("cyclesTransferSearchFill", () => {
  it("fills the picker search, including clearing it", async () => {
    const script = new Recorder();
    await script.driver().cyclesTransferSearchFill("target b");
    await script.driver().cyclesTransferSearchFill("");
    expect(script.fills).toEqual([
      { path: TRANSFER_SEARCH_PATH, value: "target b" },
      { path: TRANSFER_SEARCH_PATH, value: "" },
    ]);
  });
});

describe("cyclesTransferEmptyText", () => {
  it("reads null while options render", async () => {
    const script = new Recorder();
    script.optionTexts = ["Target A current"];
    expect(await script.driver().cyclesTransferEmptyText()).toBeNull();
  });

  it("reads null when the picker holds no note", async () => {
    const script = new Recorder();
    script.notePresent = false;
    expect(await script.driver().cyclesTransferEmptyText()).toBeNull();
  });

  it("reads the no-target copy", async () => {
    const script = new Recorder();
    script.notePresent = true;
    script.noteText = "You don't have any current cycle. Please create one to transfer the work items.";
    expect(await script.driver().cyclesTransferEmptyText()).toBe(
      "You don't have any current cycle. Please create one to transfer the work items."
    );
  });

  it("reads null while the picker loads and on a blank note", async () => {
    const script = new Recorder();
    script.notePresent = true;
    script.noteText = "Loading cycles…";
    expect(await script.driver().cyclesTransferEmptyText()).toBeNull();
    script.noteText = "   ";
    expect(await script.driver().cyclesTransferEmptyText()).toBeNull();
  });
});

describe("cyclesRowFavoriteState", () => {
  it("reads absent when the star never mounts", async () => {
    const script = new Recorder();
    script.starCount = 0;
    expect(await script.driver().cyclesRowFavoriteState("Cycle 7")).toBe("absent");
  });

  it("reads absent when the star hides", async () => {
    const script = new Recorder();
    script.starVisible = false;
    expect(await script.driver().cyclesRowFavoriteState("Cycle 7")).toBe("absent");
  });

  it("reads the data-favorite flag otherwise", async () => {
    const script = new Recorder();
    script.starAttr = "true";
    expect(await script.driver().cyclesRowFavoriteState("Cycle 7")).toBe("starred");
    script.starAttr = "false";
    expect(await script.driver().cyclesRowFavoriteState("Cycle 7")).toBe("unstarred");
    script.starAttr = null;
    expect(await script.driver().cyclesRowFavoriteState("Cycle 7")).toBe("unstarred");
  });
});

describe("cyclesRowFavoriteToggle", () => {
  it("clicks the row star", async () => {
    const script = new Recorder();
    await script.driver().cyclesRowFavoriteToggle("Cycle 7");
    expect(script.callsFor(starPath("Cycle 7"), "click")).toHaveLength(1);
  });
});

describe("cyclesEditHeroCycleName", () => {
  it("reads null without a hero row", async () => {
    const script = new Recorder();
    script.heroRowCount = 0;
    expect(await script.driver().cyclesEditHeroCycleName()).toBeNull();
  });

  it("reads null when the row carries no name yet", async () => {
    const script = new Recorder();
    script.heroNameCount = 0;
    expect(await script.driver().cyclesEditHeroCycleName()).toBeNull();
  });

  it("reads null on a blank name and the name otherwise", async () => {
    const script = new Recorder();
    script.heroNameText = "  ";
    expect(await script.driver().cyclesEditHeroCycleName()).toBeNull();
    script.heroNameText = "  Current cycle ";
    expect(await script.driver().cyclesEditHeroCycleName()).toBe("Current cycle");
  });
});

type RowState = "starred" | "unstarred" | "absent";

interface ScriptedDrive {
  clicks: number;
  settles: number;
  page: Page;
}

/** A stub page with a chainable row locator. Clicks and settles are counted. */
function scriptedDrive(): ScriptedDrive {
  const state = { clicks: 0, settles: 0 };
  const chain = {
    filter: () => chain,
    first: () => chain,
    locator: () => chain,
    getByTestId: () => chain,
    click: async () => {
      state.clicks += 1;
    },
  };
  const page = {
    locator: () => chain,
    waitForTimeout: async () => {
      state.settles += 1;
    },
  };
  return Object.assign(state, { page: page as unknown as Page });
}

/** A driver whose row reads replay `states`, holding the last one. */
function driverWithRowStates(script: ScriptedDrive, states: RowState[]): WebNewDriver {
  const queue = [...states];
  const driver = new WebNewDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () =>
    queue.length > 1 ? (queue.shift() ?? "absent") : (queue[0] ?? "absent");
  return driver;
}

/**
 * A driver whose row flips to `want` once the `flipOnClick`-th click has
 * landed, modeling a swallowed click. Keying the flip to the click
 * count instead of a wall-clock delay keeps the first drive's poll on
 * `initial` however many reads the poll makes before its budget runs
 * out, and the second drive's poll on `want`, on any machine speed.
 */
function driverWithClickFlip(
  script: ScriptedDrive,
  initial: RowState,
  want: RowState,
  flipOnClick: number
): WebNewDriver {
  const driver = new WebNewDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () => (script.clicks >= flipOnClick ? want : initial);
  return driver;
}

/** A scripted server read resolving `value` while counting calls. */
function scriptedServer(value: boolean): { calls: number; isServerFavorite: () => Promise<boolean> } {
  const state = { calls: 0 };
  return Object.assign(state, {
    isServerFavorite: async () => {
      state.calls += 1;
      return value;
    },
  });
}

describe("cyclesRowFavoriteToggleConverge", () => {
  it("returns without clicking when already starred", async () => {
    const script = scriptedDrive();
    const driver = driverWithRowStates(script, ["starred"]);
    const server = scriptedServer(true);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(0);
    expect(server.calls).toBe(0);
  });

  it("clicks once and converges on a synchronous flip", async () => {
    const script = scriptedDrive();
    const driver = driverWithRowStates(script, ["unstarred", "starred"]);
    const server = scriptedServer(false);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(0);
    expect(server.calls).toBe(0);
  });

  it("polls until the flip lands", async () => {
    const script = scriptedDrive();
    const driver = driverWithRowStates(script, ["unstarred", "unstarred", "starred"]);
    const server = scriptedServer(false);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(1);
    expect(script.settles).toBe(1);
    expect(server.calls).toBe(0);
  });

  it("re-drives after a swallowed click and converges", async () => {
    const script = scriptedDrive();
    // The first 30ms poll never sees the flip; the second drive lands it.
    const driver = driverWithClickFlip(script, "unstarred", "starred", 2);
    const server = scriptedServer(false);
    await driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30);
    expect(script.clicks).toBe(2);
    expect(server.calls).toBe(1);
  });

  it("throws without a second click when the server flipped but the row did not", async () => {
    const script = scriptedDrive();
    const driver = driverWithRowStates(script, ["unstarred"]);
    const server = scriptedServer(true);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30)
    ).rejects.toThrow("refusing another toggle");
    expect(script.clicks).toBe(1);
    expect(server.calls).toBe(1);
  });

  it("throws after two toggles when nothing flips", async () => {
    const script = scriptedDrive();
    const driver = driverWithRowStates(script, ["unstarred"]);
    const server = scriptedServer(false);
    await expect(
      driver.cyclesRowFavoriteToggleConverge("Cycle", "starred", server.isServerFavorite, 30)
    ).rejects.toThrow("after 2 toggles");
    expect(script.clicks).toBe(2);
    expect(server.calls).toBe(2);
  });
});

/**
 * A driver whose row and server reads follow scripted timelines of the
 * click count and the read counts, so clobbers and late commits replay
 * deterministically without any clock.
 */
function driverWithTimeline(
  script: ScriptedDrive,
  rowAt: (clicks: number, rowReads: number) => RowState,
  serverAt: (clicks: number, serverReads: number) => boolean
): { driver: WebNewDriver; isServerFavorite: () => Promise<boolean> } {
  let rowReads = 0;
  let serverReads = 0;
  const driver = new WebNewDriver(script.page);
  const scripted = driver as unknown as { cyclesRowFavoriteState: (name: string) => Promise<RowState> };
  scripted.cyclesRowFavoriteState = async () => {
    rowReads += 1;
    return rowAt(script.clicks, rowReads);
  };
  return {
    driver,
    isServerFavorite: async () => {
      serverReads += 1;
      return serverAt(script.clicks, serverReads);
    },
  };
}

describe("cyclesRowFavoriteSettleAgreement", () => {
  it("returns without clicking when already agreed", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      () => "starred",
      () => true
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(0);
    expect(script.settles).toBe(11);
  });

  it("heals a stale read that overwrites the flip", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      (_clicks, rowReads) => (rowReads === 1 || rowReads === 3 ? "unstarred" : "starred"),
      () => true
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(2);
  });

  it("throws instead of healing forever when the row never sticks", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      (_clicks, rowReads) => (rowReads === 2 ? "starred" : "unstarred"),
      () => true
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500)
    ).rejects.toThrow("refusing further toggles");
    expect(script.clicks).toBe(3);
  });

  it("waits out a pending server and then agrees", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      (clicks) => (clicks >= 1 ? "starred" : "unstarred"),
      (_clicks, serverReads) => serverReads >= 5
    );
    await timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 500);
    expect(script.clicks).toBe(1);
  });

  it("throws when the server never flips", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      (clicks) => (clicks >= 1 ? "starred" : "unstarred"),
      () => false
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 40)
    ).rejects.toThrow("never settled on starred");
    expect(script.clicks).toBe(1);
  });

  it("throws when neither side ever agrees", async () => {
    const script = scriptedDrive();
    const timeline = driverWithTimeline(
      script,
      (_clicks, rowReads) => (rowReads === 2 ? "starred" : "unstarred"),
      () => false
    );
    await expect(
      timeline.driver.cyclesRowFavoriteSettleAgreement("Cycle", "starred", timeline.isServerFavorite, 40)
    ).rejects.toThrow("never settled on starred");
    expect(script.clicks).toBe(1);
  });
});
