// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the intake gate driver methods (NEWFRONT-695): the
// cross-cutting delegates and probes (intakeX*, INT-018/030/031/032/034),
// the triage header/menu/dialog readers (intakeTriage*, INT-020/021/022/
// 023/025), and the shared rules entries the intake scenarios use.
// A fake Page answers from a path-keyed table so each case pins the hook
// a method reads or the control it activates; no browser runs here.
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
  enabled?: boolean;
  evaluated?: unknown;
}

type Table = Record<string, Answer>;

const ROOT = "page";

function keyFor(parent: string, kind: string, args: unknown[]): string {
  const rendered = args
    .map((arg) => {
      if (arg instanceof RegExp) return String(arg);
      if (arg && typeof arg === "object") return String((arg as { name?: string }).name ?? "");
      return String(arg);
    })
    .join(",");
  return `${parent} ${kind}(${rendered})`;
}

/**
 * A Playwright-shaped fake. Locators are addressed by the chain of calls
 * that built them, so a table key such as
 * `page testid(intake-list) testid(intake-source-pill)` answers
 * `page.getByTestId("intake-list").getByTestId("intake-source-pill")`.
 */
class FakePage {
  readonly clicks: string[] = [];
  readonly fills: string[] = [];
  readonly selects: string[] = [];
  readonly pageCalls: string[] = [];
  readonly evaluatedFns: string[] = [];
  routeHandler: ((route: unknown) => Promise<void>) | undefined;
  unrouted = 0;
  granted: string[] = [];
  clipboardText = "";
  waitForFunctionCalls = 0;
  path = "/parity-ws/projects/p1/intake";

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
      or: () => self.locator(id),
      filter: () => self.locator(id),
      and: () => self.locator(id),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(`${id} role(${role},${String(options?.name ?? "")})`),
      getByText: (text: string | RegExp) => self.locator(`${id} text(${String(text)})`),
      getByLabel: (label: string | RegExp, options?: { exact?: boolean }) =>
        self.locator(`${id} label(${String(label)}${options?.exact === true ? ",exact" : ""})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isEnabled: async () => self.answer(id).enabled ?? !(self.answer(id).disabled ?? false),
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      textContent: async () => self.answer(id).text ?? null,
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      fill: async (value: string) => {
        self.fills.push(`${id}=${value}`);
      },
      pressSequentially: async (value: string) => {
        self.fills.push(`${id}~${value}`);
      },
      press: async (key: string) => {
        self.pageCalls.push(`press:${id}:${key}`);
      },
      blur: async () => {
        self.pageCalls.push(`blur:${id}`);
      },
      focus: async () => {
        self.pageCalls.push(`focus:${id}`);
      },
      selectOption: async (value: string) => {
        self.selects.push(`${id}=${value}`);
      },
      waitFor: async () => undefined,
      evaluate: async (fn: (...args: never[]) => unknown) => {
        self.evaluatedFns.push(String(fn).slice(0, 40));
        return self.answer(id).evaluated ?? "";
      },
      evaluateAll: async () => self.answer(id).evaluated ?? [],
    };
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => self.locator(keyFor(ROOT, "testid", [testId])),
      getByRole: (role: string, options?: { name?: string | RegExp }) =>
        self.locator(keyFor(ROOT, "role", [role, options?.name ?? ""])),
      getByText: (text: string | RegExp) => self.locator(keyFor(ROOT, "text", [String(text)])),
      getByLabel: (label: string | RegExp) => self.locator(keyFor(ROOT, "label", [String(label)])),
      locator: (selector: string) => self.locator(keyFor(ROOT, "css", [selector])),
      url: () => `https://app.test${self.path}`,
      title: async () => "Parity Project - Intake",
      evaluate: async (fn: (...args: never[]) => unknown) => {
        self.evaluatedFns.push(String(fn).slice(0, 60));
        if (String(fn).includes("clipboard")) return self.clipboardText;
        return "2026-10-28";
      },
      goto: async (url: string) => {
        self.pageCalls.push(`goto:${url}`);
      },
      reload: async () => {
        self.pageCalls.push("reload");
      },
      waitForLoadState: async () => {
        self.pageCalls.push("waitForLoadState");
      },
      waitForURL: async (pattern: RegExp | string) => {
        self.pageCalls.push(`waitForURL:${String(pattern)}`);
      },
      waitForFunction: async () => {
        self.waitForFunctionCalls += 1;
      },
      waitForTimeout: async () => undefined,
      setViewportSize: async (size: { width: number; height: number }) => {
        self.pageCalls.push(`viewport:${size.width}x${size.height}`);
      },
      bringToFront: async () => {
        self.pageCalls.push("bringToFront");
      },
      keyboard: {
        press: async (key: string) => {
          self.pageCalls.push(`key:${key}`);
        },
      },
      route: async (_matcher: unknown, handler: (route: unknown) => Promise<void>) => {
        self.routeHandler = handler;
      },
      unroute: async () => {
        self.unrouted += 1;
        self.routeHandler = undefined;
      },
      context: () => ({
        grantPermissions: async (perms: string[]) => {
          self.granted = perms;
        },
      }),
      on: () => undefined,
      viewportSize: () => ({ width: 1440, height: 900 }),
    };
    return page as unknown as Page;
  }
}

