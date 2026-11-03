// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the module links driver methods (NEWFRONT-805, MOD-035):
// the section disclosure, the empty notice, the add action, the row
// readers, the add/edit dialog controls, the row copy/edit/delete
// actions, the meta reader, the write-refusal route and the
// cross-cutting section probe.
// A fake Page answers from a path-keyed table so each case pins the hook
// a method reads or the control it activates; no browser runs here.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  text?: string;
  attrs?: Record<string, string | null>;
}

type Table = Record<string, Answer>;

function keyFor(parent: string, kind: string, args: unknown[]): string {
  const rendered = args
    .map((arg) => (arg && typeof arg === "object" ? String((arg as { name?: string }).name ?? "") : String(arg)))
    .join(",");
  return `${parent} ${kind}(${rendered})`;
}

/**
 * A Playwright-shaped fake. Locators are addressed by the chain of calls
 * that built them, and nth() keeps its index so multi-row readers can be
 * tabled per position.
 */
class FakePage {
  readonly clicks: string[] = [];
  readonly fills: string[] = [];
  readonly waits: string[] = [];
  readonly permissions: string[][] = [];
  readonly routes: Array<{ matcher: unknown; handler: (route: Route) => Promise<void> }> = [];

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
      nth: (index: number) => self.locator(`${id} nth(${index})`),
      filter: (options: { has?: Locator; hasText?: string | RegExp }) =>
        options.has !== undefined
          ? self.locator(`${id} has(${(options.has as unknown as { __id: string }).__id})`)
          : self.locator(`${id} hasText(${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string }) =>
        self.locator(`${id} role(${role},${options?.name ?? ""})`),
      getByLabel: (text: string | RegExp) => self.locator(`${id} label(${String(text)})`),
      getByText: (text: string | RegExp) => self.locator(`${id} text(${String(text)})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      innerText: async () => self.answer(id).text ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      fill: async (value: string) => {
        self.fills.push(`${id}=${value}`);
      },
      waitFor: async (options?: { state?: string }) => {
        self.waits.push(`${id} ${options?.state ?? "visible"}`);
      },
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => self.locator(keyFor("page", "testid", [testId])),
      getByRole: (role: string, options?: { name?: string }) =>
        self.locator(keyFor("page", "role", [role, options?.name ?? ""])),
      context: () => ({
        grantPermissions: async (permissions: string[]) => {
          self.permissions.push(permissions);
        },
      }),
      route: async (matcher: unknown, handler: (route: Route) => Promise<void>) => {
        self.routes.push({ matcher, handler });
      },
    };
    return page as unknown as Page;
  }
}

const SECTION = "page testid(modules-links-section)";
const TOGGLE = `${SECTION} testid(modules-links-toggle)`;
const NOTICE = `${SECTION} testid(modules-links-empty)`;
const ADD = `${SECTION} testid(modules-links-add)`;
const ROWS = `${SECTION} testid(modules-link-row)`;
const DIALOG = "page role(dialog,) hasText(/Add link|Update link/)";
const DIALOG_HEADING = `${DIALOG} role(heading,)`;
const DIALOG_URL = `${DIALOG} label(URL)`;
const DIALOG_TITLE = `${DIALOG} label(/Display title/)`;
const DIALOG_SUBMIT = `${DIALOG} testid(modules-link-submit)`;
const DIALOG_CANCEL = `${DIALOG} role(button,Cancel)`;

function rowFor(title: string): string {
  return `${ROWS} has(page role(link,${title}))`;
}

function driverFor(table: Table = {}): { driver: WebNewDriver; fake: FakePage } {
  const fake = new FakePage(table);
  return { driver: new WebNewDriver(fake.asPage()), fake };
}

