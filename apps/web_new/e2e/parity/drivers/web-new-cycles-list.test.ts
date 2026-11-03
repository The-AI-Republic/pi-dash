// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver live cycles list methods (NEWFRONT-716
// gate, CYC-001–CYC-008): the list opener, visible names, group headings,
// counts, toggles and expansion, the hero name, row links and clicks, the
// detail opener, name and issue reads, the row-facts probe, peek open and
// close, reload, back, and the search box. A recording fake stands in for
// Playwright: each locator remembers the path of calls that built it,
// scripted answers stand in for the page, and a test asserts both which
// selector the driver used and what it does with the answer. The
// row-facts probe runs its page function against a tiny fake DOM so every
// marker branch resolves deterministically. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "nth", "getByRole", "getByTestId", "locator", "filter"];
const ACTIONS = [
  "count",
  "click",
  "fill",
  "press",
  "innerText",
  "allInnerTexts",
  "inputValue",
  "getAttribute",
  "hover",
  "waitFor",
  "isVisible",
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

const ROWS_PATH = `page.locator("div[data-testid=\\"cycle-row\\"]")`;
// A regex nested in an options object stringifies to {}, so every
// button-with-text lookup shares one path; tests assert the pattern
// itself from the recorded args.
const GROUP_BUTTONS_PATH = `page.locator("button",{"hasText":{}})`;
const HERO_PATH = `page.getByTestId("cycle-hero")`;
const HERO_ROW_PATH = `${HERO_PATH}.locator("div[data-testid=\\"cycle-row\\"]").first()`;
const HERO_NAME_PATH = `${HERO_ROW_PATH}.locator("a span.truncate").first()`;
const DETAIL_H2_PATH = `page.getByTestId("issues-header").locator("h2").first()`;
const REGION_PARAS_PATH = `page.getByRole("region",{"name":"Issues"}).locator("article a[data-row-anchor=\\"true\\"] > p")`;
const PEEK_PATH = `page.getByTestId("cycle-preview")`;
const PEEK_H4_PATH = `${PEEK_PATH}.locator("h4").first()`;
const SEARCH_BOX_PATH = `page.getByTestId("cycles-search-box")`;
const SEARCH_TOGGLE_PATH = `page.getByTestId("cycles-search-toggle")`;
const SEARCH_INPUT_PATH = `page.getByRole("textbox",{"name":"Search cycles"})`;
const SEARCH_CLOSE_PATH = `page.getByRole("button",{"name":"Close search"})`;
const FILTER_TRIGGER_PATH = `page.getByTestId("cycles-filters-trigger")`;
const FILTER_PANEL_PATH = `page.getByTestId("cycles-filters-panel")`;

function rowPath(name: string): string {
  return `${ROWS_PATH}.filter(${JSON.stringify({ hasText: name })}).first()`;
}

function rowLinkPath(name: string): string {
  return `${rowPath(name)}.locator("a").first()`;
}

/** A minimal DOM node: tag, attributes, own text, and children. */
class FakeEl {
  readonly children: FakeEl[] = [];
  parent: FakeEl | null = null;

  constructor(
    readonly tag: string = "span",
    readonly attrs: Record<string, string> = {},
    readonly text: string = "",
    children: FakeEl[] = []
  ) {
    for (const child of children) {
      child.parent = this;
      this.children.push(child);
    }
  }

  get textContent(): string {
    return this.text + this.children.map((child) => child.textContent).join("");
  }

  get parentElement(): FakeEl | null {
    return this.parent;
  }

  getAttribute(name: string): string | null {
    return this.attrs[name] ?? null;
  }

  querySelector(selector: string): FakeEl | null {
    return this.querySelectorAll(selector)[0] ?? null;
  }

  querySelectorAll(selector: string): FakeEl[] {
    const found: FakeEl[] = [];
    const walk = (el: FakeEl): void => {
      for (const child of el.children) {
        if (fakeMatches(child, selector)) found.push(child);
        walk(child);
      }
    };
    walk(this);
    return found;
  }
}

function fakeMatches(el: FakeEl, selector: string): boolean {
  const attrMatch = /\[([a-z-]+)="([^"]*)"\]/.exec(selector);
  const attrName = attrMatch?.[1] ?? null;
  const attrValue = attrMatch?.[2] ?? null;
  const rest = selector.replace(/\[([a-z-]+)="([^"]*)"\]/, "");
  const classMatch = /\.([a-z-]+)/.exec(rest);
  const className = classMatch?.[1] ?? null;
  const tag = rest.replace(/\.([a-z-]+)/, "");
  if (tag !== "" && el.tag !== tag) return false;
  if (className !== null && !(el.attrs["class"] ?? "").split(" ").includes(className)) return false;
  if (attrName !== null && el.attrs[attrName] !== attrValue) return false;
  return true;
}