function driverOn(table: Table = {}): { fake: FakePage; driver: WebNewDriver } {
  const fake = new FakePage(table);
  return { fake, driver: new WebNewDriver(fake.asPage()) };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("intakeXOpen", () => {
  it("delegates to the shell open", async () => {
    const { driver } = driverOn();
    const open = vi.spyOn(driver, "intakeShellOpen").mockResolvedValue(undefined);
    await driver.intakeXOpen("parity-ws", "p1");
    expect(open).toHaveBeenCalledWith("parity-ws", "p1");
  });
});

describe("intakeXOpenDetail", () => {
  it("deep-links onto the open tab and settles on detail or redirect", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXOpenDetail("parity-ws", "p1", "issue-9");
    expect(fake.pageCalls).toContain("goto:/parity-ws/projects/p1/intake?currentTab=open&inboxIssueId=issue-9");
    expect(fake.waitForFunctionCalls).toBe(1);
  });
});

describe("intakeXListTitles", () => {
  it("delegates to the shell titles", async () => {
    const { driver } = driverOn();
    const titles = vi.spyOn(driver, "intakeShellListTitles").mockResolvedValue(["One", "Two"]);
    expect(await driver.intakeXListTitles()).toEqual(["One", "Two"]);
    expect(titles).toHaveBeenCalledTimes(1);
  });
});

describe("intakeXActiveTab / intakeXClickTab", () => {
  it("delegates both directions to the shell tabs", async () => {
    const { driver } = driverOn();
    const active = vi.spyOn(driver, "intakeShellActiveTab").mockResolvedValue("closed");
    const click = vi.spyOn(driver, "intakeShellClickTab").mockResolvedValue(undefined);
    expect(await driver.intakeXActiveTab()).toBe("closed");
    await driver.intakeXClickTab("open");
    expect(click).toHaveBeenCalledWith("open");
    expect(active).toHaveBeenCalledTimes(1);
  });
});

describe("intakeXDetailTitle", () => {
  it("returns null when no detail renders", async () => {
    const { driver } = driverOn({ "page testid(intake-detail)": { count: 0 } });
    expect(await driver.intakeXDetailTitle()).toBeNull();
  });

  it("delegates to the detail title once the detail mounts", async () => {
    const { driver } = driverOn({ "page testid(intake-detail)": { count: 1 } });
    const title = vi.spyOn(driver, "intakeDetailTitle").mockResolvedValue("Request one");
    expect(await driver.intakeXDetailTitle()).toBe("Request one");
    expect(title).toHaveBeenCalledTimes(1);
  });
});

describe("intakeXOpenCreate / intakeXCreateFillTitle", () => {
  it("opens the create dialog from the header control", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXOpenCreate();
    expect(fake.clicks).toEqual(["page testid(intake-header-create)"]);
  });

  it("fills through the create title field", async () => {
    const { driver } = driverOn();
    const fill = vi.spyOn(driver, "intakeCreateFillTitle").mockResolvedValue(undefined);
    await driver.intakeXCreateFillTitle("A title");
    expect(fill).toHaveBeenCalledWith("A title");
  });
});

