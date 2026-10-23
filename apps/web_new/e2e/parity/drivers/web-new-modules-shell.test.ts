// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the modules list shell driver methods (NEWFRONT-793,
// MOD-001/002/008/011): the trail, the tab title, the flag gate, the layout
// switcher in both widths, the slow-fetch skeleton probe and the reload.
// A fake Page answers from a path-keyed table so each case pins the hook a
// method reads or the control it activates; no browser runs here.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  texts?: string[];
  text?: string;
  attrs?: Record<string, string | null>;
  disabled?: boolean;
  evaluate?: unknown;
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
 * `page testid(modules-list) testid(modules-row-name)` answers
 * `page.getByTestId("modules-list").last().getByTestId("modules-row-name")`.
 */
class FakePage {
  readonly clicks: string[] = [];
  readonly keys: string[] = [];
  readonly pageCalls: string[] = [];
  readonly routes: unknown[] = [];
  path = "/acme/projects/p1/modules";

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
      filter: (options: { has?: Locator }) =>
        self.locator(`${id} has(${String((options.has as never)?.["__id"] ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string }) =>
        self.locator(`${id} role(${role},${options?.name ?? ""})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      waitFor: async () => undefined,
      evaluate: async () => self.answer(id).evaluate ?? {},
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => {
        self.keys.push(`testid:${testId}`);
        return self.locator(keyFor(ROOT, "testid", [testId]));
      },
      getByRole: (role: string, options?: { name?: string }) => {
        self.keys.push(`role:${role}:${options?.name ?? ""}`);
        return self.locator(keyFor(ROOT, "role", [role, options?.name ?? ""]));
      },
      locator: (selector: string) => {
        self.keys.push(`css:${selector}`);
        return self.locator(keyFor(ROOT, "css", [selector]));
      },
      url: () => `https://app.test${self.path}`,
      title: async () => "Website relaunch - Modules",
      evaluate: async (callback: (arg?: unknown) => unknown) => callback(),
      goto: async (url: string) => {
        self.pageCalls.push(`goto:${url}`);
      },
      reload: async () => {
        self.pageCalls.push("reload");
      },
      waitForLoadState: async () => {
        self.pageCalls.push("waitForLoadState");
      },
      waitForURL: async (pattern: RegExp) => {
        self.pageCalls.push(`waitForURL:${String(pattern)}`);
      },
      waitForTimeout: async () => undefined,
      keyboard: { press: async () => undefined },
      route: async (matcher: unknown) => {
        self.routes.push(matcher);
      },
      unrouteAll: async () => {
        self.pageCalls.push("unrouteAll");
      },
      context: () => ({ addCookies: async () => undefined }),
    };
    return page as unknown as Page;
  }
}

const SWITCHER = "page role(group,Module layout)";

function switcherTable(active: "list" | "board" | "timeline" | null): Table {
  const pressed = (layout: string): Answer => ({
    count: 1,
    visible: true,
    attrs: { "aria-pressed": layout === active ? "true" : "false" },
  });
  return {
    [`${SWITCHER} css([data-layout="list"])`]: pressed("list"),
    [`${SWITCHER} css([data-layout="board"])`]: pressed("board"),
    [`${SWITCHER} css([data-layout="timeline"])`]: pressed("timeline"),
    [`${SWITCHER} role(button,)`]: { count: 3 },
    [SWITCHER]: { count: 1, visible: true },
    "page role(menuitem,)": { count: 2, texts: ["List layout", "Gallery layout"] },
  };
}

function compactDriver(table: Table = {}, layout: "list" | "board" | "timeline" = "board"): WebNewDriver {
  const fake = new FakePage({ [SWITCHER]: { count: 0 }, ...table });
  const driver = new WebNewDriver(fake.asPage());
  vi.spyOn(
    driver as unknown as { currentModulesLayout: () => Promise<unknown> },
    "currentModulesLayout"
  ).mockResolvedValue(layout);
  return driver;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("modulesBreadcrumb", () => {
  it("reads the project crumb and the trailing trail", async () => {
    const page = new FakePage().asPage();
    vi.spyOn(page, "evaluate").mockResolvedValue(["Website relaunch", "Modules"]);
    expect(await new WebNewDriver(page).modulesBreadcrumb()).toEqual({
      project: "Website relaunch",
      trail: ["Modules"],
    });
  });

  it("reads an empty trail when no breadcrumb renders", async () => {
    const page = new FakePage().asPage();
    vi.spyOn(page, "evaluate").mockResolvedValue([]);
    expect(await new WebNewDriver(page).modulesBreadcrumb()).toEqual({ project: "", trail: [] });
  });
});

describe("modulesPageTitle", () => {
  it("reads the document title", async () => {
    expect(await new WebNewDriver(new FakePage().asPage()).modulesPageTitle()).toBe("Website relaunch - Modules");
  });
});

describe("modulesAwaitRow", () => {
  it("waits on a row whose own name span matches exactly", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-row"], [data-testid="modules-board-card"], [data-testid="modules-timeline-row"])':
        {
          count: 1,
        },
    });
    await new WebNewDriver(fake.asPage()).modulesAwaitRow("Frame (Alpha)");
    expect(fake.keys[0]).toBe(
      'css:[data-testid="modules-row"], [data-testid="modules-board-card"], [data-testid="modules-timeline-row"]'
    );
  });
});