function nthIndex(path: string): number | null {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match === null ? null : Number(match[1]);
}

class Recorder {
  readonly calls: Call[] = [];
  readonly gotoUrls: string[] = [];
  readonly waitedUrls: unknown[] = [];
  readonly keyboardPresses: string[] = [];
  readonly fills: { path: string; value: string }[] = [];
  readonly presses: { path: string; key: string }[] = [];
  pageUrl = "https://app.test/acme/projects/pid/cycles";
  reloaded = 0;
  wentBack = 0;
  rowCount = 0;
  rowTexts: string[] = [];
  rowHref: string | null = null;
  groupButtonCount = 0;
  groupButtonTexts: string[] = [];
  groupButtonVisible: boolean[] = [];
  groupExpandedAttr: string | null = "true";
  heroRowCount = 1;
  heroNameCount = 1;
  heroNameText = "Current cycle";
  h2Count = 1;
  h2Text = "Current cycle";
  regionCount = 0;
  regionTexts: string[] = [];
  peekH4Count = 0;
  peekH4Visible = true;
  peekH4Text = "";
  searchBoxCount = 0;
  searchValue = "";
  panelCount = 1;
  fakeRowRoot: FakeEl | null = null;

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
    if (method === "allInnerTexts") return path === REGION_PARAS_PATH ? [...this.regionTexts] : [];
    if (method === "inputValue") return this.searchValue;
    if (method === "isVisible") return this.visible(path);
    if (method === "getAttribute") return this.attribute(path, String(args[0]));
    if (method === "evaluate") {
      if (this.fakeRowRoot === null) throw new Error("no fake row root");
      return (args[0] as (el: FakeEl) => unknown)(this.fakeRowRoot);
    }
    if (method === "fill") {
      this.fills.push({ path, value: String(args[0]) });
      return undefined;
    }
    if (method === "press") {
      this.presses.push({ path, key: String(args[0]) });
      return undefined;
    }
    if (method === "waitFor" || method === "click" || method === "hover") return undefined;
    throw new Error(`unexpected action ${method} at ${path} with ${JSON.stringify(args)}`);
  }

  private count(path: string): number {
    if (path === ROWS_PATH) return this.rowCount;
    if (path === GROUP_BUTTONS_PATH) return this.groupButtonCount;
    if (path === DETAIL_H2_PATH) return this.h2Count;
    if (path === REGION_PARAS_PATH) return this.regionCount;
    if (path === PEEK_H4_PATH) return this.peekH4Count;
    if (path === SEARCH_BOX_PATH) return this.searchBoxCount;
    if (path === FILTER_PANEL_PATH) return this.panelCount;
    if (path === HERO_ROW_PATH) return this.heroRowCount;
    if (path === HERO_NAME_PATH) return this.heroNameCount;
    return 1;
  }

  private text(path: string): string {
    const rowIndex = path.startsWith(ROWS_PATH) ? nthIndex(path) : null;
    if (rowIndex !== null) return this.rowTexts[rowIndex] ?? "";
    const groupIndex = path.startsWith(GROUP_BUTTONS_PATH) ? nthIndex(path) : null;
    if (groupIndex !== null) return this.groupButtonTexts[groupIndex] ?? "";
    if (path === DETAIL_H2_PATH) return this.h2Text;
    if (path === PEEK_H4_PATH) return this.peekH4Text;
    if (path === HERO_NAME_PATH) return this.heroNameText;
    return "";
  }

  private visible(path: string): boolean {
    const groupIndex = path.startsWith(GROUP_BUTTONS_PATH) ? nthIndex(path) : null;
    if (groupIndex !== null) return this.groupButtonVisible[groupIndex] ?? true;
    if (path === PEEK_H4_PATH) return this.peekH4Visible;
    return true;
  }

  private attribute(path: string, name: string): string | null {
    if (name === "aria-expanded" && path === `${GROUP_BUTTONS_PATH}.first()`) return this.groupExpandedAttr;
    if (name === "href" && path.endsWith(`.locator("a").first()`)) return this.rowHref;
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
      this.gotoUrls.push(url);
      return undefined;
    };
    node.reload = async () => {
      this.reloaded += 1;
      return undefined;
    };
    node.goBack = async () => {
      this.wentBack += 1;
      return undefined;
    };
    node.url = () => this.pageUrl;
    node.waitForLoadState = async () => undefined;
    node.waitForURL = async (pattern: unknown) => {
      this.waitedUrls.push(pattern);
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

describe("cyclesOpenList", () => {
  it("opens the live list address", async () => {
    const script = new Recorder();
    await script.driver().cyclesOpenList("acme", "pid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles"]);
  });
});

