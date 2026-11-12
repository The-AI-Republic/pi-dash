// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the modules slice-D driver hooks (NEWFRONT-796,
// MOD-012/019/020): the row's progress marker, detail href and peek
// trigger, the archived row's body press, the card's name tooltip, status
// slot, count, lead, segmented bar and accessory slots, the card body
// press, and the address-driven peek panel. A Playwright-shaped fake
// answers from a chain-keyed table, so every case pins the selector, the
// attribute or the gesture the hook reads — no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  text?: string;
  attrs?: Record<string, string | null>;
  evaluated?: unknown;
  /** Markup the element carries, so an evaluate callback can read real DOM. */
  html?: string;
}

type ClickHook = (id: string) => void;

type Table = Record<string, Answer>;

class FakePage {
  readonly clicks: string[] = [];
  readonly hovers: string[] = [];
  readonly keys: string[] = [];
  readonly waits: string[] = [];
  path = "/acme/projects/p1/modules";
  /** Set by a case whose press changes what another selector answers. */
  onClick: ClickHook = () => undefined;
  private readonly table: Table;

  constructor(table: Table = {}) {
    this.table = table;
  }

  /** Replace what a selector answers from now on. */
  set(key: string, answer: Answer): void {
    this.table[key] = answer;
  }

  private answer(id: string): Answer & { count: number; visible: boolean } {
    // Keys name the tail of a locator chain, so a case reads as the element
    // it describes ("testid(modules-row-progress)") and still answers when
    // the driver wraps it in another `.filter()`.
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.endsWith(` ${key}`) || id.endsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    return { count: 1, visible: true, ...(candidates[0]?.[1] ?? {}) };
  }

  private element(answer: Answer): HTMLElement {
    const host = document.createElement("div");
    host.innerHTML = answer.html ?? "";
    return (host.firstElementChild as HTMLElement | null) ?? host;
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: () => self.locator(id),
      filter: (options: { has?: unknown; hasText?: string | RegExp }) =>
        self.locator(`${id} filter(${String(options.hasText ?? "")})`),
      or: () => self.locator(id),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      getByText: (text: string) => self.locator(`${id} text(${text})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      innerText: async () => self.answer(id).text ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
        self.onClick(id);
      },
      hover: async () => {
        self.hovers.push(id);
      },
      waitFor: async () => {
        self.waits.push(id);
      },
      evaluate: async (callback: (element: HTMLElement) => unknown) => {
        const answer = self.answer(id);
        if ("evaluated" in answer) return answer.evaluated;
        return callback(self.element(answer));
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
      url: () => `https://app.test${self.path}`,
      keyboard: {
        press: async (key: string) => {
          self.keys.push(key);
        },
      },
      waitForTimeout: async () => undefined,
      waitForFunction: async () => {
        self.waits.push("waitForFunction");
        return null;
      },
      evaluate: async (callback: () => unknown) => callback(),
      context: () => ({ addCookies: async () => undefined }),
      goto: async () => undefined,
      waitForLoadState: async () => undefined,
    } as unknown as Page;
  }
}

// The driver scopes a wrapper by its own name span, so the chain ends in
// a bare `filter()`; keys name that whole tail and a leaf appends its own
// handle. The fake answers the longest matching tail.
const ROW = "testid(modules-list) testid(modules-row) filter()";
const CARD = "testid(modules-list) testid(modules-board-card) filter()";

function rowPage(answer: Answer): FakePage {
  return new FakePage({
    [ROW]: { html: '<li><span data-testid="modules-row">Delivery</span></li>' },
    [`${ROW} testid(modules-row-progress)`]: answer,
  });
}

describe("modulesListRowProgress", () => {
  it("reads a percentage marker", async () => {
    const fake = rowPage({ attrs: { "data-progress-kind": "percent", "data-progress-percent": "25" } });
    await expect(new WebNewDriver(fake.asPage()).modulesListRowProgress("Delivery")).resolves.toEqual({
      kind: "percent",
      percent: 25,
    });
  });

  it("reads the check and alert markers", async () => {
    await expect(
      new WebNewDriver(rowPage({ attrs: { "data-progress-kind": "check" } }).asPage()).modulesListRowProgress(
        "Delivery"
      )
    ).resolves.toEqual({ kind: "check" });
    await expect(
      new WebNewDriver(rowPage({ attrs: { "data-progress-kind": "alert" } }).asPage()).modulesListRowProgress(
        "Delivery"
      )
    ).resolves.toEqual({ kind: "alert" });
  });

  it("refuses a row with no readable marker", async () => {
    await expect(
      new WebNewDriver(
        rowPage({ attrs: { "data-progress-kind": "percent", "data-progress-percent": "x" } }).asPage()
      ).modulesListRowProgress("Delivery")
    ).rejects.toThrow(/no readable progress marker/);
  });
});

