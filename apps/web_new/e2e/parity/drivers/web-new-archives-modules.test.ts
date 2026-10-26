// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the ten archived-modules driver methods NEWFRONT-702 owns
// (parity/specs/archives.md §5.1, §5.2, §5.6, §5.7): the sort label and
// control, the search box's visibility, text and Escape, the active-filter
// marker, the slow-fetch skeleton entry, the read-only peek reading, and
// the live/archived row-menu entry lists. NEWFRONT-799's twenty-two
// methods are consumed, never re-declared, so the fake page answers only
// what these ten read.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  text?: string;
  value?: string;
  attrs?: Record<string, string | null>;
  disabled?: boolean;
  /** Make the read reject, as a detached element or a chained locator would. */
  rejectText?: boolean;
  nullText?: boolean;
  rejectWaitOnHidden?: boolean;
  rejectWait?: boolean;
  failClick?: boolean;
}

type Table = Record<string, Answer>;

class FakePage {
  readonly clicks: string[] = [];
  readonly keys: string[] = [];
  readonly waits: string[] = [];
  readonly gotos: string[] = [];
  readonly presses: string[] = [];
  readonly unroutes: unknown[] = [];
  readonly initScripts: unknown[] = [];
  /** Make the next unroute / keyboard press refuse, as a closed page would. */
  unrouteFails = false;
  keyboardFails = false;
  readonly routes: { matcher: unknown; handler: (route: unknown) => Promise<void> }[] = [];
  urlString = "/acme/projects/p1/archives/modules";
  /** Set by a case whose press changes what another selector answers. */
  onClick: (id: string) => void = () => undefined;
  skeletonSeen = true;

  constructor(private readonly table: Table = {}) {}

  set(key: string, answer: Answer): void {
    this.table[key] = answer;
  }