describe("cyclesVisibleNames", () => {
  it("reads every row link name in order and drops blanks", async () => {
    const script = new Recorder();
    script.rowCount = 3;
    script.rowTexts = ["  Current cycle ", "", "Upcoming cycle"];
    expect(await script.driver().cyclesVisibleNames()).toEqual(["Current cycle", "Upcoming cycle"]);
    expect(script.callsFor(ROWS_PATH, "count")).toHaveLength(1);
  });
});

describe("cyclesGroupHeadings", () => {
  it("reads the visible disclosure buttons in DOM order", async () => {
    const script = new Recorder();
    script.groupButtonCount = 4;
    script.groupButtonTexts = ["Active cycle\n1", "Upcoming cycle  2", "Completed cycle 1", "Stale"];
    script.groupButtonVisible = [true, true, true, false];
    expect(await script.driver().cyclesGroupHeadings()).toEqual([
      "Active cycle 1",
      "Upcoming cycle 2",
      "Completed cycle 1",
    ]);
    const lookups = script.calls.filter((call) => call.path === "page" && call.method === "locator");
    expect(lookups).toHaveLength(1);
    const lookup = lookups[0];
    if (lookup === undefined) throw new Error("expected one locator lookup");
    const pattern = (lookup.args[1] as { hasText: RegExp }).hasText;
    expect(pattern).toBeInstanceOf(RegExp);
    expect(pattern.test("Active cycle 1")).toBe(true);
    expect(pattern.test("Stale")).toBe(false);
  });
});

describe("cyclesGroupCounts", () => {
  it("parses the trailing counts", async () => {
    const script = new Recorder();
    script.groupButtonCount = 3;
    script.groupButtonTexts = ["Active cycle 1", "Upcoming cycle 2", "Completed cycle 1"];
    script.groupButtonVisible = [true, true, true];
    expect(await script.driver().cyclesGroupCounts()).toEqual({ upcoming: 2, completed: 1 });
  });

  it("reads zero for a missing group", async () => {
    const script = new Recorder();
    script.groupButtonCount = 2;
    script.groupButtonTexts = ["Active cycle 1", "Upcoming cycle 2"];
    script.groupButtonVisible = [true, true];
    expect(await script.driver().cyclesGroupCounts()).toEqual({ upcoming: 2, completed: 0 });
  });

  it("reads zero when a heading carries no trailing digits", async () => {
    const script = new Recorder();
    script.groupButtonCount = 3;
    script.groupButtonTexts = ["Active cycle 1", "Upcoming cycle", "Completed cycle 1"];
    script.groupButtonVisible = [true, true, true];
    expect(await script.driver().cyclesGroupCounts()).toEqual({ upcoming: 0, completed: 1 });
  });
});

describe("cyclesGroupToggle", () => {
  it("clicks the section disclosure by prefix", async () => {
    const script = new Recorder();
    await script.driver().cyclesGroupToggle("Upcoming");
    const lookups = script.calls.filter((call) => call.path === "page" && call.method === "locator");
    expect(lookups).toHaveLength(1);
    const lookup = lookups[0];
    if (lookup === undefined) throw new Error("expected one locator lookup");
    const pattern = (lookup.args[1] as { hasText: RegExp }).hasText;
    expect(pattern.source).toBe("^Upcoming");
    expect(script.callsFor(`${GROUP_BUTTONS_PATH}.first()`, "click")).toHaveLength(1);
  });
});

