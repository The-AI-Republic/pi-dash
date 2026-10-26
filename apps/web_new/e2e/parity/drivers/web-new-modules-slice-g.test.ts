// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the modules slice-G driver hooks (NEWFRONT-799,
// MOD-026..028): entering the project's archived-modules tab, the archived
// list's names, search, filters, chips and empties, the archived and live
// peeks, the shared row menu's live/archived choose paths, and the archive
// confirmation's read, confirm, cancel and next-write failure. A
// Playwright-shaped fake answers from a chain-keyed table, so each case
// pins the selector or gesture the hook reads — no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  text?: string;
  texts?: string[];
  attrs?: Record<string, string | null>;
  value?: string;
  evaluated?: unknown;
  /** Markup the element carries, so an evaluateAll callback reads real DOM. */
  html?: string;
}

type Table = Record<string, Answer>;

const MODULE_ID = "123e4567-e89b-12d3-a456-426614174201";
const LIST_ROW = "page testid(modules-list) testid(modules-row) filter()";

class FakePage {
  readonly clicks: string[] = [];
  readonly fills: { id: string; text: string }[] = [];
  readonly gotos: string[] = [];
  readonly keys: string[] = [];
  readonly routes: { times: number | undefined }[] = [];
  readonly waits: string[] = [];
  readonly responses: string[] = [];
  path = "/acme/projects/p1/archives/modules";
  search = "";
  /** Set by a case whose press changes what another selector answers. */
  onClick: (id: string) => void = () => undefined;

  constructor(private readonly table: Table = {}) {}

  set(key: string, answer: Answer): void {
    this.table[key] = answer;
  }

  private answer(id: string): Answer & { count: number; visible: boolean } {
    // Keys name the tail of a locator chain, so the longest matching tail
    // answers and a case reads as the element it describes.
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
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      or: () => self.locator(id),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
        self.onClick(id);
      },
      hover: async () => undefined,
      scrollIntoViewIfNeeded: async () => undefined,
      fill: async (text: string) => {
        self.fills.push({ id, text });
      },
      waitFor: async () => {
        self.waits.push(id);
      },
      evaluate: async (callback: (element: HTMLElement) => unknown) => {
        const answer = self.answer(id);
        if ("evaluated" in answer) return answer.evaluated;
        return callback(self.element(answer));
      },
      evaluateAll: async (callback: (elements: HTMLElement[]) => unknown) => {
        const answer = self.answer(id);
        if ("evaluated" in answer) return answer.evaluated;
        return callback([self.element(answer)]);
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
      url: () => `https://app.test${self.path}${self.search}`,
      keyboard: {
        press: async (key: string) => {
          self.keys.push(key);
        },
      },
      goto: async (url: string) => {
        self.gotos.push(url);
      },
      waitForFunction: async () => null,
      waitForResponse: async (predicate: unknown) => {
        self.responses.push(String(predicate).slice(0, 24));
        return null;
      },
      route: async (_predicate: unknown, _handler: unknown, options?: { times?: number }) => {
        self.routes.push({ times: options?.times });
      },
      waitForTimeout: async () => undefined,
      context: () => ({
        grantPermissions: async () => undefined,
        waitForEvent: async () => ({ waitForLoadState: async () => undefined }),
      }),
    } as unknown as Page;
  }
}

/** A settled archived list holding one row called `name`. */
function archivedList(name: string, extra: Table = {}): FakePage {
  return new FakePage({
    "page testid(modules-list)": { visible: true },
    "page testid(modules-list) testid(modules-row-name)": { texts: [` ${name} `] },
    [LIST_ROW]: { visible: true },
    [`${LIST_ROW} testid(modules-row-link)`]: { attrs: { href: `/acme/projects/p1/modules/${MODULE_ID}` } },
    "page testid(modules-peek)": { visible: true },
    "page testid(modules-peek) role(heading,)": { text: name },
    ...extra,
  });
}