describe("intakeXCreateDuplicateDetectionVisible", () => {
  it("is true while the dialog carries the indicator", async () => {
    const { driver } = driverOn({
      "page role(dialog,New intake request) testid(intake-duplicate-indicator)": { count: 1, visible: true },
    });
    expect(await driver.intakeXCreateDuplicateDetectionVisible()).toBe(true);
  });

  it("is false when the indicator is absent", async () => {
    const { driver } = driverOn({
      "page role(dialog,New intake request) testid(intake-duplicate-indicator)": { count: 0 },
    });
    expect(await driver.intakeXCreateDuplicateDetectionVisible()).toBe(false);
  });
});

describe("intakeXDetailDuplicateDetectionVisible", () => {
  it("reads the indicator inside the detail", async () => {
    const { driver } = driverOn({
      "page testid(intake-detail) testid(intake-duplicate-indicator)": { count: 1, visible: true },
    });
    expect(await driver.intakeXDetailDuplicateDetectionVisible()).toBe(true);
  });

  it("is false when the detail carries none", async () => {
    const { driver } = driverOn({
      "page testid(intake-detail) testid(intake-duplicate-indicator)": { count: 0 },
    });
    expect(await driver.intakeXDetailDuplicateDetectionVisible()).toBe(false);
  });
});

describe("intakeXSetViewport / intakeXMobileHeaderVisible / intakeXListPaneVisible", () => {
  it("resizes and reads the mobile chrome and the pane", async () => {
    const { fake, driver } = driverOn({
      "page testid(intake-mobile-header)": { count: 1, visible: true },
      "page testid(intake-list-pane)": { count: 1, visible: true },
    });
    await driver.intakeXSetViewport(390, 844);
    expect(fake.pageCalls).toContain("viewport:390x844");
    expect(await driver.intakeXMobileHeaderVisible()).toBe(true);
    expect(await driver.intakeXListPaneVisible()).toBe(true);
  });

  it("reports the pane hidden once the overlay unmounts it", async () => {
    const { driver } = driverOn({ "page testid(intake-list-pane)": { count: 0 } });
    expect(await driver.intakeXListPaneVisible()).toBe(false);
  });
});

describe("intakeXToggleMobileSidebar", () => {
  it("clicks the Request list toggle", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXToggleMobileSidebar();
    expect(fake.clicks).toEqual(["page testid(intake-mobile-list-toggle) role(button,Request list)"]);
  });
});

describe("intakeXMobileMenuItems", () => {
  it("opens the menu, compacts its labels, and closes it again", async () => {
    const { fake, driver } = driverOn({
      "page role(menuitem,)": { count: 5, texts: ["Snooze", "  Mark as  duplicate ", "", "Delete", "Accept"] },
    });
    expect(await driver.intakeXMobileMenuItems()).toEqual(["Snooze", "Mark as duplicate", "Delete", "Accept"]);
    expect(fake.clicks).toEqual(["page testid(intake-mobile-menu) role(button,Request actions)"]);
    expect(fake.pageCalls).toContain("key:Escape");
  });
});

describe("intakeXBeginApiSpy / intakeXApiRequests", () => {
  function fakeRoute(method: string, url: string): unknown {
    return {
      request: () => ({ method: () => method, url: () => url }),
      continue: async () => undefined,
    };
  }

  it("records method and url in order and replays a copy", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXBeginApiSpy();
    expect(fake.routeHandler).toBeDefined();
    await fake.routeHandler?.(fakeRoute("GET", "https://app.test/api/workspaces/w/projects/p/intake-issues/?x=1"));
    await fake.routeHandler?.(fakeRoute("PATCH", "https://app.test/api/workspaces/w/projects/p/intake-issues/i/"));
    const first = await driver.intakeXApiRequests();
    expect(first).toEqual([
      { method: "GET", url: "https://app.test/api/workspaces/w/projects/p/intake-issues/?x=1" },
      { method: "PATCH", url: "https://app.test/api/workspaces/w/projects/p/intake-issues/i/" },
    ]);
    expect(await driver.intakeXApiRequests()).not.toBe(first);
  });

  it("unroutes the previous spy before starting again", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXBeginApiSpy();
    await driver.intakeXBeginApiSpy();
    expect(fake.unrouted).toBe(1);
    expect(await driver.intakeXApiRequests()).toEqual([]);
  });
});