describe("cyclesGroupExpanded", () => {
  it("reads the disclosure state by prefix", async () => {
    const script = new Recorder();
    script.groupExpandedAttr = "true";
    expect(await script.driver().cyclesGroupExpanded("Completed")).toBe(true);
    script.groupExpandedAttr = "false";
    expect(await script.driver().cyclesGroupExpanded("Completed")).toBe(false);
    script.groupExpandedAttr = null;
    expect(await script.driver().cyclesGroupExpanded("Completed")).toBe(false);
  });
});

describe("cyclesHeroName", () => {
  it("reads the hero row name through the shared reader", async () => {
    const script = new Recorder();
    script.heroNameText = "Current cycle";
    expect(await script.driver().cyclesHeroName()).toBe("Current cycle");
    script.heroRowCount = 0;
    expect(await script.driver().cyclesHeroName()).toBeNull();
  });
});

describe("cyclesRowHref", () => {
  it("reads the row link address", async () => {
    const script = new Recorder();
    script.rowHref = "/acme/projects/pid/cycles/cid";
    expect(await script.driver().cyclesRowHref("Cycle 7")).toBe("/acme/projects/pid/cycles/cid");
    script.rowHref = null;
    expect(await script.driver().cyclesRowHref("Cycle 7")).toBeNull();
  });
});

describe("cyclesClickRow", () => {
  it("clicks the row link and waits for the detail address and title", async () => {
    const script = new Recorder();
    await script.driver().cyclesClickRow("Cycle 7");
    expect(script.callsFor(rowLinkPath("Cycle 7"), "click")).toHaveLength(1);
    expect(script.waitedUrls).toHaveLength(1);
    const pattern = script.waitedUrls[0] as RegExp;
    expect(pattern.test("/acme/projects/pid/cycles/cid")).toBe(true);
    expect(pattern.test("/acme/projects/pid/cycles")).toBe(false);
    expect(script.callsFor(DETAIL_H2_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesOpenDetail", () => {
  it("opens the detail address and waits for the title", async () => {
    const script = new Recorder();
    await script.driver().cyclesOpenDetail("acme", "pid", "cid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles/cid"]);
    expect(script.callsFor(DETAIL_H2_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesDetailName", () => {
  it("reads the header title", async () => {
    const script = new Recorder();
    script.h2Text = "  Current cycle ";
    expect(await script.driver().cyclesDetailName()).toBe("Current cycle");
  });

  it("reads null without a title", async () => {
    const script = new Recorder();
    script.h2Count = 0;
    expect(await script.driver().cyclesDetailName()).toBeNull();
    script.h2Count = 1;
    script.h2Text = "   ";
    expect(await script.driver().cyclesDetailName()).toBeNull();
  });
});

describe("cyclesDetailIssueNames", () => {
  it("reads through the shared issues reader", async () => {
    const script = new Recorder();
    script.regionCount = 2;
    script.regionTexts = ["issue a", "issue b"];
    expect(await script.driver().cyclesDetailIssueNames()).toEqual(["issue a", "issue b"]);
    expect(script.callsFor(REGION_PARAS_PATH, "allInnerTexts")).toHaveLength(1);
  });
});

function ringEl(text: string): FakeEl {
  return new FakeEl("span", { "data-testid": "cycle-progress-ring", role: "img", "aria-label": "42% complete" }, text);
}

function datesEl(text: string): FakeEl {
  return new FakeEl("span", { "data-testid": "cycle-date-span" }, text);
}

function countGroupEl(count: string): FakeEl {
  return new FakeEl("span", { class: "inline-flex" }, "", [
    new FakeEl("span", {}, ""),
    new FakeEl("span", {}, count),
    new FakeEl("span", { class: "sr-only" }, "work items"),
  ]);
}

function avatarEl(label: string): FakeEl {
  return new FakeEl("span", { role: "img", "aria-label": label }, "");
}

function starEl(favorite: string | null): FakeEl {
  return new FakeEl(
    "button",
    favorite === null
      ? { "data-testid": "cycle-favorite-marker" }
      : { "data-testid": "cycle-favorite-marker", "data-favorite": favorite },
    ""
  );
}

describe("cyclesRowFacts", () => {
  it("reads every marker off a full row", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", [
      ringEl("42%"),
      datesEl("Sep 1 – Dec 31"),
      countGroupEl("2"),
      avatarEl("Ada Creator"),
      starEl("true"),
    ]);
    expect(await script.driver().cyclesRowFacts("Cycle 7")).toEqual({
      progress: "42%",
      dateText: "Sep 1 – Dec 31",
      workItemCount: "2",
      hasCreatorAvatar: true,
      hasFavorite: true,
      favoriteSelected: true,
    });
  });

  it("reads done at a full ring", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", [ringEl("100%")]);
    const facts = await script.driver().cyclesRowFacts("Cycle 7");
    expect(facts.progress).toBe("done");
    expect(facts.dateText).toBeNull();
    expect(facts.workItemCount).toBeNull();
    expect(facts.hasCreatorAvatar).toBe(false);
    expect(facts.hasFavorite).toBe(false);
    expect(facts.favoriteSelected).toBe(false);
  });

  it("reads null for a missing or blank ring and date span", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", [ringEl("  "), datesEl("   ")]);
    const facts = await script.driver().cyclesRowFacts("Cycle 7");
    expect(facts.progress).toBeNull();
    expect(facts.dateText).toBeNull();
  });

  it("reads null without a count marker and with an uncounted one", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", [new FakeEl("span", {}, "7")]);
    expect((await script.driver().cyclesRowFacts("Cycle 7")).workItemCount).toBeNull();
    script.fakeRowRoot = new FakeEl("div", {}, "", [
      new FakeEl("span", { class: "inline-flex" }, "", [
        new FakeEl("span", {}, ""),
        new FakeEl("span", { class: "sr-only" }, "work items"),
      ]),
    ]);
    expect((await script.driver().cyclesRowFacts("Cycle 7")).workItemCount).toBeNull();
  });

  it("ignores the ring image when looking for avatars", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", [ringEl("42%")]);
    expect((await script.driver().cyclesRowFacts("Cycle 7")).hasCreatorAvatar).toBe(false);
  });

  it("reads an unmounted and an unselected star", async () => {
    const script = new Recorder();
    script.fakeRowRoot = new FakeEl("div", {}, "", []);
    const absent = await script.driver().cyclesRowFacts("Cycle 7");
    expect(absent.hasFavorite).toBe(false);
    expect(absent.favoriteSelected).toBe(false);
    script.fakeRowRoot = new FakeEl("div", {}, "", [starEl("false")]);
    const plain = await script.driver().cyclesRowFacts("Cycle 7");
    expect(plain.hasFavorite).toBe(true);
    expect(plain.favoriteSelected).toBe(false);
    script.fakeRowRoot = new FakeEl("div", {}, "", [starEl(null)]);
    expect((await script.driver().cyclesRowFacts("Cycle 7")).favoriteSelected).toBe(false);
  });
});