describe("entering the archived-modules tab", () => {
  it("goes through the live list, the project archives entry and the strip", async () => {
    const fake = archivedList("Archived alpha");
    fake.path = "/acme/projects/p1/modules";
    await new WebNewDriver(fake.asPage()).archivesOpenModulesTab("acme", "p1");

    expect(fake.clicks.some((click) => click.includes("testid(project-row-menu)"))).toBe(true);
    expect(fake.clicks.some((click) => click.includes("role(menuitem,Archives)"))).toBe(true);
    expect(
      fake.clicks.some((click) =>
        click.includes('css([data-testid="archives-tab-strip"] a[href*="/archives/modules"])')
      )
    ).toBe(true);
  });

  it("loads the live list only when the address is elsewhere", async () => {
    const fake = archivedList("Delivery");
    fake.path = "/acme/projects/p1/archives/modules";
    await new WebNewDriver(fake.asPage()).archivesOpenLiveModules("acme", "p1");
    expect(fake.gotos).toEqual(["/acme/projects/p1/modules"]);

    fake.path = "/acme/projects/p1/modules";
    await new WebNewDriver(fake.asPage()).archivesOpenLiveModules("acme", "p1");
    expect(fake.gotos).toHaveLength(1);
  });
});

describe("the archived list readers", () => {
  it("waits for a named row", async () => {
    const fake = archivedList("Archived alpha");
    await new WebNewDriver(fake.asPage()).archivesAwaitModuleRow("Archived alpha");
    expect(fake.waits).toContain("page testid(modules-list) testid(modules-row) filter()");
  });

  it("reads the row names in display order and trims them", async () => {
    const fake = archivedList("Archived alpha");
    await expect(new WebNewDriver(fake.asPage()).archivesModuleRowNames()).resolves.toEqual(["Archived alpha"]);
  });

  it("reports no rows when the list never rendered", async () => {
    const fake = new FakePage({ "page testid(modules-list)": { count: 0 } });
    await expect(new WebNewDriver(fake.asPage()).archivesModuleRowNames()).resolves.toEqual([]);
  });

  it("names the empty state and refuses an unknown one", async () => {
    const zero = new FakePage({
      "page testid(archives-content)": { visible: true },
      "page testid(modules-list)": { count: 0 },
      "page testid(modules-list) testid(modules-row-name)": { texts: [] },
      "page testid(archived-modules-empty)": { attrs: { "data-empty-kind": "zero" } },
    });
    await expect(new WebNewDriver(zero.asPage()).archivesModulesEmptyKind()).resolves.toBe("zero");

    const rows = archivedList("Archived alpha");
    await expect(new WebNewDriver(rows.asPage()).archivesModulesEmptyKind()).resolves.toBe("rows");

    const unknown = new FakePage({
      "page testid(modules-list)": { count: 0 },
      "page testid(archived-modules-empty)": { attrs: { "data-empty-kind": "surprise" } },
    });
    await expect(new WebNewDriver(unknown.asPage()).archivesModulesEmptyKind()).rejects.toThrow(
      /unknown empty kind "surprise"/
    );

    const nothing = new FakePage({
      "page testid(modules-list)": { count: 0 },
      "page testid(archived-modules-empty)": { count: 0 },
    });
    await expect(new WebNewDriver(nothing.asPage()).archivesModulesEmptyKind()).rejects.toThrow(
      /neither rows nor a known empty state/
    );
  });
});