  private answer(id: string): Answer & { count: number; visible: boolean } {
    // A case names the tail of the locator chain it describes; the longest
    // matching tail answers, so unrelated selectors fall back to "absent".
    const matches = (key: string): boolean =>
      id === key ||
      id.endsWith(` ${key}`) ||
      id.endsWith(key) ||
      (key.startsWith("page ") && id.endsWith(key.slice(5)));
    const candidates = Object.entries(this.table)
      .filter(([key]) => matches(key))
      .sort(([a], [b]) => b.length - a.length);
    return { count: 0, visible: false, ...(candidates[0]?.[1] ?? {}) };
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: (index: number) => self.locator(`${id} nth(${index})`),
      filter: (options: { hasText?: string; has?: unknown }) =>
        self.locator(`${id} filter(${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled === true,
      innerText: async () => {
        const answer = self.answer(id);
        if (answer.rejectText === true) throw new Error("detached");
        if (answer.nullText === true) return null as unknown as string;
        return answer.text ?? "";
      },
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
        if (self.answer(id).failClick === true) throw new Error("no such control");
        self.onClick(id);
      },
      press: async (key: string) => {
        self.presses.push(`${id}:${key}`);
      },
      hover: async () => undefined,
      scrollIntoViewIfNeeded: async () => undefined,
      fill: async () => undefined,
      waitFor: async (options?: { state?: string }) => {
        const answer = self.answer(id);
        if (answer.rejectWait === true || (answer.rejectWaitOnHidden === true && options?.state === "hidden")) {
          self.waits.push(`${id} rejected`);
          throw new Error("timeout");
        }
        self.waits.push(id);
      },
    };
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    return {
      url: () => `https://app.test${self.urlString}`,
      getByTestId: (testId: string) => self.locator(`page testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp; exact?: boolean }) =>
        self.locator(`page role(${role},${String(options?.name ?? "")})`),
      locator: (selector: string) => self.locator(`page css(${selector})`),
      goto: async (url: string) => {
        self.gotos.push(url);
      },
      addInitScript: async (script: unknown) => {
        self.initScripts.push(script);
      },
      evaluate: async () => self.skeletonSeen,
      route: async (matcher: unknown, handler: (route: unknown) => Promise<void>) => {
        self.routes.push({ matcher, handler });
      },
      unroute: async (matcher: unknown) => {
        self.unroutes.push(matcher);
        if (self.unrouteFails) throw new Error("already gone");
      },
      waitForFunction: async () => null,
      waitForTimeout: async () => undefined,
      keyboard: {
        press: async (key: string) => {
          self.keys.push(key);
          if (self.keyboardFails) throw new Error("no focused control");
        },
      },
    } as unknown as Page;
  }
}

const MODULE_ID = "123e4567-e89b-12d3-a456-426614174201";
const PEEK_PANEL = "page testid(modules-peek)";
const SEARCH_INPUT = "page testid(modules-search-input)";
const MENU_ITEMS = 'page css([role="menuitem"]:visible)';

function peekPage(extra: Table = {}, search = `?peekModule=${MODULE_ID}`): FakePage {
  const fake = new FakePage({
    [PEEK_PANEL]: { count: 1, visible: true },
    [`${PEEK_PANEL} role(heading,)`]: { count: 1, text: " Archived module " },
    [`${PEEK_PANEL} testid(modules-peek-summary)`]: { count: 1, visible: true },
    ...extra,
  });
  fake.urlString = `/acme/projects/p1/archives/modules${search}`;
  return fake;
}

const STATUS_CONTROL =
  'page testid(modules-peek) css(button[aria-label="Backlog"], button[aria-label="Planned"], button[aria-label="In Progress"], button[aria-label="Paused"], button[aria-label="Completed"], button[aria-label="Cancelled"])';

describe("the archived sort control", () => {
  it("reads the active key from the header label", async () => {
    const fake = new FakePage({ "page testid(modules-sort-label)": { count: 1, text: "  Name " } });
    await expect(new WebNewDriver(fake.asPage()).archivesModuleSortLabel()).resolves.toBe("Name");
  });

  it("reads empty when no list is mounted", async () => {
    await expect(new WebNewDriver(new FakePage().asPage()).archivesModuleSortLabel()).resolves.toBe("");
  });

  it("reads empty when the label has no text", async () => {
    const fake = new FakePage({ "page testid(modules-sort-label)": { count: 1, nullText: true } });
    await expect(new WebNewDriver(fake.asPage()).archivesModuleSortLabel()).resolves.toBe("");
  });

  it("opens the menu and presses the entry that names the chosen key", async () => {
    const fake = new FakePage({ "page testid(modules-sort-trigger)": { count: 1, visible: true } });
    await new WebNewDriver(fake.asPage()).archivesSetModuleSort("Created date");

    // The trigger opens the menu; only the entry naming the chosen key is
    // pressed, so a similarly named key can never answer.
    expect(fake.clicks).toEqual(["page testid(modules-sort-trigger)", "page role(menuitem,Created date)"]);
    expect(fake.waits.filter((wait) => wait.startsWith("page role(menuitem,Created date)")).length).toBe(2);
  });

  it("tolerates a menu that is already gone before the hidden wait settles", async () => {
    const fake = new FakePage({ "page testid(modules-sort-trigger)": { count: 1, visible: true } });
    fake.set("page role(menuitem,Descending)", { count: 1, visible: true, rejectWaitOnHidden: true });
    await expect(new WebNewDriver(fake.asPage()).archivesSetModuleSort("Descending")).resolves.toBeUndefined();
  });
});

describe("the archived search box", () => {
  it("reports the field shown only while the box is expanded", async () => {
    const open = new FakePage({ "page testid(modules-search-field)": { count: 1, visible: true } });
    await expect(new WebNewDriver(open.asPage()).archivesModuleSearchVisible()).resolves.toBe(true);

    const collapsed = new FakePage({ "page testid(modules-search-field)": { count: 1, visible: false } });
    await expect(new WebNewDriver(collapsed.asPage()).archivesModuleSearchVisible()).resolves.toBe(false);

    const absent = new FakePage({ "page testid(modules-search-field)": { count: 0 } });
    await expect(new WebNewDriver(absent.asPage()).archivesModuleSearchVisible()).resolves.toBe(false);
  });

  it("reads the typed text and treats a collapsed box as empty", async () => {
    const typed = new FakePage({ [SEARCH_INPUT]: { count: 1, value: "Alpha" } });
    await expect(new WebNewDriver(typed.asPage()).archivesModuleSearchText()).resolves.toBe("Alpha");

    const collapsed = new FakePage({ [SEARCH_INPUT]: { count: 0 } });
    await expect(new WebNewDriver(collapsed.asPage()).archivesModuleSearchText()).resolves.toBe("");
  });

  it("sends Escape to the field itself, so the box can clear then collapse", async () => {
    const fake = new FakePage({ [SEARCH_INPUT]: { count: 1, value: "Alpha" } });
    await new WebNewDriver(fake.asPage()).archivesEscapeModuleSearch();
    expect(fake.presses).toEqual([`${SEARCH_INPUT}:Escape`]);
  });
});

describe("the active-filter marker", () => {
  it("follows the dot on the trigger", async () => {
    const shown = new FakePage({ "page testid(modules-filter-active)": { count: 1, visible: true } });
    await expect(new WebNewDriver(shown.asPage()).archivesModuleFiltersActive()).resolves.toBe(true);

    const hidden = new FakePage({ "page testid(modules-filter-active)": { count: 1, visible: false } });
    await expect(new WebNewDriver(hidden.asPage()).archivesModuleFiltersActive()).resolves.toBe(false);

    const absent = new FakePage({ "page testid(modules-filter-active)": { count: 0 } });
    await expect(new WebNewDriver(absent.asPage()).archivesModuleFiltersActive()).resolves.toBe(false);
  });
});

describe("the slow-fetch skeleton", () => {
  it("arms the observer, holds only the archived read and settles afterwards", async () => {
    const fake = new FakePage({
      "page testid(modules-list)": { visible: true },
      "page testid(modules-row) filter()": { visible: true },
      "page testid(project-row-menu)": { visible: true },
    });
    fake.urlString = "/acme/projects/p1/modules";

    await expect(new WebNewDriver(fake.asPage()).archivesModulesShowsSkeleton("acme", "p1")).resolves.toBe(true);

    expect(fake.initScripts).toHaveLength(1);
    expect(fake.routes).toHaveLength(1);
    // Already on the live list, so the entry is made without navigating.
    expect(fake.gotos).toEqual([]);
    // Entry goes through the project menu and the archives strip, not a
    // fabricated address.
    expect(fake.clicks.some((click) => click.includes("testid(project-row-menu)"))).toBe(true);
    expect(fake.clicks.some((click) => click.includes("/archives/modules"))).toBe(true);
    expect(fake.unroutes).toEqual([fake.routes[0]?.matcher]);

    const matcher = fake.routes[0]?.matcher as (url: URL) => boolean;
    expect(matcher(new URL("https://app.test/api/workspaces/acme/projects/p1/archived-modules/"))).toBe(true);
    expect(matcher(new URL("https://app.test/api/workspaces/acme/projects/p1/modules/"))).toBe(false);
  });

  it("holds a GET for three seconds and lets any other request straight through", async () => {
    const fake = new FakePage({ "page testid(project-row-menu)": { visible: true } });
    await new WebNewDriver(fake.asPage()).archivesModulesShowsSkeleton("acme", "p1");
    const handler = fake.routes[0]?.handler;
    if (handler === undefined) throw new Error("no route handler armed");

    let continued = 0;
    const route = (method: string) => ({
      request: () => ({ method: () => method }),
      continue: async () => {
        continued += 1;
      },
    });
    vi.useFakeTimers();
    try {
      const held = handler(route("GET"));
      await vi.advanceTimersByTimeAsync(3_000);
      await held;
      expect(continued).toBe(1);
      await handler(route("POST"));
      expect(continued).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("releases the route even when unroute or continue refuse", async () => {
    const fake = new FakePage({ "page testid(project-row-menu)": { visible: true } });
    fake.unrouteFails = true;
    await expect(new WebNewDriver(fake.asPage()).archivesModulesShowsSkeleton("acme", "p1")).resolves.toBe(true);
    const handler = fake.routes[0]?.handler;
    if (handler === undefined) throw new Error("no route handler armed");
    await expect(
      handler({
        request: () => ({ method: () => "GET" }),
        continue: async () => {
          throw new Error("page closed");
        },
      })
    ).resolves.toBeUndefined();
    expect(fake.unroutes).toEqual([fake.routes[0]?.matcher]);
  });

  it("reports false when the observer never saw the skeleton", async () => {
    const fake = new FakePage({ "page testid(project-row-menu)": { visible: true } });
    fake.skeletonSeen = false;
    await expect(new WebNewDriver(fake.asPage()).archivesModulesShowsSkeleton("acme", "p1")).resolves.toBe(false);
  });

  it("releases the route even when the tab entry fails", async () => {
    const fake = new FakePage({
      'page css(li[data-project-row="p1"]) testid(project-row-menu)': { count: 1, visible: true, failClick: true },
    });
    await expect(new WebNewDriver(fake.asPage()).archivesModulesShowsSkeleton("acme", "p1")).rejects.toThrow();
    expect(fake.unroutes).toEqual([fake.routes[0]?.matcher]);
  });
});

describe("the read-only module peek", () => {
  it("reports an absent panel as locked and empty without reading a heading", async () => {
    const fake = new FakePage({ [PEEK_PANEL]: { count: 0 } });
    fake.urlString = "/acme/projects/p1/archives/modules";
    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: null,
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: false,
    });
  });

  it("reads the archived panel: locked status, no add link, summary shown", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: true },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 1, visible: true },
      [`${PEEK_PANEL} testid(modules-links-add)`]: { count: 0 },
    });

    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: "Archived module",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: true,
    });
    // A locked trigger is read, never pressed: pressing it opens nothing
    // and its dismiss Escape would close the panel being read.
    expect(fake.clicks).not.toContain(STATUS_CONTROL);
    expect(fake.keys).toEqual([]);
    // The disclosure was already open, so it is not pressed again.
    expect(fake.clicks).not.toContain(`${PEEK_PANEL} testid(modules-links-toggle)`);
  });

  it("never escapes a locked trigger, so the archived peek stays open", async () => {
    // The regression behind NEWFRONT-702's gate failure: the status button
    // is rendered but disabled on an archived module, so clicking then
    // pressing Escape reached the peek chrome and closed it mid-read.
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: true },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 1, visible: true },
    });

    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: "Archived module",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: true,
    });
    expect(fake.clicks).not.toContain(STATUS_CONTROL);
    expect(fake.keys).toEqual([]);
  });

  it("reads the live twin: the status opens its list and the add link is offered", async () => {
    const fake = peekPage(
      {
        [STATUS_CONTROL]: { count: 1, visible: true, disabled: false },
        "page role(option,)": { count: 6 },
        [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 0 },
        [`${PEEK_PANEL} testid(modules-links-toggle)`]: { count: 1, visible: true },
        [`${PEEK_PANEL} testid(modules-links-add)`]: { count: 1, visible: true },
      },
      `?peekModule=${MODULE_ID}`
    );
    // Opening the disclosure reveals the empty note, and with it the offer.
    fake.set("page role(option,)", { count: 6 });

    const panel = await new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly();
    expect(panel).toEqual({
      name: "Archived module",
      statusLocked: false,
      addLinkOffered: true,
      summaryShown: true,
    });
    expect(fake.clicks).toContain(`${PEEK_PANEL} testid(modules-links-toggle)`);
    // Closing the status list keeps the page's own Escape handlers out.
    expect(fake.keys).toContain("Escape");
  });

  it("keeps reading when the disclosure press refuses", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: true },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-toggle)`]: { count: 1, visible: true, failClick: true },
      [`${PEEK_PANEL} testid(modules-links-add)`]: { count: 0 },
    });

    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: "Archived module",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: true,
    });
    // The locked trigger is left alone; the refusal is swallowed rather
    // than failing the read, and nothing is ever escaped.
    expect(fake.clicks).not.toContain(STATUS_CONTROL);
    expect(fake.clicks).toContain(`${PEEK_PANEL} testid(modules-links-toggle)`);
    expect(fake.keys).toEqual([]);
  });

  it("keeps reading when the status press and its Escape refuse", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: false, failClick: true },
      "page role(option,)": { count: 0 },
      'page css([role="menu"]:visible, [role="listbox"]:visible)': { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 1, visible: true },
    });
    fake.keyboardFails = true;

    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: "Archived module",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: true,
    });
    // An enabled trigger is pressed; with no list opened there is nothing
    // to dismiss, so no Escape reaches the panel.
    expect(fake.clicks).toContain(STATUS_CONTROL);
    expect(fake.keys).toEqual([]);
  });

  it("keeps reading when the disclosure's empty note never lands", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-toggle)`]: { count: 1, visible: true },
      [`${PEEK_PANEL} testid(modules-links-add)`]: { visible: false },
    });
    // The disclosure is pressed but the note stays absent: the offer is
    // still read from whatever is shown, not assumed.
    fake.set(`${PEEK_PANEL} testid(modules-links-add)`, { count: 1, visible: false });
    fake.set(`${PEEK_PANEL} testid(modules-links-empty)`, { count: 0, rejectWait: true });

    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toMatchObject({
      addLinkOffered: false,
    });
    expect(fake.clicks).toContain(`${PEEK_PANEL} testid(modules-links-toggle)`);
  });

  it("treats a status control that opens only a menu as unlocked", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: false },
      "page role(option,)": { count: 0 },
      'page css([role="menu"]:visible, [role="listbox"]:visible)': { count: 1 },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 1 },
    });
    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toMatchObject({
      statusLocked: false,
    });
  });

  it("treats a status control that opens nothing at all as locked", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 1, visible: true, disabled: false },
      "page role(option,)": { count: 0 },
      'page css([role="menu"]:visible, [role="listbox"]:visible)': { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 1 },
    });
    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toMatchObject({
      statusLocked: true,
    });
  });

  it("holds the lock when the panel has no status control and no links disclosure", async () => {
    const fake = peekPage({
      [STATUS_CONTROL]: { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-empty)`]: { count: 0 },
      [`${PEEK_PANEL} testid(modules-links-toggle)`]: { count: 1, visible: false },
      [`${PEEK_PANEL} testid(modules-peek-summary)`]: { count: 1, visible: false },
    });
    await expect(new WebNewDriver(fake.asPage()).archivesModulePeekReadOnly()).resolves.toEqual({
      name: "Archived module",
      statusLocked: true,
      addLinkOffered: false,
      summaryShown: false,
    });
    expect(fake.clicks).not.toContain(`${PEEK_PANEL} testid(modules-links-toggle)`);
  });
});

