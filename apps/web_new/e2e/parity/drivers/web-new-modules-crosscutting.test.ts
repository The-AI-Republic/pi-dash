// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the modules cross-cutting driver hooks (NEWFRONT-807,
// MOD-038/039/042/043): deep links and the access wall, the peek address,
// the list reads, the dialog openers, the traffic allowlist and the
// desktop/edition marker scan. A fake Page answers from a path-keyed table
// so each case pins the hook a method reads or the control it activates;
// no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  texts?: string[];
  text?: string;
  value?: string;
  attrs?: Record<string, string | null>;
  disabled?: boolean;
  evaluate?: unknown;
  evaluated?: unknown;
}

type Table = Record<string, Answer>;

const ROOT = "page";

function keyFor(parent: string, kind: string, args: unknown[]): string {
  const rendered = args
    .map((arg) => (arg && typeof arg === "object" ? String((arg as { name?: string }).name ?? "") : String(arg)))
    .join(",");
  return `${parent} ${kind}(${rendered})`;
}

/**
 * A Playwright-shaped fake. Locators are addressed by the chain of calls
 * that built them, so a table key such as
 * `page testid(modules-row) role(button,/favorite/i)` answers
 * `page.getByTestId("modules-row").getByRole("button", { name: /favorite/i })`.
 */
class FakePage {
  readonly clicks: string[] = [];
  readonly fills: string[] = [];
  readonly pageCalls: string[] = [];
  readonly requests: Array<(request: { url(): string }) => void> = [];
  path = "/acme/projects/p1/modules";
  focus = "body";

  /** Swap one answer mid-test (a control that opens on click). */
  set(key: string, answer: Answer): void {
    this.table[key] = answer;
  }

  constructor(private readonly table: Table = {}) {}

  private answer(id: string): Answer & { visible: boolean; count: number } {
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.startsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    return { visible: true, count: 1, ...(candidates[0]?.[1] ?? {}) };
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: () => self.locator(id),
      filter: (options: { has?: Locator; hasText?: string | RegExp }) =>
        self.locator(`${id} has(${String((options.has as never)?.["__id"] ?? "")},${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      getByLabel: (text: string, options?: { exact?: boolean }) =>
        self.locator(`${id} label(${text},${String(options?.exact ?? false)})`),
      getByText: (text: string, options?: { exact?: boolean }) =>
        self.locator(`${id} text(${text},${String(options?.exact ?? false)})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      fill: async (value: string) => {
        self.fills.push(`${id}=${value}`);
      },
      press: async () => undefined,
      hover: async () => undefined,
      waitFor: async () => undefined,
      evaluate: async () => self.focus,
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => {
        self.pageCalls.push(`testid:${testId}`);
        return self.locator(keyFor(ROOT, "testid", [testId]));
      },
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(keyFor(ROOT, "role", [role, String(options?.name ?? "")])),
      getByLabel: (text: string, options?: { exact?: boolean }) =>
        self.locator(keyFor(ROOT, "label", [text, String(options?.exact ?? false)])),
      getByText: (text: string, options?: { exact?: boolean }) =>
        self.locator(keyFor(ROOT, "text", [text, String(options?.exact ?? false)])),
      locator: (selector: string) => self.locator(keyFor(ROOT, "css", [selector])),
      url: () => `https://app.test${self.path}`,
      title: async () => "Delivery - Modules",
      evaluate: async (callback: (arg?: unknown) => unknown) => callback(),
      goto: async (url: string) => {
        self.pageCalls.push(`goto:${url}`);
      },
      waitForLoadState: async () => {
        self.pageCalls.push("waitForLoadState");
      },
      reload: async () => {
        self.pageCalls.push("reload");
      },
      waitForTimeout: async () => undefined,
      keyboard: { press: async () => undefined },
      on: (event: string, listener: (request: { url(): string }) => void) => {
        if (event === "request") self.requests.push(listener);
      },
      off: (event: string, listener: (request: { url(): string }) => void) => {
        if (event !== "request") return;
        const at = self.requests.indexOf(listener);
        if (at >= 0) self.requests.splice(at, 1);
      },
      waitForResponse: async () => ({ ok: () => true }),
      context: () => ({ addCookies: async () => undefined }),
    };
    return page as unknown as Page;
  }