describe("the archived header controls", () => {
  it("expands the name search and focuses its field", async () => {
    const fake = new FakePage({ "page role(button,Search modules)": { visible: true } });
    await new WebNewDriver(fake.asPage()).archivesOpenModuleSearch();
    expect(fake.clicks.some((click) => click.includes("role(button,Search modules)"))).toBe(true);
  });

  it("types into the shared search field", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).archivesTypeModuleSearch("alpha");
    expect(fake.fills).toEqual([{ id: "page testid(modules-search-input)", text: "alpha" }]);
  });

  it("clears the search through the field's own control", async () => {
    const fake = new FakePage({
      "page testid(modules-search-input)": { value: "" },
      "page testid(modules-search-field) role(button,Clear search)": {},
    });
    await new WebNewDriver(fake.asPage()).archivesClearModuleSearch();
    expect(fake.clicks.some((click) => click.includes("role(button,Clear search)"))).toBe(true);
  });

  it("opens the filters once and reads the groups", async () => {
    const fake = new FakePage({
      "page testid(modules-filter-panel)": { visible: true },
      "page testid(modules-filter-panel) testid(modules-filter-group)": {
        html:
          '<div data-testid="modules-filter-group" data-filter-group="Lead"></div>' +
          '<div data-testid="modules-filter-group" data-filter-group="Members"></div>' +
          '<div data-testid="modules-filter-group" data-filter-group=""></div>',
      },
    });
    // evaluateAll receives one element, so answer with the full mapping.
    fake.set("page testid(modules-filter-panel) testid(modules-filter-group)", {
      evaluated: ["Lead", "Members", ""],
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.archivesOpenModuleFilters();
    await expect(driver.archivesModuleFilterGroups()).resolves.toEqual(["Lead", "Members"]);
    // The panel is already open: no second trigger click.
    expect(fake.clicks.filter((click) => click.includes("modules-filter-trigger"))).toHaveLength(0);
  });

  it("opens the panel when it is closed", async () => {
    const fake = new FakePage({
      "page testid(modules-filter-panel)": { visible: false },
    });
    await new WebNewDriver(fake.asPage()).archivesOpenModuleFilters();
    expect(fake.clicks.some((click) => click.includes("testid(modules-filter-trigger)"))).toBe(true);
  });

  it("toggles a lead option inside the Lead group", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).archivesToggleLeadFilter("Ada Lovelace");
    expect(
      fake.clicks.some((click) => click.includes('[data-testid="modules-filter-group"][data-filter-group="Lead"]'))
    ).toBe(true);
    expect(fake.clicks.some((click) => click.includes('[data-filter-option="Ada Lovelace"]'))).toBe(true);
  });

  it("reads the chips and removes one by its key", async () => {
    const fake = new FakePage({
      "page testid(modules-filter-chips) testid(modules-filter-chip)": {
        html: '<span data-testid="modules-filter-chip" data-chip-key="lead:u1"> Lead: Ada </span>',
      },
    });
    const driver = new WebNewDriver(fake.asPage());
    await expect(driver.archivesModuleChips()).resolves.toEqual([{ key: "lead:u1", text: "Lead: Ada" }]);
    await driver.archivesRemoveModuleChip("lead:u1");
    expect(
      fake.clicks.some((click) => click.includes('[data-testid="modules-filter-chip"][data-chip-key="lead:u1"]'))
    ).toBe(true);
  });

  it("clears every chip, and does nothing when none are shown", async () => {
    const fake = new FakePage({
      "page testid(modules-filter-chips)": { visible: true },
    });
    await new WebNewDriver(fake.asPage()).archivesClearModuleFilters();
    expect(fake.clicks.some((click) => click.includes("testid(modules-filter-clear-all)"))).toBe(true);

    const none = new FakePage({ "page testid(modules-filter-chips)": { count: 0 } });
    await new WebNewDriver(none.asPage()).archivesClearModuleFilters();
    expect(none.clicks).toHaveLength(0);
  });

  it("closes the panel with Escape", async () => {
    const fake = new FakePage({ "page testid(modules-filter-panel)": { visible: true } });
    await new WebNewDriver(fake.asPage()).archivesCloseModuleFilters();
    expect(fake.keys).toEqual(["Escape"]);
  });
});

describe("the archived peek", () => {
  it("opens from the row body and closes from the panel", async () => {
    const fake = archivedList("Archived alpha");
    fake.search = `?peekModule=${MODULE_ID}`;
    const driver = new WebNewDriver(fake.asPage());
    await driver.archivesOpenModulePeek("Archived alpha");
    expect(fake.clicks.some((click) => click.includes("testid(modules-row-progress)"))).toBe(true);

    await driver.archivesCloseModulePeek();
    expect(fake.clicks.some((click) => click.includes("role(button,Close preview)"))).toBe(true);
  });

  it("reports no peek when the address carries none", async () => {
    const fake = archivedList("Archived alpha");
    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekName()).resolves.toBeNull();
  });

  it("opens the live peek from the row's own info control", async () => {
    const fake = archivedList("Delivery", {
      [`${LIST_ROW} testid(modules-row-peek)`]: {},
    });
    fake.path = "/acme/projects/p1/modules";
    fake.search = `?peekModule=${MODULE_ID}`;
    await new WebNewDriver(fake.asPage()).archivesOpenLiveModulePeek("Delivery");
    expect(fake.clicks.some((click) => click.includes("testid(modules-row-peek)"))).toBe(true);
    // The gesture, not a fabricated address: nothing navigates by goto.
    expect(fake.gotos).toEqual([]);
  });
});

