// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the MOD-036/037/040/041 driver hooks (NEWFRONT-806): the
// modules settings tab and its switch, the palette module menus, the key
// presses, the row/card drag attempts, the export scan and the timeline
// row-to-itself gesture. A fake Page answers from a path-keyed table, so
// each case pins the control a hook reads or the gesture it performs.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  texts?: string[];
  text?: string;
  value?: string;
  checked?: boolean;
  box?: { x: number; y: number; width: number; height: number } | null;
}

type Table = Record<string, Answer>;

/**
 * A Playwright-shaped fake. Locators are keyed by the chain of calls that
 * built them, so `page.getByRole("switch", { name: "Enable modules" })`
 * answers `page role(switch,Enable modules)`.
 */
class FakePage {
  readonly calls: string[] = [];
  readonly clicks: string[] = [];
  readonly mouseMoves: Array<{ x: number; y: number }> = [];
  mouseDowns = 0;
  mouseUps = 0;
  presses: string[] = [];
  evaluated = 0;
  path = "/acme/projects/p1/modules";

  constructor(private readonly table: Table = {}) {}

  set(key: string, answer: Answer): void {
    this.table[key] = answer;
  }

  private answer(id: string): Answer & { visible: boolean; count: number } {
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.startsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    const found = candidates[0]?.[1];
    return { visible: true, count: found === undefined ? 0 : 1, ...(found ?? {}) };
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: (index: number) => self.locator(`${id}#${index}`),
      filter: (options: { has?: Locator; hasText?: string | RegExp }) =>
        options.hasText === undefined
          ? self.locator(`${id} has(any)`)
          : self.locator(`${id} hasText(${String(options.hasText)})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      getByLabel: (text: string) => self.locator(`${id} label(${text})`),
      getByText: (text: string, options?: { exact?: boolean }) =>
        self.locator(`${id} text(${text},${String(options?.exact ?? false)})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isChecked: async () => self.answer(id).checked ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async () => null,
      click: async () => {
        self.clicks.push(id);
      },
      waitFor: async () => undefined,
      scrollIntoViewIfNeeded: async () => undefined,
      boundingBox: async () => {
        const answer = self.answer(id);
        return "box" in answer ? answer.box : { x: 10, y: 20, width: 100, height: 30 };
      },
      evaluateAll: async () => self.answer(id).texts ?? [],
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => {
        self.calls.push(`testid:${testId}`);
        return self.locator(`page testid(${testId})`);
      },
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`page role(${role},${String(options?.name ?? "")})`),
      getByLabel: (text: string) => self.locator(`page label(${text})`),
      getByText: (text: string, options?: { exact?: boolean }) =>
        self.locator(`page text(${text},${String(options?.exact ?? false)})`),
      locator: (selector: string) => self.locator(`page css(${selector})`),
      url: () => `https://app.test${self.path}`,
      goto: async (url: string) => {
        self.calls.push(`goto:${url}`);
      },
      waitForLoadState: async () => {
        self.calls.push("waitForLoadState");
      },
      waitForTimeout: async () => undefined,
      evaluate: async (callback: () => unknown) => {
        self.evaluated += 1;
        return callback();
      },
      keyboard: {
        press: async (key: string) => {
          self.presses.push(key);
        },
      },
      mouse: {
        move: async (x: number, y: number) => {
          self.mouseMoves.push({ x, y });
        },
        down: async () => {
          self.mouseDowns += 1;
        },
        up: async () => {
          self.mouseUps += 1;
        },
      },
      waitForResponse: async () => ({ ok: () => true }),
      waitForFunction: async () => null,
    };
    return page as unknown as Page;
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

const SWITCH = "page role(switch,Enable modules)";
const DENIED = "page testid(project-settings-denied)";

describe("modulesXOpenSettingsModules (MOD-036)", () => {
  it("opens the tab and settles on the switch", async () => {
    const fake = new FakePage({ [SWITCH]: { count: 1, visible: true } });
    await new WebNewDriver(fake.asPage()).modulesXOpenSettingsModules("acme", "p1");
    expect(fake.calls).toContain("goto:/acme/settings/projects/p1/features/modules");
    expect(fake.calls).toContain("waitForLoadState");
  });

  it("settles on the denial for a member", async () => {
    const fake = new FakePage({ [SWITCH]: { count: 0 }, [DENIED]: { count: 1, visible: true } });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesXOpenSettingsModules("acme", "p1");
    expect(await driver.modulesXSettingsShowsNotAuthorized()).toBe(true);
    expect(await driver.modulesXModulesToggleState()).toBeNull();
  });
});

describe("modulesXModulesToggleState and modulesXFlipModulesToggle (MOD-036)", () => {
  it("reads the switch's checked state", async () => {
    const fake = new FakePage({ [SWITCH]: { count: 1, visible: true, checked: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesXModulesToggleState()).toBe(true);
  });

  it("reads off, absent and hidden switches", async () => {
    const off = new FakePage({ [SWITCH]: { count: 1, visible: true, checked: false } });
    expect(await new WebNewDriver(off.asPage()).modulesXModulesToggleState()).toBe(false);
    const absent = new FakePage({ [SWITCH]: { count: 0 } });
    expect(await new WebNewDriver(absent.asPage()).modulesXModulesToggleState()).toBeNull();
    const hidden = new FakePage({ [SWITCH]: { count: 1, visible: false, checked: true } });
    expect(await new WebNewDriver(hidden.asPage()).modulesXModulesToggleState()).toBeNull();
  });

  it("flips the switch and waits for the project write", async () => {
    const fake = new FakePage({ [SWITCH]: { count: 1, visible: true, checked: true } });
    const driver = new WebNewDriver(fake.asPage());
    const waitPatch = vi.fn(async () => undefined);
    Object.defineProperty(driver, "waitProjectPatch", { value: waitPatch });
    await driver.modulesXFlipModulesToggle();
    expect(fake.clicks).toEqual([SWITCH]);
    expect(waitPatch).toHaveBeenCalledTimes(1);
  });
});

describe("modulesXPaletteModuleMenuItems (MOD-037)", () => {
  const items = 'page css([role="dialog"]) has(any) css([role="option"])';

  it("reads each row's title, decorative mark and selection check", async () => {
    const fake = new FakePage({
      [items]: { count: 2 },
      [`${items}#0`]: { text: "Delivery In Progress", visible: true },
      [`${items}#0 testid(palette-option-title)`]: { text: "Delivery" },
      [`${items}#0 testid(palette-option-icon)`]: { count: 1 },
      [`${items}#1`]: { text: "Launch Planned ✓", visible: true },
      [`${items}#1 testid(palette-option-title)`]: { text: "Launch" },
      [`${items}#1 testid(palette-option-icon)`]: { count: 0 },
    });
    const read = await new WebNewDriver(fake.asPage()).modulesXPaletteModuleMenuItems();
    expect(read).toEqual([
      { name: "Delivery", hasIcon: true, hasCheck: false },
      { name: "Launch", hasIcon: false, hasCheck: true },
    ]);
  });

  it("skips hidden rows and rows with no text", async () => {
    const fake = new FakePage({
      [items]: { count: 2 },
      [`${items}#0`]: { text: "Hidden", visible: false },
      [`${items}#1`]: { text: "   ", visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesXPaletteModuleMenuItems()).toEqual([]);
  });

  it("activates the named row", async () => {
    const target = `${items} hasText(Launch)`;
    const fake = new FakePage({ [target]: { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesXActivatePaletteModuleMenuItem("Launch");
    expect(fake.clicks).toEqual([target]);
  });
});

describe("modulesXPressKey and modulesXPressKeySequence (MOD-040)", () => {
  it("grounds focus before a bare key", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesXPressKey("n");
    expect(fake.evaluated).toBe(1);
    expect(fake.presses).toEqual(["n"]);
  });

  it("lets Escape reach the focused control", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesXPressKey("Escape");
    expect(fake.evaluated).toBe(0);
    expect(fake.presses).toEqual(["Escape"]);
  });

  it("types a sequence back to back after one focus reset", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesXPressKeySequence("nm");
    expect(fake.evaluated).toBe(1);
    expect(fake.presses).toEqual(["n", "m"]);
  });
});

describe("modulesXAttemptRowDrag and modulesXAttemptCardDrag (MOD-041)", () => {
  const rows = 'page css([data-testid="modules-row"])';
  const cards = 'page css([data-testid="modules-board-card"])';

  it("presses the source row and releases over the target row", async () => {
    const fake = new FakePage({
      [`${rows} hasText(Alpha)`]: { count: 1, box: { x: 0, y: 0, width: 100, height: 20 } },
      [`${rows} hasText(Bravo)`]: { count: 1, box: { x: 0, y: 60, width: 100, height: 20 } },
    });
    await new WebNewDriver(fake.asPage()).modulesXAttemptRowDrag("Alpha", "Bravo");
    expect(fake.mouseDowns).toBe(1);
    expect(fake.mouseUps).toBe(1);
    expect(fake.mouseMoves[0]).toEqual({ x: 50, y: 10 });
    expect(fake.mouseMoves[fake.mouseMoves.length - 1]).toEqual({ x: 50, y: 70 });
  });

  it("does the same over board cards", async () => {
    const fake = new FakePage({
      [`${cards} hasText(Alpha)`]: { count: 1, box: { x: 0, y: 0, width: 40, height: 40 } },
      [`${cards} hasText(Bravo)`]: { count: 1, box: { x: 200, y: 0, width: 40, height: 40 } },
    });
    await new WebNewDriver(fake.asPage()).modulesXAttemptCardDrag("Alpha", "Bravo");
    expect(fake.mouseDowns).toBe(1);
    expect(fake.mouseMoves[fake.mouseMoves.length - 1]).toEqual({ x: 220, y: 20 });
  });

  it("refuses a gesture whose endpoints have no boxes", async () => {
    const fake = new FakePage({
      [`${rows} hasText(Alpha)`]: { count: 1, box: null },
      [`${rows} hasText(Bravo)`]: { count: 1 },
    });
    await expect(new WebNewDriver(fake.asPage()).modulesXAttemptRowDrag("Alpha", "Bravo")).rejects.toThrow(/boxes/);
    expect(fake.mouseDowns).toBe(0);
  });
});

describe("modulesXExportActionsPresent (MOD-041)", () => {
  it("reports a visible action in each scope", async () => {
    const list = new FakePage({
      "page css(main) role(button,/export|import|print|download/i)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(list.asPage()).modulesXExportActionsPresent("list")).toBe(true);
    const detail = new FakePage({
      "page css(main) role(link,/export|import|print|download/i)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(detail.asPage()).modulesXExportActionsPresent("detail")).toBe(true);
    const dialog = new FakePage({
      "page testid(module-dialog-form) role(menuitem,/export|import|print|download/i)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(dialog.asPage()).modulesXExportActionsPresent("dialog")).toBe(true);
  });

  it("reports nothing when only hidden or absent actions exist", async () => {
    for (const scope of ["list", "detail", "dialog"] as const) {
      for (const role of ["button", "link", "menuitem"]) {
        const fake = new FakePage({
          [`page ${scope === "dialog" ? "testid(module-dialog-form)" : "css(main)"} role(${role},/export|import|print|download/i)`]:
            { count: 1, visible: false },
        });
        expect(
          await new WebNewDriver(fake.asPage()).modulesXExportActionsPresent(scope),
          `${scope}/${role}/hidden`
        ).toBe(false);
        const empty = new FakePage({
          [`page ${scope === "dialog" ? "testid(module-dialog-form)" : "css(main)"} role(${role},/export|import|print|download/i)`]:
            { count: 0 },
        });
        expect(await new WebNewDriver(empty.asPage()).modulesXExportActionsPresent(scope), `${scope}/${role}`).toBe(
          false
        );
      }
    }
  });
});

describe("modulesDragTimelineRowToSelf (MOD-021 helper)", () => {
  it("presses the named timeline row and releases back over it", async () => {
    const fake = new FakePage({
      "page testid(gantt-sidebar-row) hasText(Alpha)": { count: 1, box: { x: 0, y: 100, width: 200, height: 20 } },
    });
    await new WebNewDriver(fake.asPage()).modulesDragTimelineRowToSelf("Alpha");
    expect(fake.mouseDowns).toBe(1);
    expect(fake.mouseUps).toBe(1);
    expect(fake.mouseMoves[0]).toEqual({ x: 100, y: 110 });
    expect(fake.mouseMoves[fake.mouseMoves.length - 1]).toEqual({ x: 100, y: 110 });
  });

  it("refuses a row with no box", async () => {
    const fake = new FakePage({ "page testid(gantt-sidebar-row) hasText(Alpha)": { count: 1, box: null } });
    await expect(new WebNewDriver(fake.asPage()).modulesDragTimelineRowToSelf("Alpha")).rejects.toThrow(/bounding box/);
  });
});
