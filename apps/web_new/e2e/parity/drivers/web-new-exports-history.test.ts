// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver previous-exports history methods
// (NEWFRONT-767, EXP-006..013). A recording fake stands in for
// Playwright: each locator remembers the path of calls that built it,
// scripted answers stand in for the page, and a test asserts both which
// selector the driver used and what it does with the answer. Never about
// a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["filter", "first", "last", "nth", "getByRole", "getByText", "locator", "getByTestId"];
const ACTIONS = ["count", "click", "innerText", "textContent", "allTextContents", "getAttribute", "isDisabled"];

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

/** One scripted history row behind the fake table. */
interface ScriptRow {
  byCell: string;
  glyph: string;
  img: string | null;
  date: string;
  countText: string;
  format: string;
  status: string;
  link: { href: string | null; target: string | null } | null;
  downloadText: string;
}

const TABLE_PATH = `page.getByRole("table",{"name":"Previous exports"})`;
const PREV_PATH = `page.getByRole("button",{"name":"Prev","exact":true})`;
const NEXT_PATH = `page.getByRole("button",{"name":"Next","exact":true})`;
const STATUS_PATH = `page.getByRole("status",{"name":"Loading export history"})`;
const HEADING_PATH = `page.getByRole("heading",{"name":"No exports yet"})`;

/**
 * Whether the path addresses the manual refresh. Its name option is a
 * regular expression, which collapses in the rendered path, so the
 * refresh is the button query that is neither Prev nor Next; tests
 * assert the expression itself from the recorded chain call.
 */
function isRefreshPath(path: string): boolean {
  return path !== PREV_PATH && path !== NEXT_PATH && path.startsWith('page.getByRole("button",');
}

function rowIndexOf(path: string): number {
  const match = /\.nth\((\d+)\)/.exec(path);
  return match?.[1] === undefined ? 0 : Number(match[1]);
}

function cellIndexOf(path: string): number | null {
  const matches = [...path.matchAll(/\.nth\((\d+)\)/g)];
  if (matches.length < 2) return null;
  return Number(matches[1]?.[1] ?? 0);
}

class Recorder {
  readonly calls: Call[] = [];
  readonly routes: Array<{ pattern: unknown; handler: (route: never) => Promise<void> }> = [];
  readonly listeners: Array<{ event: string; handler: (request: never) => void }> = [];
  rows: ScriptRow[] = [];
  tablePresent = true;
  refreshLabel: string | null = "Refresh status";
  prevPresent = true;
  nextPresent = true;
  prevDisabled = true;
  nextDisabled = false;
  skeletonPresent = false;
  emptyParts: string[] | null = null;
  popupUrl = "https://files.test/export.csv";
  popupCloseFails = false;

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
    if (method === "getAttribute") return this.attribute(path, args[0]);
    if (method === "allTextContents") return this.emptyParts ?? [];
    if (method === "isDisabled") {
      if (path === `${PREV_PATH}.first()`) return this.prevDisabled;
      if (path === `${NEXT_PATH}.first()`) return this.nextDisabled;
      return false;
    }
    return undefined;
  }

  private count(path: string): number {
    if (path === TABLE_PATH) return this.tablePresent ? 1 : 0;
    if (path === `${TABLE_PATH}.locator("tbody tr")`) return this.rows.length;
    if (isRefreshPath(path) && !path.includes(".first()")) return this.refreshLabel === null ? 0 : 1;
    if (path === PREV_PATH) return this.prevPresent ? 1 : 0;
    if (path === NEXT_PATH) return this.nextPresent ? 1 : 0;
    if (path === STATUS_PATH) return this.skeletonPresent ? 1 : 0;
    if (path === HEADING_PATH) return this.emptyParts === null ? 0 : 1;
    if (path.includes('.locator("img")')) return (this.rows[rowIndexOf(path)]?.img ?? null) === null ? 0 : 1;
    if (path.includes('getByRole("link")')) return this.rows[rowIndexOf(path)]?.link === null ? 0 : 1;
    return 0;
  }

  private text(path: string): string {
    if (isRefreshPath(path)) return this.refreshLabel ?? "";
    const row = this.rows[rowIndexOf(path)];
    if (!row) return "";
    if (path.endsWith('.getByRole("img").first()')) return row.glyph;
    if (path.includes('.getByRole("cell").last()')) return row.downloadText;
    switch (cellIndexOf(path)) {
      case 0:
        return row.byCell;
      case 1:
        return row.date;
      case 2:
        return row.countText;
      case 3:
        return row.format;
      case 4:
        return row.status;
      default:
        return "";
    }
  }

  private attribute(path: string, name: unknown): string | null {
    const row = this.rows[rowIndexOf(path)];
    if (path.includes('.locator("img").first()') && name === "src") return row?.img ?? null;
    if (path.includes('getByRole("link").first()')) {
      if (name === "href") return row?.link?.href ?? null;
      if (name === "target") return row?.link?.target ?? null;
    }
    return null;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByText", "locator", "getByTestId"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.on = (event: string, handler: (request: never) => void) => {
      this.listeners.push({ event, handler });
    };
    node.route = async (pattern: unknown, handler: (route: never) => Promise<void>) => {
      this.routes.push({ pattern, handler });
    };
    node.waitForEvent = async () => ({
      url: () => this.popupUrl,
      close: async () => {
        if (this.popupCloseFails) throw new Error("already closed");
      },
    });
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  requests(count: (request: { method(): string; url(): string }) => void): void {
    for (const listener of this.listeners) {
      if (listener.event === "request") count(listener.handler as never);
    }
  }
}