describe("the shared row menu", () => {
  it("opens the menu and chooses an entry on both lists", async () => {
    const live = archivedList("Delivery", {
      'page css([role="menuitem"]:visible)': { count: 1, text: "Archive" },
      'page css([role="menuitem"]:visible) css([data-menu-title])': { count: 0 },
    });
    const driver = new WebNewDriver(live.asPage());
    await driver.archivesChooseLiveModuleMenuEntry("Delivery", "Archive");
    // The menu was already open, so the trigger is left alone and only the
    // chosen entry is pressed.
    expect(live.clicks).toEqual(['page css([role="menuitem"]:visible)']);

    const archived = archivedList("Archived alpha", {
      'page css([role="menuitem"]:visible)': { count: 1, text: "Restore" },
      'page css([role="menuitem"]:visible) css([data-menu-title])': { count: 0 },
    });
    await new WebNewDriver(archived.asPage()).archivesChooseArchivedModuleMenuEntry("Archived alpha", "Restore");
    expect(archived.clicks).toEqual(['page css([role="menuitem"]:visible)']);
  });

  it("opens a closed menu through the row's desktop trigger", async () => {
    const fake = archivedList("Delivery", {
      'page css([role="menuitem"]:visible)': { count: 1, text: "Archive" },
      'page css([role="menuitem"]:visible) css([data-menu-title])': { count: 0 },
    });
    // Closed at first, open once the trigger is pressed.
    fake.set('page css([role="menuitem"]:visible)', { count: 0 });
    // The menu scopes by the page-level row, not the live list container.
    const trigger = 'page testid(modules-row) filter() css(div[data-main-menu="true"]:visible button)';
    fake.onClick = (id) => {
      if (id === trigger) fake.set('page css([role="menuitem"]:visible)', { count: 1, text: "Archive" });
    };
    await new WebNewDriver(fake.asPage()).archivesChooseLiveModuleMenuEntry("Delivery", "Archive");
    expect(fake.clicks).toContain(trigger);
    expect(fake.clicks).toContain('page css([role="menuitem"]:visible)');
  });

  it("reuses an already open menu and refuses a title it cannot find", async () => {
    const fake = archivedList("Delivery", {
      'page css([role="menuitem"]:visible)': { count: 1, text: "Open in new tab" },
      'page css([role="menuitem"]:visible) css([data-menu-title])': { count: 0 },
    });
    const driver = new WebNewDriver(fake.asPage());
    await expect(driver.archivesChooseLiveModuleMenuEntry("Delivery", "Archive")).rejects.toThrow(
      /modules menu has no entry titled "Archive"/
    );
  });
});

describe("the archive confirmation", () => {
  // Per parity/specs/archives.md §5.7 the dialog trio is NEWFRONT-700's
  // family and this slice reuses it read-only; here we only check the
  // module-owned failure hook and that the trio reads the dialog the
  // shared implementation shows.
  it("fails exactly the next archive write", async () => {
    const fake = new FakePage({});
    await new WebNewDriver(fake.asPage()).archivesFailNextModuleWrite();
    expect(fake.routes).toEqual([{ times: 1 }]);
  });

  it("reads the open confirmation through the shared archives trio", async () => {
    const DIALOG = "page css([role=\"dialog\"])";
    const fake = new FakePage({
      [DIALOG]: { count: 1 },
      [`${DIALOG} role(heading,)`]: { text: " Archive “Atlas” " },
      [`${DIALOG} css(p)`]: { text: " “Atlas” can be restored. " },
    });
    await expect(new WebNewDriver(fake.asPage()).archivesArchiveDialog()).resolves.toEqual({
      title: "Archive “Atlas”",
      body: "“Atlas” can be restored.",
      confirmLabel: "",
    });
  });
});