describe("intakeXRefocusPage", () => {
  it("blurs, fronts, and focuses the window", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeXRefocusPage();
    expect(fake.evaluatedFns.join(" ")).toContain("blur");
    expect(fake.evaluatedFns.join(" ")).toContain("focus");
    expect(fake.pageCalls).toContain("bringToFront");
  });
});

describe("intakeXSourceIndicatorVisible", () => {
  it("is true while a row carries the source pill", async () => {
    const { driver } = driverOn({
      "page testid(intake-list)": { count: 1 },
      "page testid(intake-list) testid(intake-source-pill)": { count: 2 },
    });
    expect(await driver.intakeXSourceIndicatorVisible()).toBe(true);
  });

  it("is false with no rows or no pills", async () => {
    const { driver } = driverOn({ "page testid(intake-list)": { count: 0 } });
    expect(await driver.intakeXSourceIndicatorVisible()).toBe(false);
    const { driver: bare } = driverOn({
      "page testid(intake-list)": { count: 1 },
      "page testid(intake-list) testid(intake-source-pill)": { count: 0 },
    });
    expect(await bare.intakeXSourceIndicatorVisible()).toBe(false);
  });
});

describe("intakeCreateFillDescription", () => {
  it("targets the editor exactly, not the attach-image input", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeCreateFillDescription("Some body");
    expect(fake.clicks).toEqual(["page role(dialog,New intake request) label(Description,exact)"]);
  });
});

describe("intakeDetailCopyLink", () => {
  it("grants clipboard permission before clicking through the menu", async () => {
    const { fake, driver } = driverOn({
      "page testid(intake-detail-header) testid(intake-detail-menu)": { count: 1, visible: true },
    });
    await driver.intakeDetailCopyLink();
    expect(fake.granted).toEqual(["clipboard-read", "clipboard-write"]);
    expect(fake.clicks).toEqual([
      "page testid(intake-detail-header) testid(intake-detail-menu) role(button,Request actions)",
      "page role(menuitem,Copy work item link)",
    ]);
  });
});

describe("intakeDetailDescriptionText", () => {
  const editor = 'page testid(intake-detail) css(.ProseMirror[aria-label="Description"])';

  it("reads the scoped description editor while it is open", async () => {
    const { driver } = driverOn({
      [editor]: { count: 1, visible: true, text: "Edited body" },
    });
    expect(await driver.intakeDetailDescriptionText()).toBe("Edited body");
  });

  it("falls back to the rendered region once the editor closes", async () => {
    const { driver } = driverOn({
      [editor]: { count: 0 },
      "page testid(intake-detail) role(region,Description)": { count: 1, text: "Restored body" },
    });
    expect(await driver.intakeDetailDescriptionText()).toBe("Restored body");
  });
});

describe("intakeDetailSetDescription", () => {
  it("types through the model instead of filling the DOM", async () => {
    const { fake, driver } = driverOn({
      "page testid(intake-detail) role(button,Add a description)": { count: 0 },
    });
    await driver.intakeDetailSetDescription("New body");
    const editor = 'page testid(intake-detail) css(.ProseMirror[aria-label="Description"])';
    expect(fake.clicks).toContain(editor);
    expect(fake.pageCalls).toContain(`press:${editor}:ControlOrMeta+a`);
    expect(fake.fills).toContain(`${editor}~New body`);
    expect(fake.fills.every((entry) => entry.includes("~"))).toBe(true);
    expect(fake.pageCalls).toContain(`blur:${editor}`);
  });
});

describe("rulesOpenIntakeIssue", () => {
  it("deep-links onto the request and settles on shell and detail", async () => {
    const { fake, driver } = driverOn();
    await driver.rulesOpenIntakeIssue("parity-ws", "p1", "issue-7");
    expect(fake.pageCalls).toContain("goto:/parity-ws/projects/p1/intake?inboxIssueId=issue-7");
  });
});

describe("rulesReadClipboard", () => {
  it("grants clipboard permission and reads the text", async () => {
    const { fake, driver } = driverOn();
    fake.clipboardText = "https://app.test/parity-ws/browse/P-1/";
    expect(await driver.rulesReadClipboard()).toBe("https://app.test/parity-ws/browse/P-1/");
    expect(fake.granted).toEqual(["clipboard-read", "clipboard-write"]);
  });
});