  /** Deliver one request URL to every attached listener (a page load). */
  emitRequest(url: string): void {
    for (const listener of [...this.requests]) listener({ url: () => url });
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

const SETTLED =
  'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"])';

describe("modulesXOpenModulesList", () => {
  it("navigates to the list address and settles on its body", async () => {
    const fake = new FakePage({ [SETTLED]: { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesXOpenModulesList("acme", "p1");
    expect(fake.pageCalls).toContain("goto:/acme/projects/p1/modules");
    expect(fake.pageCalls).toContain("waitForLoadState");
  });
});

describe("modulesXListGateVisible and the gate shortcut", () => {
  it("reads the flag gate and its shortcut state", async () => {
    const fake = new FakePage({
      "page testid(modules-feature-gate)": { count: 1, visible: true },
      "page testid(modules-feature-gate) role(button,Manage features)": { count: 1, visible: true, disabled: true },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesXListGateVisible()).toBe(true);
    expect(await driver.modulesXGateManageFeaturesState()).toEqual({ visible: true, disabled: true });
  });

  it("reports no shortcut when the gate is absent", async () => {
    const fake = new FakePage({ "page testid(modules-feature-gate)": { count: 0 } });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesXListGateVisible()).toBe(false);
    expect(await driver.modulesXGateManageFeaturesState()).toEqual({ visible: false, disabled: false });
  });
});

describe("modulesXListNames", () => {
  it("reads the names of the active layout", async () => {
    const fake = new FakePage({
      [SETTLED]: { count: 1 },
      "page testid(modules-list) testid(modules-row-name)": { texts: ["Alpha"] },
    });
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("list");
    expect(await driver.modulesXListNames()).toEqual(["Alpha"]);
  });

  it("reads card names on the board layout", async () => {
    const fake = new FakePage({
      [SETTLED]: { count: 1 },
      "page testid(modules-list) testid(modules-row-name)": { texts: [] },
      "page testid(modules-list) testid(modules-board-card-name)": { texts: ["Card"] },
    });
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("board");
    expect(await driver.modulesXListNames()).toEqual(["Card"]);
  });
});

describe("the module dialog hooks", () => {
  const form = "page testid(module-dialog-form)";

  it("reports the dialog open when its form shows", async () => {
    const fake = new FakePage({ [form]: { count: 1, visible: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesXModuleDialogOpen()).toBe(true);
  });

  it("reports the dialog closed when its form is gone", async () => {
    const fake = new FakePage({ [form]: { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesXModuleDialogOpen()).toBe(false);
  });

  it("fills the title field by its label", async () => {
    const fake = new FakePage({ [form]: { count: 1 }, [`${form} label(Title,true)`]: { count: 1, value: "Delivery" } });
    await new WebNewDriver(fake.asPage()).modulesXFillCreateName("Delivery");
    expect(fake.fills).toEqual([`${form} label(Title,true)=Delivery`]);
  });

  it("submits create through the form's own button", async () => {
    const fake = new FakePage({
      [form]: { count: 1 },
      [`${form} role(button,Create Module)`]: { count: 1 },
    });
    await new WebNewDriver(fake.asPage()).modulesXSubmitCreate();
    expect(fake.clicks).toEqual([`${form} role(button,Create Module)`]);
  });

  it("opens the edit dialog from a row's menu", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-row"], [data-testid="modules-board-card"])': { count: 1 },
      "page role(menuitem,Edit)": { count: 1 },
      [form]: { count: 1 },
    });
    await new WebNewDriver(fake.asPage()).modulesXOpenEditDialogFromRow("Delivery");
    expect(fake.clicks.some((click) => click.startsWith('page css([data-testid="modules-row"]'))).toBe(true);
    expect(fake.clicks.some((click) => click.includes("role(menuitem,Edit)"))).toBe(true);
  });

  it("walks Tab from the dialog's focus through the focus descriptors", async () => {
    const fake = new FakePage({ [form]: { count: 1 } });
    const page = fake.asPage();
    const press = vi.fn(async () => undefined);
    Object.defineProperty(page, "keyboard", { value: { press } });
    const driver = new WebNewDriver(page);
    const stops = await driver.modulesXDialogTabSequence();
    expect(stops).toHaveLength(12);
    expect(press).toHaveBeenCalledTimes(12);
    expect(await driver.modulesXDialogFocused()).toBe("body");
  });
});

describe("the deep-link hooks (MOD-038)", () => {
  it("opens a detail address and waits for one of its three verdicts", async () => {
    const fake = new FakePage({
      'page css([data-testid="module-detail-screen"], [data-testid="modules-gone-away"], [data-testid="project-access-wall"])':
        { count: 1 },
    });
    await new WebNewDriver(fake.asPage()).modulesXOpenModuleDetail("acme", "p1", "m1");
    expect(fake.pageCalls).toContain("goto:/acme/projects/p1/modules/m1");
    expect(fake.pageCalls).toContain("waitForLoadState");
  });

  it("reads the detail name from the sidebar's name field", async () => {
    const fake = new FakePage({
      "page testid(module-detail-screen)": { count: 1, visible: true },
      "page testid(module-detail-screen) testid(modules-sidebar-name)": {
        count: 1,
        visible: true,
        value: " Delivery ",
      },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXDetailName()).toBe("Delivery");
  });

  it("reads no detail name off the detail page", async () => {
    const fake = new FakePage({ "page testid(module-detail-screen)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesXDetailName()).toBeNull();
  });

  it("reads the missing-module state and its way back", async () => {
    const fake = new FakePage({
      "page testid(modules-gone-away)": { count: 1, visible: true },
      "page testid(modules-gone-away) role(button,Back to modules)": { count: 1 },
      [SETTLED]: { count: 1 },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesXDetailMissingVisible()).toBe(true);
    await driver.modulesXMissingModuleBackToList();
    expect(fake.clicks).toEqual(["page testid(modules-gone-away) role(button,Back to modules)"]);
  });

  it("reports the access wall through its own hook", async () => {
    const fake = new FakePage({ "page testid(project-access-wall)": { count: 1, visible: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesXProjectAccessWallVisible()).toBe(true);
  });

  it("opens the list carrying the peek key and reads the panel's name", async () => {
    const fake = new FakePage({
      [SETTLED]: { count: 1 },
      'page css([data-testid="modules-sidebar-name"], [data-testid="modules-gone-away"])': { count: 1 },
      "page testid(modules-list-screen)": { count: 1, visible: true },
      "page testid(modules-list-screen) testid(modules-sidebar-name)": { count: 1, visible: true, value: "Delivery" },
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesXOpenModulesListWithPeek("acme", "p1", "m1");
    expect(fake.pageCalls).toContain("goto:/acme/projects/p1/modules?peekModule=m1");
    expect(await driver.modulesXPeekName()).toBe("Delivery");
  });

  it("reads no peek name on the detail page", async () => {
    const fake = new FakePage({ "page testid(modules-list-screen)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesXPeekName()).toBeNull();
  });
});

describe("the list search hooks", () => {
  const header = "page testid(modules-list-header)";
  const field = `${header} testid(modules-search-field)`;
  const trigger = `${header} role(button,Search modules)`;

  it("opens the search through its icon and reports the state", async () => {
    const fake = new FakePage({
      [field]: { count: 1, visible: true },
      [trigger]: { count: 1 },
      [`${header} role(textbox,Search modules by name)`]: { count: 1, value: "deliv" },
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesXOpenListSearch();
    await driver.modulesXFillListSearch("deliv");
    expect(fake.fills).toEqual([`${header} role(textbox,Search modules by name)=deliv`]);
    expect(await driver.modulesXListSearchState()).toEqual({ expanded: true, query: "deliv" });
  });

  it("clicks the trigger when the field is collapsed", async () => {
    const fake = new FakePage({ [field]: { count: 0, visible: false }, [trigger]: { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesXOpenListSearch();
    expect(fake.clicks).toEqual([trigger]);
    expect(await new WebNewDriver(fake.asPage()).modulesXListSearchState()).toEqual({ expanded: false, query: "" });
  });
});

describe("the layout and analytics reads", () => {
  it("switches layout through the shared list hook", async () => {
    const fake = new FakePage({ [SETTLED]: { count: 1 } });
    const driver = new WebNewDriver(fake.asPage());
    const switchLayout = vi.fn(async () => undefined);
    Object.defineProperty(driver, "modulesSetLayout", { value: switchLayout });
    await driver.modulesXSwitchModulesLayout("board");
    expect(switchLayout).toHaveBeenCalledWith("board");
  });

  it("reads board names, falling back to rows when the board shows none", async () => {
    const fake = new FakePage({
      [SETTLED]: { count: 1 },
      "page testid(modules-list) testid(modules-row-name)": { texts: ["Row"] },
      "page testid(modules-list) testid(modules-board-card-name)": { texts: [] },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXBoardNames()).toEqual(["Row"]);
  });

  it("reads the analytics sidebar and the links section", async () => {
    const fake = new FakePage({
      "page testid(modules-analytics-sidebar)": { count: 1, visible: true },
      "page testid(modules-links-section)": { count: 1, visible: true },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesXAnalyticsSidebarVisible()).toBe(true);
    expect(await driver.modulesXLinksSectionVisible()).toBe(true);
  });

  it("reads the links disclosure when the section hook is absent", async () => {
    const fake = new FakePage({
      "page testid(modules-links-section)": { count: 0 },
      "page testid(modules-analytics-sidebar)": { count: 1, visible: true },
      "page testid(modules-analytics-sidebar) role(button,/^links\\b/i)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXLinksSectionVisible()).toBe(true);
  });

  it("reads no links section without a sidebar", async () => {
    const fake = new FakePage({
      "page testid(modules-links-section)": { count: 0 },
      "page testid(modules-analytics-sidebar)": { count: 0 },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXLinksSectionVisible()).toBe(false);
  });
});

describe("the traffic allowlist (MOD-042/043)", () => {
  it("collects only module-owned paths from one list entry and detaches", async () => {
    const fake = new FakePage({ [SETTLED]: { count: 1 } });
    const driver = new WebNewDriver(fake.asPage());
    const open = vi.spyOn(driver, "modulesXOpenModulesList").mockImplementation(async () => {
      fake.emitRequest("https://api.test/api/workspaces/acme/projects/p1/modules/");
      fake.emitRequest("https://api.test/api/workspaces/acme/user-favorite-modules/");
      fake.emitRequest("https://api.test/api/workspaces/acme/projects/p1/");
      fake.emitRequest("https://api.test/api/billing/subscriptions/");
      fake.emitRequest("not a url");
    });
    const paths = await driver.modulesXEntryRequestPaths("acme", "p1");
    expect(open).toHaveBeenCalledWith("acme", "p1");
    expect(paths).toEqual(["/api/workspaces/acme/projects/p1/modules/", "/api/workspaces/acme/user-favorite-modules/"]);
    expect(fake.requests).toHaveLength(0);
  });
});

describe("the favorite toggle and the filters menu", () => {
  it("reads the star only inside the first row", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-row"])': { count: 1 },
      'page css([data-testid="modules-row"]) role(button,/favorite/i)': { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXRowFavoriteToggleVisible()).toBe(true);
  });

  it("reports no star without a row", async () => {
    const fake = new FakePage({ 'page css([data-testid="modules-row"])': { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesXRowFavoriteToggleVisible()).toBe(false);
  });

  it("opens a closed filter panel from the header trigger", async () => {
    const fake = new FakePage({
      "page testid(modules-list-header) testid(modules-filter-trigger)": { count: 1 },
      "page testid(modules-filter-panel)": { count: 0, visible: false },
    });
    const page = fake.asPage();
    // The panel appears once its trigger is pressed.
    Object.defineProperty(page, "getByTestId", {
      value: (testId: string) => {
        if (testId === "modules-filter-panel" && fake.clicks.length > 0) {
          fake.set("page testid(modules-filter-panel)", { count: 1, visible: true });
        }
        return fake.locator(keyFor(ROOT, "testid", [testId]));
      },
    });
    expect(await new WebNewDriver(page).modulesXFiltersMenuOpens()).toBe(true);
    expect(fake.clicks).toEqual(["page testid(modules-list-header) testid(modules-filter-trigger)"]);
  });

  it("leaves an already-open filter panel alone", async () => {
    const fake = new FakePage({
      "page testid(modules-list-header) testid(modules-filter-trigger)": { count: 1 },
      "page testid(modules-filter-panel)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXFiltersMenuOpens()).toBe(true);
    expect(fake.clicks).toEqual([]);
  });

  it("reports no filter menu without a trigger", async () => {
    const fake = new FakePage({ "page testid(modules-list-header) testid(modules-filter-trigger)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesXFiltersMenuOpens()).toBe(false);
  });
});

describe("the desktop and edition marker scan (MOD-042/043)", () => {
  it("finds no gate marker on a plain list", async () => {
    const fake = new FakePage({
      "page css(main) role(button,/upgrade|premium|enterprise|contact sales|go to cloud/i)": { count: 0 },
      "page css(main) role(link,/upgrade|premium|enterprise|contact sales|go to cloud/i)": { count: 0 },
      "page css(main) role(menuitem,/upgrade|premium|enterprise|contact sales|go to cloud/i)": { count: 0 },
      "page css(main) role(heading,Desktop app)": { count: 0 },
      "page css(main) role(heading,Offline mode)": { count: 0 },
      "page css(main) role(heading,Available on)": { count: 0 },
      "page css(main) role(heading,Not available in your plan)": { count: 0 },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXEditionGateMarkersPresent("list")).toBe(false);
  });

  it("finds an upgrade offer when one shows", async () => {
    const fake = new FakePage({
      "page css(main) role(button,/upgrade|premium|enterprise|contact sales|go to cloud/i)": {
        count: 1,
        visible: true,
      },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXEditionGateMarkersPresent("detail")).toBe(true);
  });

  it("finds a desktop-only heading when one shows", async () => {
    const fake = new FakePage({
      "page css(main) role(heading,Desktop app)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXEditionGateMarkersPresent("detail")).toBe(true);
  });
});