function avatarRow(): ScriptRow {
  return {
    byCell: "Ana Siva",
    glyph: "",
    img: "https://files.test/ana.png",
    date: "Apr 11, 2024",
    countText: "2 project(s)",
    format: "CSV",
    status: "completed",
    link: { href: "https://files.test/export.csv", target: "_blank" },
    downloadText: "Download",
  };
}

function initialRow(): ScriptRow {
  return {
    byCell: "AAna Siva",
    glyph: "A",
    img: null,
    date: "Apr 10, 2024",
    countText: "1 project(s)",
    format: "Excel",
    status: "processing",
    link: null,
    downloadText: "-",
  };
}

describe("exportsSettingsHistoryRows", () => {
  it("reads an avatar row through the named table", async () => {
    const rec = new Recorder();
    rec.rows = [avatarRow()];
    const rows = await rec.driver().exportsSettingsHistoryRows();

    expect(rows).toEqual([
      {
        exportedBy: "Ana Siva",
        avatarUrl: "https://files.test/ana.png",
        initial: null,
        exportedOn: "Apr 11, 2024",
        projectCount: 2,
        format: "CSV",
        status: "completed",
        download: { kind: "link", href: "https://files.test/export.csv", target: "_blank" },
      },
    ]);
    expect(rec.calls.some((call) => call.path === TABLE_PATH && call.method === "count")).toBe(true);
  });

  it("strips the initial glyph to recover the display name", async () => {
    const rec = new Recorder();
    rec.rows = [initialRow()];
    const rows = await rec.driver().exportsSettingsHistoryRows();

    expect(rows).toEqual([
      {
        exportedBy: "Ana Siva",
        avatarUrl: null,
        initial: "A",
        exportedOn: "Apr 10, 2024",
        projectCount: 1,
        format: "Excel",
        status: "processing",
        download: { kind: "dash" },
      },
    ]);
  });

  it("returns no rows when the table is absent", async () => {
    const rec = new Recorder();
    rec.tablePresent = false;
    await expect(rec.driver().exportsSettingsHistoryRows()).resolves.toEqual([]);
  });

  it("reads the expired marker case-insensitively", async () => {
    const rec = new Recorder();
    rec.rows = [{ ...initialRow(), downloadText: "EXPIRED" }];
    const rows = await rec.driver().exportsSettingsHistoryRows();

    expect(rows[0]?.download).toEqual({ kind: "expired" });
  });

  it("falls back to empty link facts when the anchor omits them", async () => {
    const rec = new Recorder();
    rec.rows = [{ ...avatarRow(), link: { href: null, target: null } }];
    const rows = await rec.driver().exportsSettingsHistoryRows();

    expect(rows[0]?.download).toEqual({ kind: "link", href: "", target: "" });
  });
});