describe("rulesIntakeTriageVisible", () => {
  it("requires both triage buttons visible", async () => {
    const { driver } = driverOn({
      "page testid(intake-triage-accept)": { count: 1, visible: true },
      "page testid(intake-triage-decline)": { count: 1, visible: true },
    });
    expect(await driver.rulesIntakeTriageVisible()).toBe(true);
  });

  it("is false when decline hides", async () => {
    const { driver } = driverOn({
      "page testid(intake-triage-accept)": { count: 1, visible: true },
      "page testid(intake-triage-decline)": { count: 1, visible: false },
    });
    expect(await driver.rulesIntakeTriageVisible()).toBe(false);
  });
});

describe("intakeTriageAcceptVisible / intakeTriageDeclineVisible", () => {
  it("reads the header buttons", async () => {
    const { driver } = driverOn({
      "page testid(intake-triage-accept)": { count: 1, visible: true },
      "page testid(intake-triage-decline)": { count: 0 },
    });
    expect(await driver.intakeTriageAcceptVisible()).toBe(true);
    expect(await driver.intakeTriageDeclineVisible()).toBe(false);
  });
});

describe("intakeTriageClickAccept / intakeTriageClickDecline", () => {
  it("clicks without waiting on the dialog (a member is refused instead)", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeTriageClickAccept();
    await driver.intakeTriageClickDecline();
    expect(fake.clicks).toEqual(["page testid(intake-triage-accept)", "page testid(intake-triage-decline)"]);
  });
});

describe("intakeTriageAcceptDialogText", () => {
  it("reads the heading and the confirm label", async () => {
    const base = "page role(dialog,/to project work items/)";
    const { driver } = driverOn({
      [base]: { count: 1, visible: true },
      [`${base} role(heading,)`]: { text: "Add P-1 to project work items" },
      [`${base} role(button,Add to project)`]: { text: "Add to project" },
    });
    expect(await driver.intakeTriageAcceptDialogText()).toEqual({
      heading: "Add P-1 to project work items",
      confirm: "Add to project",
    });
  });
});

describe("intakeTriageOverflowOptions", () => {
  it("reads the open menu and closes it with Escape", async () => {
    const { fake, driver } = driverOn({
      "page role(menuitem,)": { count: 4, texts: ["Snooze", "Mark as duplicate", "Copy work item link", "Delete"] },
    });
    expect(await driver.intakeTriageOverflowOptions()).toEqual([
      "Snooze",
      "Mark as duplicate",
      "Copy work item link",
      "Delete",
    ]);
    expect(fake.clicks).toEqual([]);
    expect(fake.pageCalls).toContain("key:Escape");
  });

  it("opens the menu first when its entries are absent", async () => {
    const table: Table = { "page role(menuitem,)": { count: 0, texts: [] } };
    const { fake, driver } = driverOn(table);
    await driver.intakeTriageOverflowOptions();
    expect(fake.clicks).toEqual(["page testid(intake-detail-menu) role(button,Request actions)"]);
  });
});

describe("intakeTriageStatusChipText", () => {
  it("reads the chip text and nulls an empty chip", async () => {
    const { driver } = driverOn({ "page testid(intake-status-chip)": { count: 1, text: "3 days to go" } });
    expect(await driver.intakeTriageStatusChipText()).toBe("3 days to go");
    const { driver: missing } = driverOn({ "page testid(intake-status-chip)": { count: 0 } });
    expect(await missing.intakeTriageStatusChipText()).toBeNull();
  });
});

describe("intakeTriageSnoozePastDisabled", () => {
  const grid = "page role(dialog,Snooze request) testid(intake-snooze-calendar-grid)";

  it("passes when every past day disables", async () => {
    // One past day button: data-day sorts before the fake today.
    const { driver } = driverOn({
      [`${grid} css(button[data-day])`]: { count: 1, attrs: { "data-day": "2026-10-01" }, disabled: true },
    });
    expect(await driver.intakeTriageSnoozePastDisabled()).toBe(true);
  });

  it("fails when a past day stays enabled", async () => {
    const fake = new FakePage({
      [`${grid} css(button[data-day])`]: { count: 1, attrs: { "data-day": "2026-10-01" }, disabled: false },
    });
    expect(await new WebNewDriver(fake.asPage()).intakeTriageSnoozePastDisabled()).toBe(false);
  });
});