describe("the module row-menu entry lists", () => {
  const liveTable = (): Table => ({
    [MENU_ITEMS]: { count: 3 },
    [`${MENU_ITEMS} nth(0)`]: { count: 1 },
    [`${MENU_ITEMS} nth(0) css([data-menu-title])`]: { count: 1, text: " Archive " },
    [`${MENU_ITEMS} nth(0)`]: { count: 1, attrs: { "data-disabled": "" } },
    [`${MENU_ITEMS} nth(1)`]: { count: 1, attrs: { "data-disabled": null } },
    [`${MENU_ITEMS} nth(1) css([data-menu-title])`]: { count: 1, text: "Edit" },
    [`${MENU_ITEMS} nth(1) css([data-menu-description])`]: { count: 1, text: "  " },
  });

  it("reads the live menu's titles, gates and explanations and closes it", async () => {
    const fake = new FakePage(liveTable());
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-description])`, {
      count: 1,
      text: "Only completed or cancelled modules can be archived",
    });
    // A blank row (no stable title) is skipped rather than guessed at.
    fake.set(`${MENU_ITEMS} nth(2)`, { count: 1 });
    fake.set(`${MENU_ITEMS} nth(2) css([data-menu-title])`, { count: 1, text: "   " });

    const entries = await new WebNewDriver(fake.asPage()).archivesLiveModuleMenuEntries("Gate Planned");

    expect(entries).toEqual([
      {
        title: "Archive",
        disabled: true,
        description: "Only completed or cancelled modules can be archived",
      },
      { title: "Edit", disabled: false, description: null },
    ]);
    expect(fake.keys).toEqual(["Escape"]);
  });

  it("reads an explanation whose text is missing as absent", async () => {
    const fake = new FakePage({ [MENU_ITEMS]: { count: 1 } });
    fake.set(`${MENU_ITEMS} nth(0)`, { count: 1, text: "Archive", attrs: { "data-disabled": "" } });
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-title])`, { count: 0 });
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-description])`, { count: 1, nullText: true });

    await expect(new WebNewDriver(fake.asPage()).archivesLiveModuleMenuEntries("Gate Planned")).resolves.toEqual([
      { title: "Archive", disabled: true, description: null },
    ]);
  });

  it("reads an already open menu without pressing a row trigger", async () => {
    const fake = new FakePage(liveTable());
    await new WebNewDriver(fake.asPage()).archivesArchivedModuleMenuEntries("Archived alpha");
    expect(fake.clicks.some((click) => click.includes('[role="menuitem"]:visible'))).toBe(false);
  });

  it("opens a closed menu through the row's hover trigger", async () => {
    const fake = new FakePage(liveTable());
    fake.set(MENU_ITEMS, { count: 0, visible: false });
    const row = "page testid(modules-row) filter()";
    fake.set(row, { count: 1, visible: true });
    const trigger = `${row} css(div[data-main-menu="true"]:visible button)`;
    fake.set(trigger, { count: 1, visible: true });
    // Pressing the trigger opens the menu.
    fake.onClick = (id) => {
      if (id === trigger) fake.set(MENU_ITEMS, { count: 3 });
    };

    const entries = await new WebNewDriver(fake.asPage()).archivesArchivedModuleMenuEntries("Archived alpha");
    expect(fake.clicks).toContain(trigger);
    expect(entries.map((entry) => entry.title)).toEqual(["Archive", "Edit"]);
  });

  it("falls back to the entry's own text when it has no stable title element", async () => {
    const fake = new FakePage({ [MENU_ITEMS]: { count: 1 } });
    fake.set(`${MENU_ITEMS} nth(0)`, { count: 1, text: "  Open in new tab ", attrs: { "data-disabled": null } });
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-title])`, { count: 0 });
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-description])`, { count: 0 });

    await expect(new WebNewDriver(fake.asPage()).archivesLiveModuleMenuEntries("Delivery")).resolves.toEqual([
      { title: "Open in new tab", disabled: false, description: null },
    ]);
  });

  it("skips an entry whose title cannot be read and survives a menu that lingers", async () => {
    const fake = new FakePage({ [MENU_ITEMS]: { count: 2 } });
    fake.set(`${MENU_ITEMS} nth(0)`, { count: 1, rejectText: true });
    fake.set(`${MENU_ITEMS} nth(0) css([data-menu-title])`, { count: 1, rejectText: true });
    fake.set(`${MENU_ITEMS} nth(1)`, { count: 1, text: "Restore", attrs: { "data-disabled": null } });
    fake.set(`${MENU_ITEMS} nth(1) css([data-menu-title])`, { count: 0 });
    // The menu lingers after Escape: the closing wait times out and is
    // swallowed rather than failing the read.
    fake.set(MENU_ITEMS, { count: 2, rejectWaitOnHidden: true });
    fake.set(`${MENU_ITEMS} nth(1) css([data-menu-description])`, { count: 1, rejectText: true });

    await expect(new WebNewDriver(fake.asPage()).archivesArchivedModuleMenuEntries("Archived alpha")).resolves.toEqual([
      { title: "Restore", disabled: false, description: null },
    ]);
    expect(fake.keys).toEqual(["Escape"]);
  });
});
