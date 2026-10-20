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

  it("sets a row's role through its own picker and drops the option list after", async () => {
    const click = vi.fn(async () => undefined);
    const optionClick = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="invite-row-1"]')
          return locator({
            getByRole: (role: string, options?: { name?: string }) =>
              role === "button" && options?.name === "Invitee role" ? locator({ click }) : locator(),
          });
        if (selector === '[role="option"]:visible') return locator({ filter: () => locator({ click: optionClick }) });
        return locator();
      },
    });
    await new WebNewDriver(page).wsSettingsInviteSetRowRole(1, "Admin");
    expect(click).toHaveBeenCalled();
    expect(optionClick).toHaveBeenCalled();
  });

  it("lists a row's roles, closes the list and trims the labels", async () => {
    const press = vi.fn(async () => undefined);
    const page = pageOf({
      keyboard: { press },
      locator: (selector: string) => {
        if (selector === '[data-testid="invite-row-0"]')
          return locator({ getByRole: () => locator({ click: async () => undefined }) });
        if (selector === '[role="option"]:visible')
          return locator({ allInnerTexts: async () => [" Guest", "Member ", "  "] });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).wsSettingsInviteRowRoles(0)).resolves.toEqual(["Guest", "Member"]);
    expect(press).toHaveBeenCalledWith("Escape");
  });

  it("closes the dialog from Cancel and waits for the first field to go", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'div[role="dialog"]:has(h2)'
          ? locator({ getByRole: () => locator({ click }) })
          : selector === 'input[name="emails.0.email"]'
            ? locator({ waitFor })
            : locator(),
    });
    await new WebNewDriver(page).wsSettingsInviteClose();
    expect(click).toHaveBeenCalled();
    expect(waitFor).toHaveBeenCalledWith(expect.objectContaining({ state: "hidden" }));
  });

  it("adds a row and waits for the count to grow", async () => {
    let rows = 1;
    const page = pageOf({
      waitForTimeout: vi.fn(async () => {
        rows += 1;
      }),
      locator: (selector: string) => {
        if (selector === 'div[role="dialog"]:has(h2)')
          return locator({ getByRole: () => locator({ click: async () => undefined }) });
        if (selector.startsWith('input[name^="emails."]')) return locator({ count: async () => rows });
        return locator();
      },
    });
    await new WebNewDriver(page).wsSettingsInviteAddRow();
    expect(rows).toBe(2);
  });

  it("submits, closes on success and waits for the field to go", async () => {
    let submitted = false;
    const hiddenWait = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === 'div[role="dialog"]:has(h2)')
          return locator({
            getByRole: () =>
              locator({
                click: async () => {
                  submitted = true;
                },
              }),
          });
        if (selector === 'input[name="emails.0.email"]')
          return locator({ count: async () => 1, isVisible: async () => true, waitFor: hiddenWait });
        if (selector === '[data-testid="toast-notification"]')
          return locator({
            count: async () => (submitted ? 1 : 0),
            isVisible: async () => true,
            innerText: async () => "Invitations sent\nInvitations sent. Each invitee will receive their own email.",
          });
        return locator();
      },
    });
    await new WebNewDriver(page).wsSettingsInviteSubmit();
    expect(hiddenWait).toHaveBeenCalled();
  });

  it("reports the submit that keeps the dialog with a new inline error", async () => {
    let submitted = false;
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === 'div[role="dialog"]:has(h2)')
          return locator({
            getByRole: () =>
              locator({
                click: async () => {
                  submitted = true;
                },
              }),
            locator: () => locator({ count: async () => (submitted ? 1 : 0) }),
          });
        if (selector === 'input[name="emails.0.email"]')
          return locator({ count: async () => 1, isVisible: async () => true });
        if (selector === '[data-testid="toast-notification"]') return locator({ count: async () => 0 });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).wsSettingsInviteSubmit()).resolves.toBeUndefined();
  });

  it("returns on a failure toast that changes nothing else", async () => {
    let attempted = false;
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === 'div[role="dialog"]:has(h2)')
          return locator({
            getByRole: () =>
              locator({
                click: async () => {
                  attempted = true;
                },
              }),
            locator: () => locator({ count: async () => 0 }),
          });
        if (selector === 'input[name="emails.0.email"]')
          return locator({ count: async () => 1, isVisible: async () => true });
        if (selector === '[data-testid="toast-notification"]')
          return locator({
            count: async () => (attempted ? 1 : 0),
            isVisible: async () => true,
            innerText: async () => "Error!\nThe invitations could not be sent.",
          });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).wsSettingsInviteSubmit()).resolves.toBeUndefined();
  });

  it("gives up when a submit neither closes, toasts nor flags a row", async () => {
    const now = vi.spyOn(Date, "now");
    let clock = 1_000;
    now.mockImplementation(() => {
      clock += 20_000;
      return clock;
    });
    const page = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      locator: (selector: string) => {
        if (selector === 'div[role="dialog"]:has(h2)')
          return locator({
            getByRole: () => locator({ click: async () => undefined }),
            locator: () => locator({ count: async () => 0 }),
          });
        if (selector === 'input[name="emails.0.email"]')
          return locator({ count: async () => 1, isVisible: async () => true });
        if (selector === '[data-testid="toast-notification"]') return locator({ count: async () => 0 });
        return locator();
      },
    });
    try {
      await expect(new WebNewDriver(page).wsSettingsInviteSubmit()).rejects.toThrow(/never closed/);
    } finally {
      now.mockRestore();
    }
  });

  it("removes a row and never polls below one row", async () => {
    let rows = 2;
    const waitForTimeout = vi.fn(async () => {
      rows = Math.max(rows - 1, 1);
    });
    const page = pageOf({
      waitForTimeout,
      locator: (selector: string) => {
        if (selector === '[data-testid="invite-row-0"]')
          return locator({ getByRole: () => locator({ click: async () => undefined }) });
        if (selector.startsWith('input[name^="emails."]')) return locator({ count: async () => rows });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsInviteRemoveRow(0);
    expect(rows).toBe(1);
    await driver.wsSettingsInviteRemoveRow(0);
    expect(rows).toBe(1);
    expect(waitForTimeout).toHaveBeenCalled();
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

  it("waits for the pending toggle to flip and gives up when it never does", async () => {
    let expanded = "true";
    const page = pageOf({
      getByRole: (role: string, options?: { name?: RegExp }) =>
        role === "button" && options?.name instanceof RegExp
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              getAttribute: async () => expanded,
              click: async () => {
                expanded = "false";
              },
            })
          : locator(),
    });
    await new WebNewDriver(page).wsSettingsInvitesToggleGroup();
    expect(expanded).toBe("false");

    const stuck = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      getByRole: (role: string, options?: { name?: RegExp }) =>
        role === "button" && options?.name instanceof RegExp
          ? locator({ count: async () => 1, isVisible: async () => true, getAttribute: async () => "true" })
          : locator(),
    });
    const now = vi.spyOn(Date, "now");
    let clock = 1_000;
    now.mockImplementation(() => {
      clock += 20_000;
      return clock;
    });
    try {
      await expect(new WebNewDriver(stuck).wsSettingsInvitesToggleGroup()).rejects.toThrow(/never finished toggling/);
    } finally {
      now.mockRestore();
    }
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

  it("opens an invitation's remove confirmation from its menu", async () => {
    const menuClick = vi.fn(async () => undefined);
    const removeClick = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]'
          ? locator({ getByRole: () => locator({ click: menuClick }) })
          : selector === 'div[role="dialog"]:has(h2)'
            ? locator({ waitFor })
            : locator(),
      getByRole: (role: string, options?: { name?: string }) =>
        role === "menuitem" && options?.name === "Remove"
          ? locator({ click: removeClick })
          : role === "menuitem"
            ? locator({ waitFor })
            : locator(),
    });
    await new WebNewDriver(page).wsSettingsInviteOpenRemove("ada@example.com");
    expect(menuClick).toHaveBeenCalled();
    expect(removeClick).toHaveBeenCalled();
    expect(waitFor).toHaveBeenCalled();
  });

  it("sets an invitation role, tolerates an error toast and gives up when nothing settles", async () => {
    let label = "Member";
    let toasts = 0;
    const page = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      waitForResponse: vi.fn(async (predicate: (r: Response) => boolean) => {
        expect(predicate(response(200, "PATCH", "https://app.test/api/workspaces/acme/invitations/inv/"))).toBe(true);
        throw new Error("refused");
      }),
      locator: (selector: string) => {
        if (selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]')
          return locator({
            getByRole: () =>
              locator({
                innerText: async () => label,
                click: async () => undefined,
              }),
          });
        if (selector === '[role="option"]:visible')
          return locator({ filter: () => locator({ click: async () => undefined }) });
        if (selector === '[data-testid="toast-notification"]')
          return locator({
            count: async () => (toasts > 0 ? 1 : 0),
            isVisible: async () => true,
            innerText: async () => "Error!\nThe invitation role could not be changed.",
          });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    // A refused write returns on the error toast, leaving the old label.
    toasts = 1;
    await expect(driver.wsSettingsInviteSetRole("ada@example.com", "Admin")).resolves.toBeUndefined();
    expect(label).toBe("Member");

    // A success settles onto the new label.
    toasts = 0;
    label = "Admin";
    await expect(driver.wsSettingsInviteSetRole("ada@example.com", "Admin")).resolves.toBeUndefined();

    // Neither settling nor a toast ends with the timeout error.
    label = "Member";
    const now = vi.spyOn(Date, "now");
    let clock = 1_000;
    now.mockImplementation(() => {
      clock += 20_000;
      return clock;
    });
    try {
      await expect(driver.wsSettingsInviteSetRole("ada@example.com", "Admin")).rejects.toThrow(/never settled/);
    } finally {
      now.mockRestore();
    }
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

  it("gives up when copy-link never reaches the clipboard", async () => {
    const now = vi.spyOn(Date, "now");
    let clock = 1_000;
    now.mockImplementation(() => {
      clock += 20_000;
      return clock;
    });
    const page = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      evaluate: vi.fn(async () => ""),
      locator: (selector: string) =>
        selector === '[data-testid="pending-invitation-row"][data-email="ada@example.com"]'
          ? locator({ getByRole: () => locator({ click: async () => undefined }) })
          : locator(),
    });
    try {
      await expect(new WebNewDriver(page).wsSettingsInviteCopyLink("ada@example.com")).rejects.toThrow(
        /never wrote the clipboard/
      );
    } finally {
      now.mockRestore();
    }
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

  it("confirms, accepts a dialog that stays mounted and cancels back out", async () => {
    const confirmClick = vi.fn(async () => undefined);
    const cancelClick = vi.fn(async () => undefined);
    let detached = false;
    const waitFor = vi.fn(async () => {
      if (!detached) throw new Error("still open");
      return undefined;
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === 'div[role="dialog"]:has(h2)'
          ? locator({
              waitFor,
              getByRole: (role: string, options?: { name?: RegExp | string }) => {
                if (options?.name instanceof RegExp) {
                  if (options.name.test("Remove") === false) return locator();
                  return locator({ click: confirmClick });
                }
                return locator({ click: cancelClick });
              },
            })
          : locator(),
      waitForResponse: vi.fn(async (predicate: (r: Response) => boolean) => {
        expect(predicate(response(204, "DELETE", "https://app.test/api/workspaces/acme/members/mem/"))).toBe(true);
        expect(predicate(response(200, "POST", "https://app.test/api/workspaces/acme/members/leave/"))).toBe(true);
        expect(predicate(response(200, "PATCH", "https://app.test/api/workspaces/acme/members/mem/"))).toBe(false);
        return response(204, "DELETE", "https://app.test/api/workspaces/acme/members/mem/");
      }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsRemoveDialogConfirm()).resolves.toBeUndefined();
    expect(confirmClick).toHaveBeenCalled();

    detached = true;
    await expect(driver.wsSettingsRemoveDialogCancel()).resolves.toBeUndefined();
    expect(cancelClick).toHaveBeenCalled();
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

  it("sets a member's role, waits for the PATCH and reads the settled cell", async () => {
    const waitForResponse = vi.fn(async () =>
      response(200, "PATCH", "https://app.test/api/workspaces/acme/members/mem/")
    );
    let role = "Member";
    const page = pageOf({
      waitForResponse,
      locator: (selector: string) => {
        if (selector === '[data-testid="member-row"][data-email="ada@example.com"]')
          return locator({
            getByRole: () => locator({ click: async () => undefined }),
            locator: () => locator({ nth: () => locator({ innerText: async () => role }) }),
          });
        if (selector === '[role="option"]:visible')
          return locator({
            filter: () =>
              locator({
                click: async () => {
                  role = "Admin";
                },
              }),
          });
        return locator();
      },
    });
    await new WebNewDriver(page).wsSettingsMemberSetRole("ada@example.com", "Admin");
    expect(waitForResponse).toHaveBeenCalled();
    expect(role).toBe("Admin");
  });

  it("returns when a refused role change leaves the row and shows an error toast", async () => {
    let toastShown = false;
    const waitForResponse = vi.fn(async (predicate: (r: Response) => boolean) => {
      const matches = predicate(response(200, "PATCH", "https://app.test/api/workspaces/acme/members/mem/"));
      expect(matches).toBe(true);
      expect(predicate(response(200, "GET", "https://app.test/api/workspaces/acme/members/mem/"))).toBe(false);
      toastShown = true;
      throw new Error("patch refused");
    });
    const page = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      waitForResponse,
      locator: (selector: string) => {
        if (selector === '[data-testid="member-row"][data-email="ada@example.com"]')
          return locator({
            getByRole: () => locator({ click: async () => undefined }),
            locator: () => locator({ nth: () => locator({ innerText: async () => "Member" }) }),
          });
        if (selector === '[role="option"]:visible')
          return locator({ filter: () => locator({ click: async () => undefined }) });
        if (selector === '[data-testid="toast-notification"]')
          return locator({
            count: async () => (toastShown ? 1 : 0),
            isVisible: async () => true,
            innerText: async () => "Error!\nThe member's role could not be changed.",
          });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).wsSettingsMemberSetRole("ada@example.com", "Admin")).resolves.toBeUndefined();
    expect(waitForResponse).toHaveBeenCalled();
  });

  it("gives up when a role change neither settles nor raises an error", async () => {
    const now = vi.spyOn(Date, "now");
    let clock = 1_000;
    now.mockImplementation(() => {
      clock += 20_000;
      return clock;
    });
    const page = pageOf({
      waitForTimeout: vi.fn(async () => undefined),
      waitForResponse: vi.fn(async () => {
        throw new Error("no patch");
      }),
      locator: (selector: string) => {
        if (selector === '[data-testid="member-row"][data-email="ada@example.com"]')
          return locator({
            getByRole: () => locator({ click: async () => undefined }),
            locator: () => locator({ nth: () => locator({ innerText: async () => "Member" }) }),
          });
        if (selector === '[role="option"]:visible')
          return locator({ filter: () => locator({ click: async () => undefined }) });
        if (selector === '[data-testid="toast-notification"]') return locator({ count: async () => 0 });
        return locator();
      },
    });
    try {
      await expect(new WebNewDriver(page).wsSettingsMemberSetRole("ada@example.com", "Admin")).rejects.toThrow(
        /member role never settled/
      );
    } finally {
      now.mockRestore();
    }
  });

  it("renders no billing entry", async () => {
    expect(await new WebNewDriver(pageOf()).wsSettingsMembersBillingVisible()).toBe(false);
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="members-billing"]'
          ? locator({ count: async () => 1, isVisible: async () => true })
          : locator(),
    });
    expect(await new WebNewDriver(page).wsSettingsMembersBillingVisible()).toBe(true);
  });
});