describe("exportsSettingsRefresh", () => {
  it("clicks the refresh in either label", async () => {
    const rec = new Recorder();
    await rec.driver().exportsSettingsRefresh();

    const queries = rec.calls.filter((call) => call.path === "page" && call.method === "getByRole");
    const names = queries
      .map((call) => (call.args[1] as { name?: unknown } | undefined)?.name)
      .filter((name): name is RegExp => name instanceof RegExp);
    expect(names).toHaveLength(1);
    expect(names[0]?.test("Refresh status")).toBe(true);
    expect(names[0]?.test("Refreshing")).toBe(true);
    expect(names[0]?.test("Refresh")).toBe(false);
    expect(rec.calls.some((call) => isRefreshPath(call.path) && call.method === "click")).toBe(true);
  });

  it("reads the idle and working labels", async () => {
    const idle = new Recorder();
    await expect(idle.driver().exportsSettingsRefreshLabel()).resolves.toBe("Refresh status");

    const busy = new Recorder();
    busy.refreshLabel = "Refreshing";
    await expect(busy.driver().exportsSettingsRefreshLabel()).resolves.toBe("Refreshing");
  });

  it("reads null without the control and without text", async () => {
    const missing = new Recorder();
    missing.refreshLabel = null;
    await expect(missing.driver().exportsSettingsRefreshLabel()).resolves.toBeNull();

    const blank = new Recorder();
    blank.refreshLabel = "  ";
    await expect(blank.driver().exportsSettingsRefreshLabel()).resolves.toBeNull();
  });
});

describe("exportsSettingsPagination", () => {
  it("reads the end flags from the exact Prev and Next buttons", async () => {
    const rec = new Recorder();
    rec.prevDisabled = true;
    rec.nextDisabled = false;
    await expect(rec.driver().exportsSettingsPagination()).resolves.toEqual({
      visible: true,
      prevDisabled: true,
      nextDisabled: false,
    });
    expect(rec.calls.some((call) => call.path === PREV_PATH && call.method === "count")).toBe(true);
    expect(rec.calls.some((call) => call.path === NEXT_PATH && call.method === "count")).toBe(true);
  });

  it("reports hidden when either control is missing", async () => {
    const noPrev = new Recorder();
    noPrev.prevPresent = false;
    await expect(noPrev.driver().exportsSettingsPagination()).resolves.toEqual({
      visible: false,
      prevDisabled: true,
      nextDisabled: true,
    });

    const noNext = new Recorder();
    noNext.nextPresent = false;
    await expect(noNext.driver().exportsSettingsPagination()).resolves.toEqual({
      visible: false,
      prevDisabled: true,
      nextDisabled: true,
    });
  });

  it("steps through the exact Prev and Next buttons", async () => {
    const prev = new Recorder();
    await prev.driver().exportsSettingsGoPrev();
    expect(prev.calls.some((call) => call.path === PREV_PATH && call.method === "click")).toBe(true);

    const next = new Recorder();
    await next.driver().exportsSettingsGoNext();
    expect(next.calls.some((call) => call.path === NEXT_PATH && call.method === "click")).toBe(true);
  });
});

describe("exportsSettingsSkeletonVisible", () => {
  it("tracks the loading status", async () => {
    const shown = new Recorder();
    shown.skeletonPresent = true;
    await expect(shown.driver().exportsSettingsSkeletonVisible()).resolves.toBe(true);

    const hidden = new Recorder();
    await expect(hidden.driver().exportsSettingsSkeletonVisible()).resolves.toBe(false);
    expect(hidden.calls.some((call) => call.path === STATUS_PATH && call.method === "count")).toBe(true);
  });
});

