// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver manage-half methods (NEWFRONT-538,
// WSET-009/010/012/013/015). A stub Page stands in for Playwright: the
// assertions pin the selector each method reads or clicks, the
// confirmation/visibility semantics, and the settled-write wait.
import type { Locator, Page, Response } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  getAttribute?: (name: string) => Promise<string | null>;
  allInnerTexts?: () => Promise<string[]>;
  evaluateAll?: (fn: (elements: Element[]) => unknown) => Promise<unknown>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  filter?: (options: unknown) => StubLocator;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 0,
    isVisible: async () => false,
    click: async () => undefined,
    fill: async () => undefined,
    innerText: async () => "",
    inputValue: async () => "",
    getAttribute: async () => null,
    allInnerTexts: async () => [],
    evaluateAll: async () => [],
    waitFor: async () => undefined,
    ...overrides,
  };
  const defaults: StubLocator = {
    filter: () => node,
    first: () => node,
    nth: () => node,
    getByRole: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

function response(status: number, method: string, url: string): Response {
  return {
    status: () => status,
    url: () => url,
    request: () => ({ method: () => method, url: () => url }),
  } as unknown as Response;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    url: () => "https://app.test/acme/settings/members",
    getByRole: vi.fn(() => locator()),
    getByPlaceholder: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    keyboard: { press: vi.fn(async () => undefined) },
    route: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => response(200, "GET", "https://app.test/api/x/")),
    evaluate: vi.fn(async () => ""),
    context: () => ({ grantPermissions: vi.fn(async () => undefined) }),
    ...overrides,
  } as unknown as Page;
}

describe("invite dialog driver", () => {
  it("opens the dialog from the Add control and waits for the first row", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: (role: string, options?: { name?: string }) =>
        role === "button" && options?.name === "Add member" ? locator({ click }) : locator(),
      locator: (selector: string) => (selector === 'input[name="emails.0.email"]' ? locator({ waitFor }) : locator()),
    });
    await new WebNewDriver(page).wsSettingsInviteOpen();
    expect(click).toHaveBeenCalled();
    expect(waitFor).toHaveBeenCalled();
  });

  it("reads visibility, row count, values and rows from the email fields", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === 'input[name="emails.0.email"]')
          return locator({
            count: async () => 1,
            isVisible: async () => true,
            inputValue: async () => "ada@example.com",
          });
        if (selector === '[data-testid="invite-row-0"]')
          return locator({ getByRole: () => locator({ innerText: async () => "Guest" }) });
        if (selector.startsWith('input[name^="emails."]')) return locator({ count: async () => 2 });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsInviteVisible()).toBe(true);
    expect(await driver.wsSettingsInviteRowCount()).toBe(2);
    expect(await driver.wsSettingsInviteRowValue(0)).toBe("ada@example.com");
    expect(await driver.wsSettingsInviteRowRole(0)).toBe("Guest");
  });

  it("fills a row and reports an inline error only while it shows", async () => {
    const fill = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === 'input[name="emails.1.email"]') return locator({ fill });
        if (selector === '[data-testid="invite-row-1"]')
          return locator({
            locator: () =>
              locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "Email is invalid" }),
          });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsInviteFillRow(1, "nope");
    expect(fill).toHaveBeenCalledWith("nope");
    expect(await driver.wsSettingsInviteRowError(1)).toBe("Email is invalid");
  });
});