describe("modulesRowNames", () => {
  it("compacts whitespace and drops blank names", async () => {
    const fake = new FakePage({
      "page testid(modules-list) testid(modules-row-name)": { texts: ["Frame Alpha", "  Frame  Bravo ", ""] },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesRowNames()).toEqual(["Frame Alpha", "Frame Bravo"]);
  });

  it("reads nothing when no list renders", async () => {
    const fake = new FakePage({ "page testid(modules-list)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesRowNames()).toEqual([]);
  });
});

describe("modulesGateFacts", () => {
  const gate = "page testid(modules-feature-gate)";

  it("reports the explanation, the shortcut label, its enabled state and the art", async () => {
    const fake = new FakePage({
      [gate]: { evaluate: { explanation: "Modules are not enabled.", hasArt: true } },
      [`${gate} role(button,Manage features)`]: { count: 1, text: " Manage  features " },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesGateFacts()).toEqual({
      explanation: "Modules are not enabled.",
      buttonLabel: "Manage features",
      buttonDisabled: false,
      hasArt: true,
    });
  });

  it("reports no shortcut when none renders", async () => {
    const fake = new FakePage({
      [gate]: { evaluate: { explanation: "Off", hasArt: false } },
      [`${gate} role(button,Manage features)`]: { count: 0 },
    });
    const facts = await new WebNewDriver(fake.asPage()).modulesGateFacts();
    expect(facts).toEqual({ explanation: "Off", buttonLabel: null, buttonDisabled: false, hasArt: false });
  });
});

describe("modulesGateToFeatures", () => {
  it("clicks the shortcut and waits for the project feature settings", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesGateToFeatures();
    expect(fake.clicks).toEqual(["page testid(modules-feature-gate) role(button,Manage features)"]);
    expect(fake.pageCalls.some((call) => call.startsWith("waitForURL:"))).toBe(true);
  });
});

describe("modulesListOpenList", () => {
  it("navigates to the modules address and settles", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"])': {
        count: 1,
      },
    });
    fake.path = "/acme/projects/p1/overview";
    await new WebNewDriver(fake.asPage()).modulesListOpenList("acme", "p1");
    expect(fake.pageCalls).toContain("goto:/acme/projects/p1/modules");
    expect(fake.pageCalls).toContain("waitForLoadState");
  });

  it("skips the navigation when the list address is already open", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesListOpenList("acme", "p1");
    expect(fake.pageCalls.some((call) => call.startsWith("goto:"))).toBe(false);
  });
});

describe("modulesListVisibleNames", () => {
  it("reads each layout's own name hook", async () => {
    const fake = new FakePage({
      "page testid(modules-list) testid(modules-row-name)": { texts: ["Row"] },
      "page testid(modules-list) testid(modules-board-card-name)": { texts: ["Card"] },
      "page testid(modules-list) testid(modules-timeline-row-name)": { texts: ["Bar"] },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesListVisibleNames()).toEqual(["Row"]);
    expect(await driver.modulesListBoardCardNames()).toEqual(["Card"]);
    expect(await driver.modulesListTimelineNames()).toEqual(["Bar"]);
  });
});

describe("modulesListSkeletonVisible", () => {
  it("reads the skeleton hook", async () => {
    const fake = new FakePage({ "page testid(modules-list-skeleton)": { count: 1, visible: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesListSkeletonVisible()).toBe(true);
  });
});

describe("modulesListEmptyKind", () => {
  it("reads rows once the list settles", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"])': {
        count: 1,
      },
      "page testid(modules-list) testid(modules-row-name)": { texts: ["Row"] },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesListEmptyKind()).toBe("rows");
  });

  it("leaves the zero and no-match readers to slices F and B", async () => {
    const driver = new WebNewDriver(new FakePage().asPage());
    expect(await driver.modulesListNoMatchHeading()).toBeNull();
    expect(await driver.modulesListNoMatchHint()).toBeNull();
  });
});