describe("modulesListRowDetailHref", () => {
  it("reads the name anchor's href", async () => {
    const fake = new FakePage({
      [ROW]: { html: '<li><a data-testid="modules-row-link" href="/acme/projects/p1/modules/m1">Delivery</a></li>' },
      [`${ROW} testid(modules-row-link)`]: { attrs: { href: "/acme/projects/p1/modules/m1" } },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListRowDetailHref("Delivery")).resolves.toBe(
      "/acme/projects/p1/modules/m1"
    );
  });

  it("refuses a row that never rendered", async () => {
    const fake = new FakePage({
      [ROW]: {},
      [`${ROW} testid(modules-row-link)`]: { attrs: { href: null } },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListRowDetailHref("Delivery")).rejects.toThrow(
      /list row never rendered "Delivery"/
    );
  });
});

describe("the peek panel readers", () => {
  it("reports no peek when the panel is absent", async () => {
    const fake = new FakePage({ "page testid(modules-peek)": { count: 0, visible: false } });
    const driver = new WebNewDriver(fake.asPage());
    await expect(driver.modulesListPeekName()).resolves.toBeNull();
    await expect(driver.modulesPeekVisible()).resolves.toBe(false);
  });

  it("reads the panel's heading as the peeked name", async () => {
    const fake = new FakePage({
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: " Delivery " },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListPeekName()).resolves.toBe("Delivery");
    await expect(new WebNewDriver(fake.asPage()).modulesPeekName()).resolves.toBe("Delivery");
  });

  it("returns null when the open panel has no heading yet", async () => {
    const fake = new FakePage({
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { count: 0 },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListPeekName()).resolves.toBeNull();
  });

  it("closes through the chrome's close control and waits for the panel to go", async () => {
    const fake = new FakePage({
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(button,Close preview)": {},
    });
    // Closing through the chrome's control unmounts the panel, so the
    // presence the driver polls afterwards goes away.
    fake.onClick = (id) => {
      if (id.includes("role(button,Close preview)"))
        fake.set("page testid(modules-peek)", { count: 0, visible: false });
    };
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesListPeekClose();
    expect(fake.clicks.some((click) => click.includes("role(button,Close preview)"))).toBe(true);
  });
});

describe("the row peek trigger", () => {
  it("opens the peek from a row's info control", async () => {
    const fake = new FakePage({
      [ROW]: { visible: true },
      [`${ROW} testid(modules-row-peek)`]: {},
      [CARD]: { count: 0, visible: false },
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: "Delivery" },
    });
    await new WebNewDriver(fake.asPage()).modulesListRowOpenPeek("Delivery");
    expect(fake.hovers).toEqual([`page ${ROW}`]);
    expect(fake.clicks.some((click) => click.includes("testid(modules-row-peek)"))).toBe(true);
  });

  it("opens the peek from the short toggle hook and clicks the row itself", async () => {
    const fake = new FakePage({
      [ROW]: { visible: true },
      [`${ROW} testid(modules-row-peek)`]: {},
      [CARD]: { count: 0, visible: false },
      [`${ROW} testid(modules-row-link)`]: {},
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: "Delivery" },
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesTogglePeek("Delivery");
    await driver.modulesClickRow("Delivery");
    expect(fake.clicks.some((click) => click.includes("testid(modules-row-link)"))).toBe(true);
  });
});

describe("modulesListArchivedRowClickOpensPeek", () => {
  it("presses the row body and stays on the archives path", async () => {
    const fake = new FakePage({
      [ROW]: { visible: true },
      [`${ROW} testid(modules-row-progress)`]: {},
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: "Gone" },
    });
    fake.path = "/acme/projects/p1/archives/modules";
    await new WebNewDriver(fake.asPage()).modulesListArchivedRowClickOpensPeek("Gone");
    expect(fake.clicks.some((click) => click.includes("testid(modules-row-progress)"))).toBe(true);
  });

  it("ignores the router's trailing-slash normalization (MOD-012)", async () => {
    const fake = new FakePage({
      [ROW]: { visible: true },
      [`${ROW} testid(modules-row-progress)`]: {},
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: "Gone" },
    });
    fake.path = "/acme/projects/p1/archives/modules";
    const page = fake.asPage();
    let reads = 0;
    Object.defineProperty(page, "url", {
      value: () => {
        reads += 1;
        return reads === 1
          ? "https://app.test/acme/projects/p1/archives/modules"
          : "https://app.test/acme/projects/p1/archives/modules/";
      },
    });
    await expect(new WebNewDriver(page).modulesListArchivedRowClickOpensPeek("Gone")).resolves.toBe(undefined);
  });
});

describe("modulesListRowMenuVisible", () => {
  it("reads the narrow-screen menu slot's visibility", async () => {
    const visible = new FakePage({ [ROW]: {}, [`${ROW} testid(modules-row-menu)`]: { visible: true } });
    await expect(new WebNewDriver(visible.asPage()).modulesListRowMenuVisible("Delivery")).resolves.toBe(true);
    const hidden = new FakePage({ [ROW]: {}, [`${ROW} testid(modules-row-menu)`]: { visible: false } });
    await expect(new WebNewDriver(hidden.asPage()).modulesListRowMenuVisible("Delivery")).resolves.toBe(false);
    const absent = new FakePage({ [ROW]: {}, [`${ROW} testid(modules-row-menu)`]: { count: 0 } });
    await expect(new WebNewDriver(absent.asPage()).modulesListRowMenuVisible("Delivery")).resolves.toBe(false);
  });
});

function cardPage(answers: Table): FakePage {
  return new FakePage({
    [CARD]: { visible: true },
    ...answers,
  });
}

describe("the card readers", () => {
  it("reads whether the name overflows", async () => {
    const truncated = cardPage({
      [`${CARD} testid(modules-board-card-name)`]: {
        evaluated: true,
        html: '<span data-testid="modules-board-card-name">Delivery</span>',
      },
    });
    await expect(new WebNewDriver(truncated.asPage()).modulesListCardNameTruncated("Delivery")).resolves.toBe(true);
  });

  it("prefers the rendered tooltip and falls back to the title", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-board-card-name)`]: { attrs: { title: "Delivery" } },
      "page role(tooltip,/^Delivery$/)": { text: "Delivery" },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardNameTooltip("Delivery")).resolves.toBe("Delivery");
    expect(fake.hovers.some((hover) => hover.includes("modules-board-card-name"))).toBe(true);
  });

  it("falls back to the span's full-name title when no popup paints", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-board-card-name)`]: { attrs: { title: "Delivery" } },
      "page role(tooltip,/^Delivery$/)": { count: 0 },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardNameTooltip("Delivery")).resolves.toBe("Delivery");
  });

  it("reads the status slot's label", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-card-status)`]: { text: "In Progress" },
      [`${CARD} testid(modules-card-status) role(button,)`]: { count: 0 },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardStatusText("Delivery")).resolves.toBe("In Progress");
  });

  it("prefers the pill button inside the status slot", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-card-status)`]: { text: "Pill row" },
      [`${CARD} testid(modules-card-status) role(button,)`]: { text: " Planned " },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardStatusText("Delivery")).resolves.toBe("Planned");
  });

  it("reads the work-item count, the lead kind and the progress segments", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-board-card-count)`]: { text: " 1/2 work items " },
      [`${CARD} testid(modules-card-lead)`]: { attrs: { "data-lead-kind": "none" } },
      [`${CARD} testid(modules-card-progress)`]: {
        html:
          '<div data-testid="modules-card-progress">' +
          '<div data-testid="modules-card-progress-segment" style="width: 50%; background-color: rgb(22, 163, 74);"></div>' +
          '<div data-testid="modules-card-progress-segment" style="width: 25%; background-color: rgb(245, 158, 11);"></div>' +
          "</div>",
      },
    });
    const driver = new WebNewDriver(fake.asPage());
    await expect(driver.modulesListCardIssueCount("Delivery")).resolves.toBe("1/2 work items");
    await expect(driver.modulesListCardLeadKind("Delivery")).resolves.toBe("none");
    await expect(driver.modulesListCardProgressSegments("Delivery")).resolves.toEqual({
      widths: [50, 25],
      colors: ["rgb(22, 163, 74)", "rgb(245, 158, 11)"],
    });
  });

  it("reads an avatar lead", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-card-lead)`]: { attrs: { "data-lead-kind": "avatar" } },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardLeadKind("Delivery")).resolves.toBe("avatar");
  });

  it("refuses a card whose lead slot never rendered", async () => {
    const fake = cardPage({ [`${CARD} testid(modules-card-lead)`]: { attrs: {} } });
    await expect(new WebNewDriver(fake.asPage()).modulesListCardLeadKind("Delivery")).rejects.toThrow(
      /card lead slot never rendered/
    );
  });

  it("reads the date, favorite and quick-menu slots", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-card-dates)`]: { visible: true },
      [`${CARD} testid(modules-card-favorite)`]: { visible: true },
      [`${CARD} testid(modules-card-menu)`]: { visible: false },
    });
    const driver = new WebNewDriver(fake.asPage());
    await expect(driver.modulesListCardDatesVisible("Delivery")).resolves.toBe(true);
    await expect(driver.modulesListCardFavoriteVisible("Delivery")).resolves.toBe(true);
    await expect(driver.modulesListCardQuickMenuVisible("Delivery")).resolves.toBe(false);
  });
});

describe("the card gestures", () => {
  it("opens the peek from the card's info control", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-board-card-peek)`]: {},
      "page testid(modules-peek)": { visible: true },
      "page testid(modules-peek) role(heading,)": { text: "Delivery" },
    });
    await new WebNewDriver(fake.asPage()).modulesListCardOpenPeek("Delivery");
    expect(fake.clicks.some((click) => click.includes("testid(modules-board-card-peek)"))).toBe(true);
  });

  it("presses the count line as the card body and waits for the detail address", async () => {
    const fake = cardPage({ [`${CARD} testid(modules-board-card-count)`]: {} });
    await new WebNewDriver(fake.asPage()).modulesListCardClickBody("Delivery");
    expect(fake.clicks.some((click) => click.includes("testid(modules-board-card-count)"))).toBe(true);
    expect(fake.waits).toContain("waitForFunction");
  });

  it("presses each control slot and never moves the address", async () => {
    for (const control of ["status", "dates", "favorite", "menu"] as const) {
      const fake = cardPage({
        [`${CARD} testid(modules-card-${control})`]: {},
        [`${CARD} testid(modules-card-${control}) role(button,)`]: {},
      });
      await new WebNewDriver(fake.asPage()).modulesListCardClickControl("Delivery", control);
      expect(fake.clicks.some((click) => click.includes(`testid(modules-card-${control})`))).toBe(true);
      // Both dismiss keys fire so an opened popover never leaks into the
      // next step.
      expect(fake.keys.filter((key) => key === "Escape")).toHaveLength(2);
    }
  });

  it("fails when a control moved the address", async () => {
    const fake = cardPage({
      [`${CARD} testid(modules-card-dates)`]: {},
      [`${CARD} testid(modules-card-dates) role(button,)`]: {},
    });
    const page = fake.asPage();
    const driver = new WebNewDriver(page);
    vi.spyOn(page, "url")
      .mockReturnValueOnce("https://app.test/acme/projects/p1/modules")
      .mockReturnValue("https://app.test/acme/projects/p1/modules/m1");
    await expect(driver.modulesListCardClickControl("Delivery", "dates")).rejects.toThrow(/navigated to/);
  });
});

describe("modulesBoardColumnCount", () => {
  it("counts the grid's rendered columns", async () => {
    const fake = new FakePage({
      "page testid(modules-list)": { evaluated: "320px 320px" },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesBoardColumnCount()).resolves.toBe(2);
  });

  it("runs the stylesheet read against the grid element", async () => {
    const fake = new FakePage({
      "page testid(modules-list)": { html: '<div data-testid="modules-list"></div>' },
    });
    const page = fake.asPage();
    const driver = new WebNewDriver(page);
    vi.spyOn(window, "getComputedStyle").mockReturnValue({ gridTemplateColumns: "1fr 1fr 1fr" } as CSSStyleDeclaration);
    await expect(driver.modulesBoardColumnCount()).resolves.toBe(3);
    expect(page.url()).toBe("https://app.test/acme/projects/p1/modules");
  });
});