describe("pending invitations driver", () => {
  it("reads the group, its count and its emails", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="pending-invitations"]')
          return locator({ count: async () => 1, isVisible: async () => true });
        if (selector === '[data-testid="pending-invitations-count"]')
          return locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "2" });
        if (selector === '[data-testid="pending-invitation-row"]')
          return locator({
            evaluateAll: async (fn) =>
              fn([
                { getAttribute: () => "ada@example.com" },
                { getAttribute: () => "grace@example.com" },
              ] as unknown as Element[]),
          });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsInvitesGroupVisible()).toBe(true);
    expect(await driver.wsSettingsInvitesCount()).toBe(2);
    expect(await driver.wsSettingsInvitesVisibleEmails()).toEqual(["ada@example.com", "grace@example.com"]);
  });

  it("reports no group or count for a member view", async () => {
    const driver = new WebNewDriver(pageOf());
    expect(await driver.wsSettingsInvitesGroupVisible()).toBe(false);
    expect(await driver.wsSettingsInvitesCount()).toBeNull();
    expect(await driver.wsSettingsInvitesExpanded()).toBe(false);
  });

  it("reads the expanded state from the toggle and flips it", async () => {
    let expanded = "true";
    const page = pageOf({
      getByRole: (role: string, options?: { name?: RegExp }) =>
        role === "button" && options?.name instanceof RegExp
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              getAttribute: async () => expanded,
              click: async () => {
                expanded = expanded === "true" ? "false" : "true";
              },
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsInvitesExpanded()).toBe(true);
    await driver.wsSettingsInvitesToggleGroup();
    expect(await driver.wsSettingsInvitesExpanded()).toBe(false);
  });

  it("reads an invitation's role and lists its options", async () => {
    const press = vi.fn(async () => undefined);
    const page = pageOf({
      keyboard: { press },
      locator: (selector: string) => {
        if (selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]')
          return locator({ getByRole: () => locator({ innerText: async () => "Member" }) });
        if (selector === '[role="option"]:visible')
          return locator({ allInnerTexts: async () => [" Guest ", "Member", "Admin"] });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsInviteRole("ada@example.com")).toBe("Member");
    expect(await driver.wsSettingsInviteRoleOptions("ada@example.com")).toEqual(["Guest", "Member", "Admin"]);
    expect(press).toHaveBeenCalledWith("Escape");
  });

  it("reads the menu entries", async () => {
    const press = vi.fn(async () => undefined);
    const page = pageOf({
      keyboard: { press },
      locator: (selector: string) =>
        selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]'
          ? locator({ getByRole: () => locator({ click: async () => undefined }) })
          : locator(),
      getByRole: (role: string) =>
        role === "menuitem" ? locator({ allInnerTexts: async () => ["Copy link", "Remove"] }) : locator(),
    });
    await expect(new WebNewDriver(page).wsSettingsInviteMenuEntries("ada@example.com")).resolves.toEqual([
      "Copy link",
      "Remove",
    ]);
  });

  it("sets an invitation role, waits for the PATCH and reads the settled label", async () => {
    const waitForResponse = vi.fn(async () =>
      response(200, "PATCH", "https://app.test/api/workspaces/acme/invitations/inv/")
    );
    let label = "Member";
    const page = pageOf({
      waitForResponse,
      locator: (selector: string) => {
        if (selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]')
          return locator({
            getByRole: () =>
              locator({
                innerText: async () => label,
                click: async () => {
                  label = "Admin";
                },
              }),
          });
        if (selector === '[role="option"]:visible') return locator({ allInnerTexts: async () => [] });
        return locator();
      },
    });
    await new WebNewDriver(page).wsSettingsInviteSetRole("ada@example.com", "Admin");
    expect(waitForResponse).toHaveBeenCalled();
    expect(label).toBe("Admin");
  });

  it("copies the invitation link from the clipboard", async () => {
    const page = pageOf({
      evaluate: vi.fn(async () => "https://app.test/invite/tok"),
      locator: (selector: string) =>
        selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]'
          ? locator({ getByRole: () => locator({ click: async () => undefined }) })
          : locator(),
    });
    await expect(new WebNewDriver(page).wsSettingsInviteCopyLink("ada@example.com")).resolves.toBe(
      "https://app.test/invite/tok"
    );
  });
});

describe("remove dialog driver", () => {
  it("reports the open dialog, its heading and its text", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'div[role="dialog"]:has(h2)'
          ? locator({
              count: async () => 1,
              innerText: async () => "Remove ada@example.com?\nThis withdraws the invitation",
              getByRole: () => locator({ count: async () => 1, innerText: async () => "Remove ada@example.com?" }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsRemoveDialogVisible()).toBe(true);
    expect(await driver.wsSettingsRemoveDialogTitle()).toBe("Remove ada@example.com?");
    expect(await driver.wsSettingsRemoveDialogText()).toContain("withdraws");
  });

  it("reports no dialog when it never mounts", async () => {
    const driver = new WebNewDriver(pageOf());
    expect(await driver.wsSettingsRemoveDialogVisible()).toBe(false);
    expect(await driver.wsSettingsRemoveDialogTitle()).toBeNull();
    expect(await driver.wsSettingsRemoveDialogText()).toBeNull();
  });
});

describe("member row driver", () => {
  it("reads role editability and options from the account-type cell", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="member-row"][data-email="ada@example.com"]'
          ? locator({
              count: async () => 1,
              getByRole: () =>
                locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "Member" }),
            })
          : selector === '[role="option"]:visible'
            ? locator({ allInnerTexts: async () => ["Guest", "Member", "Admin"] })
            : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsMemberRoleEditable("ada@example.com")).toBe(true);
    expect(await driver.wsSettingsMemberRoleOptions("ada@example.com")).toEqual(["Guest", "Member", "Admin"]);
    expect(await driver.wsSettingsMemberRoleEditable("missing@example.com")).toBe(false);
  });

  it("reads the single menu entry and opens the confirmation", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="member-row"][data-email="ada@example.com"]'
          ? locator({
              count: async () => 1,
              getByRole: () =>
                locator({ count: async () => 1, isVisible: async () => true, click: async () => undefined }),
            })
          : locator(),
      getByRole: (role: string) =>
        role === "menuitem" ? locator({ allInnerTexts: async () => ["Remove"] }) : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsMemberMenuEntry("ada@example.com")).toBe("Remove");
    await expect(driver.wsSettingsMemberOpenRemove("ada@example.com")).resolves.toBeUndefined();
  });

  it("renders no billing entry", async () => {
    expect(await new WebNewDriver(pageOf()).wsSettingsMembersBillingVisible()).toBe(false);
  });
});