describe("intakeTriageSnoozePickFuture", () => {
  it("jumps the caption selects and clicks the target day", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeTriageSnoozePickFuture(7);
    const target = new Date();
    target.setDate(target.getDate() + 7);
    const pad = (part: number): string => String(part).padStart(2, "0");
    const iso = `${target.getFullYear()}-${pad(target.getMonth() + 1)}-${pad(target.getDate())}`;
    const base = "page role(dialog,Snooze request)";
    expect(fake.selects).toEqual([
      `${base} role(combobox,Month)=${target.getMonth()}`,
      `${base} role(combobox,Year)=${target.getFullYear()}`,
    ]);
    expect(fake.clicks).toEqual([`${base} css(button[data-day="${iso}"])`]);
  });
});

describe("intakeTriageDuplicateOptions / intakeTriageDuplicateOfText", () => {
  it("compacts option labels and reads the reference link", async () => {
    const base = "page role(dialog,Mark as duplicate)";
    const { driver } = driverOn({
      [`${base} testid(intake-duplicate-option)`]: { texts: ["N259-4   Target one", ""] },
      "page testid(intake-duplicate-of)": { count: 1, text: "N259-4" },
    });
    expect(await driver.intakeTriageDuplicateOptions()).toEqual(["N259-4 Target one"]);
    expect(await driver.intakeTriageDuplicateOfText()).toBe("N259-4");
  });

  it("nulls the reference while the request is not a duplicate", async () => {
    const { driver } = driverOn({ "page testid(intake-duplicate-of)": { count: 0 } });
    expect(await driver.intakeTriageDuplicateOfText()).toBeNull();
  });
});

describe("intakeTriageDuplicateSearch", () => {
  it("fills the picker search and settles on options or the empty state", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeTriageDuplicateSearch("target");
    expect(fake.fills).toEqual(["page role(dialog,Mark as duplicate) role(searchbox,Search work items)=target"]);
  });
});

describe("intakeTriageCurrentIssueId / intakeTriageCurrentTab", () => {
  it("reads the intake URL params", async () => {
    const fake = new FakePage();
    fake.path = "/parity-ws/projects/p1/intake?currentTab=closed&inboxIssueId=issue-3";
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.intakeTriageCurrentIssueId()).toBe("issue-3");
    expect(await driver.intakeTriageCurrentTab()).toBe("closed");
  });

  it("nulls missing params", async () => {
    const { driver } = driverOn();
    expect(await driver.intakeTriageCurrentIssueId()).toBeNull();
    expect(await driver.intakeTriageCurrentTab()).toBeNull();
  });
});

describe("intakeTriageFilterOpen / intakeTriageFilterToggleStatus / intakeTriageFilterClose", () => {
  it("delegates to the list filter panel", async () => {
    const { driver } = driverOn();
    const open = vi.spyOn(driver, "intakeListOpenFilters").mockResolvedValue(undefined);
    const pick = vi.spyOn(driver, "intakeListFilterPick").mockResolvedValue(undefined);
    const close = vi.spyOn(driver, "intakeListCloseFilters").mockResolvedValue(undefined);
    await driver.intakeTriageFilterOpen();
    await driver.intakeTriageFilterToggleStatus("Snoozed");
    await driver.intakeTriageFilterClose();
    expect(open).toHaveBeenCalledTimes(1);
    expect(pick).toHaveBeenCalledWith("status", "Snoozed");
    expect(close).toHaveBeenCalledTimes(1);
  });
});

describe("intakeTriageListOpenIssue", () => {
  it("delegates to the detail list selection", async () => {
    const { driver } = driverOn();
    const select = vi.spyOn(driver, "intakeDetailSelectListItem").mockResolvedValue(undefined);
    await driver.intakeTriageListOpenIssue("Request A");
    expect(select).toHaveBeenCalledWith("Request A");
  });
});

describe("intakeTriageOpenIssueOnTab", () => {
  it("addresses the tab and the request in one deep link", async () => {
    const { fake, driver } = driverOn();
    await driver.intakeTriageOpenIssueOnTab("parity-ws", "p1", "issue-5", "closed");
    expect(fake.pageCalls).toContain("goto:/parity-ws/projects/p1/intake?currentTab=closed&inboxIssueId=issue-5");
  });
});