describe("exportsSettingsEmptyFacts", () => {
  it("reads the title and explanation spans", async () => {
    const rec = new Recorder();
    rec.emptyParts = [
      "No exports yet",
      "You don't have any export records right now. Once you export data, all records will appear here.",
    ];
    await expect(rec.driver().exportsSettingsEmptyFacts()).resolves.toEqual({
      title: "No exports yet",
      description: "You don't have any export records right now. Once you export data, all records will appear here.",
    });
    expect(rec.calls.some((call) => call.path === HEADING_PATH && call.method === "count")).toBe(true);
  });

  it("reads null without the heading and without spans", async () => {
    const missing = new Recorder();
    await expect(missing.driver().exportsSettingsEmptyFacts()).resolves.toBeNull();

    const bare = new Recorder();
    bare.emptyParts = [];
    await expect(bare.driver().exportsSettingsEmptyFacts()).resolves.toBeNull();
  });

  it("falls back to an empty explanation with the title alone", async () => {
    const rec = new Recorder();
    rec.emptyParts = ["No exports yet"];
    await expect(rec.driver().exportsSettingsEmptyFacts()).resolves.toEqual({
      title: "No exports yet",
      description: "",
    });
  });
});

describe("exports history request watching", () => {
  it("counts only GET reads of the history endpoint", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsWatchHistoryRequests();
    rec.requests((emit) => {
      emit({ method: () => "GET", url: () => "https://app.test/api/workspaces/acme/export-issues?per_page=10" });
      emit({ method: () => "POST", url: () => "https://app.test/api/workspaces/acme/export-issues/" });
      emit({ method: () => "GET", url: () => "https://app.test/api/workspaces/acme/projects" });
      emit({ method: () => "GET", url: () => "https://app.test/api/workspaces/acme/export-issues/?cursor=1" });
    });

    await expect(driver.exportsSettingsHistoryRequestCount()).resolves.toBe(2);
    expect(rec.listeners.filter((listener) => listener.event === "request")).toHaveLength(1);
  });

  it("resets the count and keeps one listener when re-armed", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsWatchHistoryRequests();
    rec.requests((emit) => {
      emit({ method: () => "GET", url: () => "https://app.test/api/workspaces/acme/export-issues?per_page=10" });
    });
    await expect(driver.exportsSettingsHistoryRequestCount()).resolves.toBe(1);

    await driver.exportsSettingsWatchHistoryRequests();
    await expect(driver.exportsSettingsHistoryRequestCount()).resolves.toBe(0);
    expect(rec.listeners.filter((listener) => listener.event === "request")).toHaveLength(1);
  });
});

describe("exports history stalling", () => {
  function stubRoute(method: string): { seenFallback: () => boolean; route: never } {
    let fallback = false;
    return {
      seenFallback: () => fallback,
      route: {
        request: () => ({ method: () => method }),
        fallback: async () => {
          fallback = true;
        },
      } as never,
    };
  }

  it("holds GET reads while stalled and releases them in order", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsStallHistory();
    expect(rec.routes).toHaveLength(1);
    expect(rec.routes[0]?.pattern).toBe("**/api/workspaces/*/export-issues**");

    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;
    const first = stubRoute("GET");
    const pending = handler(first.route);
    await Promise.resolve();
    expect(first.seenFallback()).toBe(false);

    await driver.exportsSettingsReleaseHistory();
    await pending;
    expect(first.seenFallback()).toBe(true);
  });

  it("passes reads through once released and ignores other methods", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsStallHistory();
    await driver.exportsSettingsReleaseHistory();
    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;

    const read = stubRoute("GET");
    await handler(read.route);
    expect(read.seenFallback()).toBe(true);

    const write = stubRoute("POST");
    await driver.exportsSettingsStallHistory();
    await handler(write.route);
    expect(write.seenFallback()).toBe(true);
    expect(rec.routes).toHaveLength(1);
  });
});

describe("exportsSettingsDownloadPopupUrl", () => {
  it("clicks the row's download link and returns the popup url", async () => {
    const rec = new Recorder();
    rec.popupUrl = "https://files.test/export.csv";
    const url = await rec.driver().exportsSettingsDownloadPopupUrl(0);

    expect(url).toBe("https://files.test/export.csv");
    expect(
      rec.calls.some(
        (call) =>
          call.method === "click" &&
          call.path === `${TABLE_PATH}.locator("tbody tr").nth(0).getByRole("cell").last().getByRole("link")`
      )
    ).toBe(true);
  });

  it("still returns the url when the popup is already closed", async () => {
    const rec = new Recorder();
    rec.popupCloseFails = true;
    await expect(rec.driver().exportsSettingsDownloadPopupUrl(1)).resolves.toBe("https://files.test/export.csv");
  });
});