describe("modulesListSwitcherVisible", () => {
  it("reads the wide switcher group", async () => {
    const fake = new FakePage({ [SWITCHER]: { count: 1, visible: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesListSwitcherVisible()).toBe(true);
  });

  it("reads false when the group is absent", async () => {
    const fake = new FakePage({ [SWITCHER]: { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesListSwitcherVisible()).toBe(false);
  });
});

describe("modulesListActiveLayout", () => {
  it("reads the pressed option", async () => {
    const fake = new FakePage(switcherTable("board"));
    expect(await new WebNewDriver(fake.asPage()).modulesListActiveLayout()).toBe("board");
  });

  it("reads the compact trigger's current layout on a small screen", async () => {
    const fake = new FakePage({ [SWITCHER]: { count: 0 } });
    const page = fake.asPage();
    vi.spyOn(page, "evaluate").mockResolvedValue("Change module layout, current Timeline");
    expect(await new WebNewDriver(page).modulesListActiveLayout()).toBe("timeline");
  });

  it("throws once no layout reads as active", async () => {
    const fake = new FakePage({ [SWITCHER]: { count: 0 } });
    const page = fake.asPage();
    vi.spyOn(page, "evaluate").mockResolvedValue(null);
    await expect(new WebNewDriver(page).modulesListActiveLayout()).rejects.toThrow(/no modules layout/);
  });
});

describe("modulesListLayoutOptionCount", () => {
  it("counts the switcher buttons", async () => {
    const fake = new FakePage(switcherTable("list"));
    expect(await new WebNewDriver(fake.asPage()).modulesListLayoutOptionCount()).toBe(3);
  });

  it("reads zero without a switcher", async () => {
    const fake = new FakePage({ [SWITCHER]: { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesListLayoutOptionCount()).toBe(0);
  });
});

describe("modulesListSwitchLayout", () => {
  it("clicks the option and confirms the new active layout", async () => {
    const fake = new FakePage(switcherTable("board"));
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(driver, "modulesListActiveLayout").mockResolvedValue("board");
    await driver.modulesListSwitchLayout("board");
    expect(fake.clicks).toContain(`${SWITCHER} css([data-layout="board"])`);
  });
});

describe("modulesSetLayout", () => {
  it("uses the wide switcher when it is showing", async () => {
    const fake = new FakePage();
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(driver, "modulesListSwitcherVisible").mockResolvedValue(true);
    const switchLayout = vi.spyOn(driver, "modulesListSwitchLayout").mockResolvedValue(undefined);
    const compact = vi.spyOn(driver, "modulesListCompactSwitchLayout").mockResolvedValue(undefined);
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("timeline");
    await driver.modulesSetLayout("timeline");
    expect(switchLayout).toHaveBeenCalledWith("timeline");
    expect(compact).not.toHaveBeenCalled();
  });

  it("falls back to the compact menu and rejects a mismatch", async () => {
    const driver = compactDriver({}, "list");
    vi.spyOn(driver, "modulesListSwitcherVisible").mockResolvedValue(false);
    const compact = vi.spyOn(driver, "modulesListCompactSwitchLayout").mockResolvedValue(undefined);
    await expect(driver.modulesSetLayout("timeline")).rejects.toThrow(/expected timeline/);
    expect(compact).toHaveBeenCalledWith("list");
  });

  it("accepts the compact board choice", async () => {
    const driver = compactDriver({}, "list");
    vi.spyOn(driver, "modulesListSwitcherVisible").mockResolvedValue(false);
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("board");
    const compact = vi.spyOn(driver, "modulesListCompactSwitchLayout").mockResolvedValue(undefined);
    await driver.modulesSetLayout("board");
    expect(compact).toHaveBeenCalledWith("board");
  });
});

describe("modulesListReload", () => {
  it("reloads and settles", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"])': {
        count: 1,
      },
    });
    await new WebNewDriver(fake.asPage()).modulesListReload();
    expect(fake.pageCalls).toEqual(["reload"]);
  });
});

describe("modulesListCompactMenu", () => {
  it("reads the trigger visibility", async () => {
    const fake = new FakePage({
      "page testid(modules-layout-menu) role(button,)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesListCompactMenuVisible()).toBe(true);
  });

  it("reads the menu entries and closes the menu", async () => {
    const fake = new FakePage({ "page role(menuitem,)": { count: 2, texts: ["List layout", "Gallery layout"] } });
    const pressed: string[] = [];
    const page = fake.asPage();
    (page as unknown as { keyboard: unknown }).keyboard = {
      press: async (key: string) => {
        pressed.push(key);
      },
    };
    expect(await new WebNewDriver(page).modulesListCompactMenuOptions()).toEqual(["List layout", "Gallery layout"]);
    expect(pressed).toEqual(["Escape"]);
  });

  it("opens the menu and picks the gallery layout", async () => {
    const trigger = "page testid(modules-layout-menu) role(button,)";
    const fake = new FakePage({ [trigger]: { count: 1, visible: true } });
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("board");
    await driver.modulesListCompactSwitchLayout("board");
    expect(fake.clicks).toEqual([trigger, "page role(menuitem,Gallery layout)"]);
  });

  it("picks the list layout by its pinned label", async () => {
    const trigger = "page testid(modules-layout-menu) role(button,)";
    const fake = new FakePage({ [trigger]: { count: 1, visible: true } });
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue("list");
    await driver.modulesListCompactSwitchLayout("list");
    expect(fake.clicks).toContain("page role(menuitem,List layout)");
  });
});

describe("modulesListSkeletonShownOnSlowFetch", () => {
  const settled = {
    count: 1,
  };

  function driverWith(
    skeleton: Answer,
    extra: Table = {},
    current: "list" | "board" = "board"
  ): { driver: WebNewDriver; fake: FakePage } {
    const fake = new FakePage({
      "page testid(modules-list-skeleton)": { count: 1, visible: true, attrs: { "data-layout": "board" }, ...skeleton },
      "page testid(modules-feature-gate)": { count: 0, visible: false },
      'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"])':
        settled,
      ...extra,
    });
    const driver = new WebNewDriver(fake.asPage());
    vi.spyOn(driver, "modulesListOpenList").mockResolvedValue(undefined);
    vi.spyOn(
      driver as unknown as { currentModulesLayout: () => Promise<unknown> },
      "currentModulesLayout"
    ).mockResolvedValue(current);
    return { driver, fake };
  }

  it("reports the shown skeleton, its layout and its exclusivity", async () => {
    const { driver, fake } = driverWith({});
    expect(await driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "board")).toEqual({
      shown: true,
      layoutMatched: true,
      exclusive: true,
    });
    expect(fake.routes).toEqual([expect.any(Function)]);
    expect(fake.pageCalls).toContain("reload");
    expect(fake.pageCalls).toContain("unrouteAll");
  });

  it("switches layout first when the requested one is not active", async () => {
    const { driver } = driverWith({}, {}, "board");
    const switchLayout = vi.spyOn(driver, "modulesListSwitchLayout").mockResolvedValue(undefined);
    await driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "list");
    expect(switchLayout).toHaveBeenCalledWith("list");
  });

  it("reports a skeleton sharing the screen with rows as not exclusive", async () => {
    const { driver } = driverWith(
      {},
      {
        "page testid(modules-list) testid(modules-row-name)": { texts: ["Row"] },
        "page testid(modules-list)": { count: 1, visible: true },
      }
    );
    expect(await driver.modulesListVisibleNames()).toEqual(["Row"]);
    expect(await driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "board")).toEqual({
      shown: true,
      layoutMatched: true,
      exclusive: false,
    });
  });

  it("reports a gate beside the skeleton as not exclusive", async () => {
    const { driver } = driverWith({}, { "page testid(modules-feature-gate)": { count: 1, visible: true } });
    expect((await driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "board")).exclusive).toBe(false);
  });

  it("reports a skeleton that never paints and releases the hold", async () => {
    const { driver, fake } = driverWith({ visible: false, count: 0 });
    const result = await driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "board");
    expect(result).toEqual({ shown: false, layoutMatched: false, exclusive: false });
    expect(fake.pageCalls).toContain("unrouteAll");
  });

  it("releases the hold when the settle wait fails", async () => {
    const { driver, fake } = driverWith({});
    const page = (driver as unknown as { page: Page }).page;
    vi.spyOn(page, "reload").mockRejectedValue(new Error("reload broke"));
    await expect(driver.modulesListSkeletonShownOnSlowFetch("acme", "p1", "board")).rejects.toThrow(/reload broke/);
    expect(fake.pageCalls).toContain("unrouteAll");
  });
});