describe("module links drivers", () => {
  it("reads the disclosure state from the toggle", async () => {
    const expanded = driverFor({ [TOGGLE]: { attrs: { "aria-expanded": "true" } } });
    await expect(expanded.driver.modulesDetailLinksOpen()).resolves.toBe(true);
    const collapsed = driverFor({ [TOGGLE]: { attrs: { "aria-expanded": "false" } } });
    await expect(collapsed.driver.modulesDetailLinksOpen()).resolves.toBe(false);
    const missing = driverFor({ [TOGGLE]: { count: 0 } });
    await expect(missing.driver.modulesDetailLinksOpen()).resolves.toBe(false);
  });

  it("toggles through the disclosure control", async () => {
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinksToggle();
    expect(fake.clicks).toEqual([TOGGLE]);
  });

  it("reads the empty notice only while it shows", async () => {
    const shown = driverFor({ [NOTICE]: { text: "No links added yet" } });
    await expect(shown.driver.modulesDetailLinksEmptyNotice()).resolves.toBe("No links added yet");
    const hidden = driverFor({ [NOTICE]: { count: 0 } });
    await expect(hidden.driver.modulesDetailLinksEmptyNotice()).resolves.toBeNull();
  });

  it("reports whether the add action shows", async () => {
    const shown = driverFor({ [ADD]: { visible: true } });
    await expect(shown.driver.modulesDetailLinksAddVisible()).resolves.toBe(true);
    const hidden = driverFor({ [ADD]: { count: 0 } });
    await expect(hidden.driver.modulesDetailLinksAddVisible()).resolves.toBe(false);
  });

  it("opens the dialog through the add action", async () => {
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinksAddClick();
    expect(fake.clicks).toEqual([ADD]);
    expect(fake.waits).toEqual([`${DIALOG} visible`]);
  });

  it("reads rows with their targets, skipping blank titles and bare anchors", async () => {
    const anchor = (index: number): string => `${ROWS} nth(${index}) role(link,)`;
    const { driver } = driverFor({
      [ROWS]: { count: 3 },
      [anchor(0)]: {
        text: "Launch plan",
        attrs: { href: "https://docs.example.com/launch", target: "_blank" },
      },
      [anchor(1)]: { text: "   " },
      [anchor(2)]: { text: "https://plain.example.com" },
    });
    await expect(driver.modulesDetailLinksRows()).resolves.toEqual([
      { title: "Launch plan", url: "https://docs.example.com/launch", target: "_blank" },
      { title: "https://plain.example.com", url: "", target: "" },
    ]);
  });

  it("reads no rows from an empty section", async () => {
    const { driver } = driverFor({ [ROWS]: { count: 0 } });
    await expect(driver.modulesDetailLinksRows()).resolves.toEqual([]);
  });

  it("reports the dialog by attachment", async () => {
    const open = driverFor({ [DIALOG]: { count: 1 } });
    await expect(open.driver.modulesDetailLinkDialogOpen()).resolves.toBe(true);
    const closed = driverFor({ [DIALOG]: { count: 0 } });
    await expect(closed.driver.modulesDetailLinkDialogOpen()).resolves.toBe(false);
  });

  it("reads the dialog title from its heading", async () => {
    const add = driverFor({ [DIALOG]: { count: 1 }, [DIALOG_HEADING]: { text: "Add link" } });
    await expect(add.driver.modulesDetailLinkDialogTitle()).resolves.toBe("Add link");
    const missing = driverFor({ [DIALOG]: { count: 0 } });
    await expect(missing.driver.modulesDetailLinkDialogTitle()).resolves.toBeNull();
    const bare = driverFor({ [DIALOG]: { count: 1 }, [DIALOG_HEADING]: { count: 0 } });
    await expect(bare.driver.modulesDetailLinkDialogTitle()).resolves.toBeNull();
  });

  it("fills the dialog fields and submits", async () => {
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinkFillUrl("example.com");
    await driver.modulesDetailLinkFillTitle("Docs");
    await driver.modulesDetailLinkSubmit();
    expect(fake.fills).toEqual([`${DIALOG_URL}=example.com`, `${DIALOG_TITLE}=Docs`]);
    expect(fake.clicks).toEqual([DIALOG_SUBMIT]);
  });

  it("reads the URL invalid mark", async () => {
    const invalid = driverFor({ [DIALOG_URL]: { attrs: { "aria-invalid": "true" } } });
    await expect(invalid.driver.modulesDetailLinkUrlInvalid()).resolves.toBe(true);
    const valid = driverFor({ [DIALOG_URL]: { attrs: { "aria-invalid": "false" } } });
    await expect(valid.driver.modulesDetailLinkUrlInvalid()).resolves.toBe(false);
    const missing = driverFor({ [DIALOG_URL]: { count: 0 } });
    await expect(missing.driver.modulesDetailLinkUrlInvalid()).resolves.toBe(false);
  });

  it("cancels through the dialog Cancel action", async () => {
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinkCancel();
    expect(fake.clicks).toEqual([DIALOG_CANCEL]);
    expect(fake.waits).toEqual([`${DIALOG} detached`]);
  });

  it("copies a row after taking the clipboard grant", async () => {
    const row = rowFor("Launch plan");
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinkCopy("Launch plan");
    expect(fake.permissions).toEqual([["clipboard-read", "clipboard-write"]]);
    expect(fake.clicks).toEqual([`${row} testid(modules-link-copy)`]);
  });

  it("edits a row through its edit action", async () => {
    const row = rowFor("Launch plan");
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinkEdit("Launch plan");
    expect(fake.clicks).toEqual([`${row} testid(modules-link-edit)`]);
    expect(fake.waits).toEqual([`${DIALOG} visible`]);
  });

  it("deletes a row through its delete action", async () => {
    const row = rowFor("https://plain.example.com");
    const { driver, fake } = driverFor();
    await driver.modulesDetailLinkDelete("https://plain.example.com");
    expect(fake.clicks).toEqual([`${row} testid(modules-link-delete)`]);
  });

  it("reads a row's meta line", async () => {
    const row = rowFor("Launch plan");
    const meta = `${row} text(/Added /)`;
    const present = driverFor({ [meta]: { text: "Added 2 hours ago by Ada Lovelace" } });
    await expect(present.driver.modulesDetailLinkMeta("Launch plan")).resolves.toBe(
      "Added 2 hours ago by Ada Lovelace"
    );
    const missing = driverFor({ [meta]: { count: 0 } });
    await expect(missing.driver.modulesDetailLinkMeta("Launch plan")).resolves.toBeNull();
  });

  it("refuses link writes while reads pass through", async () => {
    const { driver, fake } = driverFor();
    await driver.modulesDetailFailNextLinkWrite();
    expect(fake.routes).toHaveLength(1);
    expect(fake.routes[0]?.matcher).toBe("**/module-links/**");
    const handler = fake.routes[0]?.handler as (route: Route) => Promise<void>;
    const continued: string[] = [];
    const fulfilled: unknown[] = [];
    const reading = {
      request: () => ({ method: () => "GET" }),
      continue: async () => {
        continued.push("read");
      },
      fulfill: async () => {
        fulfilled.push("read");
      },
    } as unknown as Route;
    await handler(reading);
    const writing = {
      request: () => ({ method: () => "POST" }),
      continue: async () => {
        continued.push("write");
      },
      fulfill: async (response: unknown) => {
        fulfilled.push(response);
      },
    } as unknown as Route;
    await handler(writing);
    expect(continued).toEqual(["read"]);
    expect(fulfilled).toEqual([{ status: 500, contentType: "application/json", body: "{}" }]);
  });

  it("reports whether the links section shows", async () => {
    const shown = driverFor({ [SECTION]: { visible: true } });
    await expect(shown.driver.modulesXLinksSectionVisible()).resolves.toBe(true);
    const hidden = driverFor({ [SECTION]: { count: 0 } });
    await expect(hidden.driver.modulesXLinksSectionVisible()).resolves.toBe(false);
  });
});