describe("cyclesOpenPeek", () => {
  it("hovers the row, eyes it open, and waits for the named panel", async () => {
    const script = new Recorder();
    await script.driver().cyclesOpenPeek("Cycle 7");
    expect(script.callsFor(rowPath("Cycle 7"), "hover")).toHaveLength(1);
    const eye = `${rowPath("Cycle 7")}.getByRole("button",${JSON.stringify({ name: "Quick look at Cycle 7" })})`;
    expect(script.callsFor(eye, "click")).toHaveLength(1);
    const named = `${PEEK_PATH}.locator("h4",${JSON.stringify({ hasText: "Cycle 7" })})`;
    expect(script.callsFor(named, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesOpenPeekLink", () => {
  it("opens the list at the peek key and waits for the panel", async () => {
    const script = new Recorder();
    await script.driver().cyclesOpenPeekLink("acme", "pid", "cid");
    expect(script.gotoUrls).toEqual(["/acme/projects/pid/cycles?peekCycle=cid"]);
    expect(script.callsFor(PEEK_H4_PATH, "waitFor")).toHaveLength(1);
  });
});

describe("cyclesPeekName", () => {
  it("reads the panel title", async () => {
    const script = new Recorder();
    script.peekH4Count = 1;
    script.peekH4Text = "  Peeked cycle ";
    expect(await script.driver().cyclesPeekName()).toBe("Peeked cycle");
  });

  it("reads null without a panel, while hidden, and on a blank title", async () => {
    const script = new Recorder();
    script.peekH4Count = 0;
    expect(await script.driver().cyclesPeekName()).toBeNull();
    script.peekH4Count = 1;
    script.peekH4Visible = false;
    expect(await script.driver().cyclesPeekName()).toBeNull();
    script.peekH4Visible = true;
    script.peekH4Text = "  ";
    expect(await script.driver().cyclesPeekName()).toBeNull();
  });
});

describe("cyclesPeekParam", () => {
  it("reads the address key", async () => {
    const script = new Recorder();
    script.pageUrl = "https://app.test/acme/projects/pid/cycles?peekCycle=cid";
    expect(await script.driver().cyclesPeekParam()).toBe("cid");
    script.pageUrl = "https://app.test/acme/projects/pid/cycles";
    expect(await script.driver().cyclesPeekParam()).toBeNull();
  });
});

describe("cyclesClosePeek", () => {
  it("dismisses the panel and waits for it to detach", async () => {
    const script = new Recorder();
    await script.driver().cyclesClosePeek();
    const closer = `${PEEK_PATH}.getByRole("button",${JSON.stringify({ name: "Close preview" })})`;
    expect(script.callsFor(closer, "click")).toHaveLength(1);
    const waits = script.callsFor(PEEK_H4_PATH, "waitFor");
    expect(waits).toHaveLength(1);
    expect(waits[0]?.args[0]).toMatchObject({ state: "detached" });
  });
});

describe("cyclesReload", () => {
  it("reloads the page", async () => {
    const script = new Recorder();
    await script.driver().cyclesReload();
    expect(script.reloaded).toBe(1);
  });
});

describe("cyclesGoBack", () => {
  it("navigates back", async () => {
    const script = new Recorder();
    await script.driver().cyclesGoBack();
    expect(script.wentBack).toBe(1);
  });
});

describe("cyclesSearchExpanded", () => {
  it("reads the mounted box", async () => {
    const script = new Recorder();
    script.searchBoxCount = 0;
    expect(await script.driver().cyclesSearchExpanded()).toBe(false);
    script.searchBoxCount = 1;
    expect(await script.driver().cyclesSearchExpanded()).toBe(true);
  });
});

describe("cyclesSearchOpen", () => {
  it("opens a collapsed box and leaves an open one alone", async () => {
    const script = new Recorder();
    script.searchBoxCount = 0;
    await script.driver().cyclesSearchOpen();
    expect(script.callsFor(SEARCH_TOGGLE_PATH, "click")).toHaveLength(1);
    expect(script.callsFor(SEARCH_BOX_PATH, "waitFor")).toHaveLength(1);
    script.searchBoxCount = 1;
    await script.driver().cyclesSearchOpen();
    expect(script.callsFor(SEARCH_TOGGLE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesSearchFill", () => {
  it("fills the box field", async () => {
    const script = new Recorder();
    await script.driver().cyclesSearchFill("alpha");
    expect(script.fills).toEqual([{ path: SEARCH_INPUT_PATH, value: "alpha" }]);
  });
});

describe("cyclesSearchText", () => {
  it("reads collapsed as empty and open from the field", async () => {
    const script = new Recorder();
    script.searchBoxCount = 0;
    expect(await script.driver().cyclesSearchText()).toBe("");
    expect(script.callsFor(SEARCH_INPUT_PATH, "inputValue")).toHaveLength(0);
    script.searchBoxCount = 1;
    script.searchValue = "alpha";
    expect(await script.driver().cyclesSearchText()).toBe("alpha");
  });
});

describe("cyclesSearchEscape", () => {
  it("sends Escape to the field", async () => {
    const script = new Recorder();
    await script.driver().cyclesSearchEscape();
    expect(script.presses).toEqual([{ path: SEARCH_INPUT_PATH, key: "Escape" }]);
  });
});

describe("cyclesSearchClear", () => {
  it("clicks the box clear control", async () => {
    const script = new Recorder();
    await script.driver().cyclesSearchClear();
    expect(script.callsFor(SEARCH_CLOSE_PATH, "click")).toHaveLength(1);
  });
});

describe("cyclesClickAway", () => {
  it("opens and closes the filters menu away from the search box", async () => {
    const script = new Recorder();
    await script.driver().cyclesClickAway();
    expect(script.callsFor(FILTER_TRIGGER_PATH, "click")).toHaveLength(1);
    expect(script.callsFor(FILTER_PANEL_PATH, "waitFor")).toHaveLength(3);
    expect(script.keyboardPresses).toEqual(["Escape"]);
  });
});
