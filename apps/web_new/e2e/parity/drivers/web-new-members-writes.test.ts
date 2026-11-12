// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver manage half of the project-settings
// Members tab (NEWFRONT-579, PSET-022–026). A stub Page stands in for
// Playwright: the assertions pin the selector each method reads or clicks,
// the dialog/listbox detachment waits, and the text normalization.
import type { Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  innerText?: () => Promise<string>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  evaluate?: (fn: (node: Element) => unknown) => Promise<unknown>;
  evaluateAll?: (fn: (elements: Element[]) => unknown) => Promise<unknown>;
  filter?: (options: unknown) => StubLocator;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): StubLocator {
  const node: StubLocator = {
    count: async () => 0,
    isVisible: async () => false,
    isDisabled: async () => false,
    click: async () => undefined,
    hover: async () => undefined,
    innerText: async () => "",
    waitFor: async () => undefined,
    getAttribute: async () => null,
    evaluate: async () => false,
    evaluateAll: async () => [],
    ...overrides,
  };
  const defaults: StubLocator = {
    filter: () => node,
    first: () => node,
    nth: () => node,
    getByRole: () => node,
    getByText: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    url: () => "https://app.test/acme/settings/projects/p/members/",
    getByRole: vi.fn(() => locator()),
    getByPlaceholder: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    keyboard: { press: vi.fn(async () => undefined) },
    route: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as Page;
}

function dialogLocator(node: StubLocator): StubLocator {
  return node;
}

/**
 * A waitFor stub that resolves "attached" waits and fails "detached" ones,
 * so the driver's `.catch(() => {})` settle paths run in unit tests.
 */
function detachFails(): (options?: unknown) => Promise<unknown> {
  return async (options?: unknown) => {
    if ((options as { state?: string } | undefined)?.state === "detached") {
      throw new Error("already detached");
    }
    return undefined;
  };
}

describe("invite dialog driver", () => {
  it("opens it from the Add member control and waits for the first row", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const dialog = dialogLocator(locator({ getByRole: () => locator({ waitFor }) }));
    const page = pageOf({
      getByRole: (role: string, options?: { name?: string }) =>
        role === "button" && options?.name === "Add member" ? locator({ click }) : locator(),
      locator: (selector: string) => (selector === "[role='dialog']" ? dialog : locator()),
    });
    await new WebNewDriver(page).settingsMembersOpenInvite();
    expect(click).toHaveBeenCalled();
    expect(waitFor).toHaveBeenCalled();
  });

  it("reads the dialog heading and counts rows", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === "[role='dialog']"
          ? dialogLocator(
              locator({
                getByRole: () =>
                  locator({ count: async () => 1, first: () => locator({ isVisible: async () => true }) }),
                locator: (inner: string) =>
                  inner === '[data-testid="invite-row"]' ? locator({ count: async () => 3 }) : locator(),
              })
            )
          : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersInviteOpen()).toBe(true);
    expect(await driver.settingsMembersInviteRowCount()).toBe(3);
  });

  it("adds and removes rows through their controls", async () => {
    let rows = 1;
    const click = vi.fn(async (options?: unknown) => {
      const target = options as { name?: string } | undefined;
      void target;
    });
    const dialog = dialogLocator(
      locator({
        locator: (inner: string) => (inner === '[data-testid="invite-row"]' ? { count: async () => rows } : locator()),
        getByRole: (role: string, options?: { name?: string }) =>
          locator({
            click: vi.fn(async () => {
              if (options?.name === "Add more") rows += 1;
              if (options?.name?.startsWith("Remove row")) rows -= 1;
              await click(options);
            }),
          }),
      })
    );
    const page = pageOf({ locator: (selector: string) => (selector === "[role='dialog']" ? dialog : locator()) });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersInviteAddRow();
    expect(rows).toBe(2);
    await driver.settingsMembersInviteRemoveRow(1);
    expect(rows).toBe(1);
    expect(click).toHaveBeenCalledWith({ name: "Add more", exact: true });
    expect(click).toHaveBeenCalledWith({ name: "Remove row 2", exact: true });
  });

  it("opens a row's colleague picker only when no listbox is up", async () => {
    const pickerClick = vi.fn(async () => undefined);
    let listboxOpen = false;
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "[role='dialog']") {
          return dialogLocator(
            locator({
              locator: (inner: string) =>
                inner === "button[aria-haspopup='listbox']"
                  ? locator({ nth: () => locator({ click: pickerClick }) })
                  : locator(),
            })
          );
        }
        if (selector === "[role='listbox']") {
          return locator({ count: async () => (listboxOpen ? 1 : 0) });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersInviteOpenMemberPicker(1);
    expect(pickerClick).toHaveBeenCalledTimes(1);
    listboxOpen = true;
    await driver.settingsMembersInviteOpenMemberPicker(1);
    expect(pickerClick).toHaveBeenCalledTimes(1);
  });

  it("lists colleague and role options and picks by text", async () => {
    const optionClick = vi.fn(async () => undefined);
    const option = locator({ click: optionClick });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "[role='listbox']") {
          return locator({
            locator: () => locator({ filter: () => locator({ first: () => option }), evaluateAll: () => [] }),
            getByRole: () => option,
            waitFor: detachFails(),
          });
        }
        if (selector === "[role='dialog']") return dialogLocator(locator());
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersInvitePickMember(0, "Grace");
    expect(optionClick).toHaveBeenCalled();
    await driver.settingsMembersInvitePickRole(0, "Admin");
    expect(optionClick).toHaveBeenCalledTimes(2);
  });

  it("toggles the role picker open, reads its options and shuts it", async () => {
    const pickerClick = vi.fn(async () => undefined);
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "[role='listbox']") {
          return locator({
            count: async () => 0,
            locator: () => locator({ evaluateAll: async () => ["Guest", "Member"] }),
            getByRole: () => locator(),
            waitFor: detachFails(),
          });
        }
        if (selector === "[role='dialog']") {
          return dialogLocator(
            locator({
              locator: (inner: string) =>
                inner === "button[aria-haspopup='listbox']"
                  ? locator({ nth: () => locator({ click: pickerClick }) })
                  : locator(),
            })
          );
        }
        return locator();
      },
    });
    const options = await new WebNewDriver(page).settingsMembersInviteRoleOptions(0);
    expect(options).toEqual(["Guest", "Member"]);
    expect(pickerClick).toHaveBeenCalledTimes(2);
  });

  it("lists colleague options through the shared listbox reader", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "[role='listbox']") {
          return locator({
            count: async () => 1,
            locator: () =>
              locator({
                evaluateAll: async (fn: (elements: Element[]) => unknown) =>
                  fn([{ innerText: "  Grace Hopper " }, { innerText: "   " }, {}] as unknown as Element[]),
              }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).settingsMembersInviteMemberOptions()).resolves.toEqual(["Grace Hopper"]);
  });

  it("reads a row's inline message or reports none", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === "[role='dialog']") {
          return dialogLocator(
            locator({
              locator: (inner: string) => {
                if (inner === '[data-testid="invite-row-error-0"]') {
                  return locator({
                    count: async () => 1,
                    first: () => locator({ innerText: async () => "  Select a co-worker  " }),
                  });
                }
                if (inner === '[data-testid="invite-row-error-2"]') {
                  return locator({
                    count: async () => 1,
                    first: () =>
                      locator({
                        innerText: async () => {
                          throw new Error("gone");
                        },
                      }),
                  });
                }
                if (inner === '[data-testid="invite-row-error-3"]') {
                  return locator({
                    count: async () => 1,
                    first: () => locator({ innerText: async () => null as unknown as string }),
                  });
                }
                return locator({ count: async () => 0 });
              },
            })
          );
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersInviteRowError(0)).toBe("Select a co-worker");
    expect(await driver.settingsMembersInviteRowError(1)).toBe("");
    expect(await driver.settingsMembersInviteRowError(2)).toBe("");
    expect(await driver.settingsMembersInviteRowError(3)).toBe("");
  });

  it("submits, attempts a submit without waiting, and cancels", async () => {
    const click = vi.fn(async () => undefined);
    let cancelling = false;
    const waitFor = vi.fn(async (options?: unknown) => {
      if (cancelling) return detachFails()(options);
      return undefined;
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === "[role='dialog']"
          ? dialogLocator(locator({ getByRole: () => locator({ click }), waitFor }))
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersInviteSubmit();
    await driver.settingsMembersInviteAttemptSubmit();
    cancelling = true;
    await driver.settingsMembersInviteCancel();
    expect(click).toHaveBeenCalledTimes(3);
    expect(waitFor).toHaveBeenCalled();
  });

  it("closes an open colleague picker and reports add-control visibility", async () => {
    const pickerClick = vi.fn(async () => undefined);
    let listboxOpen = true;
    const page = pageOf({
      getByRole: (role: string, options?: { name?: string }) =>
        role === "button" && options?.name === "Add member"
          ? locator({ count: async () => 1, first: () => locator({ isVisible: async () => true }) })
          : locator(),
      locator: (selector: string) => {
        if (selector === "[role='listbox']") {
          return locator({ count: async () => (listboxOpen ? 1 : 0), waitFor: detachFails() });
        }
        if (selector === "[role='dialog']") {
          return dialogLocator(
            locator({
              locator: (inner: string) =>
                inner === "button[aria-haspopup='listbox']"
                  ? locator({ nth: () => locator({ click: pickerClick }) })
                  : locator(),
            })
          );
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersAddVisible()).toBe(true);
    await driver.settingsMembersInviteCloseMemberPicker(0);
    expect(pickerClick).toHaveBeenCalledTimes(1);
    listboxOpen = false;
    await driver.settingsMembersInviteCloseMemberPicker(0);
    expect(pickerClick).toHaveBeenCalledTimes(1);
  });
});

describe("role editor driver", () => {
  function tablePage(roleCell: StubLocator): { page: Page; row: StubLocator } {
    const row = locator({
      hover: vi.fn(async () => undefined),
      locator: (inner: string) => {
        if (inner === "td") return locator({ nth: () => roleCell });
        if (inner === "button[aria-haspopup='menu']") return locator({ click: vi.fn(async () => undefined) });
        return locator();
      },
    });
    const table = locator({ locator: () => locator({ filter: () => locator({ first: () => row }) }) });
    const page = pageOf({
      getByRole: vi.fn((role: string) => (role === "table" ? table : locator())),
      locator: vi.fn((selector: string) =>
        selector === "[role='listbox']" ? locator({ count: async () => 0 }) : locator()
      ),
    });
    return { page, row };
  }

  it("reports an editable cell and reads its text", async () => {
    const { page } = tablePage(
      locator({
        locator: () => locator({ count: async () => 1 }),
        innerText: async () => "  Member \n",
      })
    );
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersRoleEditable("Grace H")).toBe(true);
    expect(await driver.settingsMembersRoleText("Grace H")).toBe("Member");
  });

  it("reports read-only text when the cell has no picker", async () => {
    const { page } = tablePage(
      locator({ locator: () => locator({ count: async () => 0 }), innerText: async () => "Guest" })
    );
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersRoleEditable("Guest")).toBe(false);
    expect(await driver.settingsMembersRoleText("Guest")).toBe("Guest");
  });

  it("reads empty text when the role cell is gone or blank", async () => {
    const failing = tablePage(
      locator({
        innerText: async () => {
          throw new Error("gone");
        },
      })
    );
    expect(await new WebNewDriver(failing.page).settingsMembersRoleText("Grace H")).toBe("");
    const blank = tablePage(locator({ innerText: async () => null as unknown as string }));
    expect(await new WebNewDriver(blank.page).settingsMembersRoleText("Grace H")).toBe("");
  });

  it("opens the editor and picks a role", async () => {
    const cellClick = vi.fn(async () => undefined);
    const optionClick = vi.fn(async () => undefined);
    let listboxOpen = false;
    const { page } = tablePage(
      locator({ locator: () => locator({ click: cellClick }), innerText: async () => "Member" })
    );
    const baseLocator = page.locator as ReturnType<typeof vi.fn>;
    baseLocator.mockImplementation((selector: string) => {
      if (selector === "[role='listbox']") {
        return locator({
          count: async () => (listboxOpen ? 1 : 0),
          getByRole: () => locator({ click: optionClick }),
          locator: () => locator({ evaluateAll: async () => ["Guest", "Member"] }),
          waitFor: detachFails(),
        });
      }
      return locator();
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersRoleEditorOpen("Grace H");
    expect(cellClick).toHaveBeenCalledTimes(1);
    expect(await driver.settingsMembersRoleEditorOptions()).toEqual(["Guest", "Member"]);
    await driver.settingsMembersRoleEditorPick("Member");
    expect(optionClick).toHaveBeenCalledTimes(1);
    listboxOpen = true;
    await driver.settingsMembersRoleEditorOpen("Grace H");
    expect(cellClick).toHaveBeenCalledTimes(1);
  });
});

describe("row menu and confirmation driver", () => {
  it("hovers a row, opens its menu and reads the items", async () => {
    const hover = vi.fn(async () => undefined);
    const menuClick = vi.fn(async () => undefined);
    const row = locator({
      hover,
      locator: (inner: string) =>
        inner === "button[aria-haspopup='menu']" ? locator({ click: menuClick }) : locator(),
    });
    const table = locator({ locator: () => locator({ filter: () => locator({ first: () => row }) }) });
    const page = pageOf({
      getByRole: vi.fn((role: string) =>
        role === "table"
          ? table
          : locator({
              evaluateAll: async (fn: (items: Element[]) => unknown) =>
                fn([{ innerText: " Remove " }, {}] as unknown as Element[]),
            })
      ),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersOpenRowMenu("Grace Hopper");
    expect(hover).toHaveBeenCalled();
    expect(menuClick).toHaveBeenCalled();
    expect(await driver.settingsMembersRowMenuItems()).toEqual(["Remove"]);
  });

  it("picks a menu verb and submits or cancels the confirmation", async () => {
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(detachFails());
    let confirmText: string | null = " Remove Grace? ";
    let confirmFails = false;
    const page = pageOf({
      getByRole: vi.fn((role: string) =>
        role === "menuitem" ? locator({ first: () => locator({ click }) }) : locator()
      ),
      locator: (selector: string) =>
        selector === "[role='dialog']"
          ? dialogLocator(
              locator({
                getByRole: () => locator({ click }),
                waitFor,
                innerText: async () => {
                  if (confirmFails) throw new Error("gone");
                  return confirmText as string;
                },
              })
            )
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.settingsMembersRowMenuPick("Remove");
    expect(click).toHaveBeenCalledTimes(1);
    expect(await driver.settingsMembersConfirmText()).toBe("Remove Grace?");
    confirmFails = true;
    expect(await driver.settingsMembersConfirmText()).toBe("");
    confirmFails = false;
    confirmText = null;
    expect(await driver.settingsMembersConfirmText()).toBe("");
    await driver.settingsMembersConfirmSubmit();
    await driver.settingsMembersConfirmCancel();
    expect(click).toHaveBeenCalledTimes(3);
    expect(waitFor).toHaveBeenCalled();
  });
});

describe("defaults driver", () => {
  function defaultsPage(lead: StubLocator, listbox: () => StubLocator): Page {
    return pageOf({
      locator: vi.fn((selector: string) => {
        if (selector === '[data-testid="members-defaults"]') return lead;
        if (selector === "[role='listbox']") return listbox();
        if (selector === '[data-testid="members-lead"]' || selector === '[data-testid="members-assignee"]') return lead;
        if (selector === '[data-testid="members-guest-visibility"]') return lead;
        return locator();
      }),
    });
  }

  it("reads visibility and picker text", async () => {
    let pickerText: string | null = "  grace ";
    let pickerFails = false;
    const lead = locator({
      count: async () => 1,
      first: () => locator({ isVisible: async () => true }),
      innerText: async () => {
        if (pickerFails) throw new Error("gone");
        return pickerText as string;
      },
      isDisabled: async () => true,
    });
    const page = defaultsPage(lead, () => locator({ count: async () => 1 }));
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersDefaultsVisible()).toBe(true);
    expect(await driver.settingsMembersLeadText()).toBe("grace");
    expect(await driver.settingsMembersAssigneeText()).toBe("grace");
    expect(await driver.settingsMembersLeadDisabled()).toBe(true);
    pickerFails = true;
    expect(await driver.settingsMembersLeadText()).toBe("");
    expect(await driver.settingsMembersAssigneeText()).toBe("");
    pickerFails = false;
    pickerText = null;
    expect(await driver.settingsMembersLeadText()).toBe("");
    expect(await driver.settingsMembersAssigneeText()).toBe("");
  });

  it("picks a lead and an assignee when no listbox is open", async () => {
    let open = false;
    const click = vi.fn(async () => {
      open = false;
    });
    const option = locator({ click });
    const lead = locator({
      click: vi.fn(async () => {
        open = true;
      }),
    });
    const page = defaultsPage(lead, () =>
      locator({
        count: async () => (open ? 1 : 0),
        locator: () => locator({ filter: () => locator({ first: () => option }) }),
        waitFor: detachFails(),
      })
    );
    const driver = new WebNewDriver(page);
    await driver.settingsMembersLeadPick("grace");
    await driver.settingsMembersAssigneePick("None");
    expect(lead.click).toHaveBeenCalledTimes(2);
    expect(click).toHaveBeenCalledTimes(2);
  });

  it("reads and flips the guest-visibility switch", async () => {
    const click = vi.fn(async () => undefined);
    let checked: string | null = "true";
    let dataChecked = false;
    const node = {
      getAttribute: (name: string) => {
        if (name === "aria-checked") return checked;
        if (name === "data-checked") return dataChecked ? "" : null;
        return null;
      },
    };
    const page = defaultsPage(
      locator({
        evaluate: async (fn: (node: Element) => unknown) => fn(node as unknown as Element),
        click,
        isDisabled: async () => false,
      }),
      () => locator()
    );
    const driver = new WebNewDriver(page);
    expect(await driver.settingsMembersGuestToggleState()).toBe(true);
    checked = "false";
    dataChecked = true;
    expect(await driver.settingsMembersGuestToggleState()).toBe(true);
    dataChecked = false;
    expect(await driver.settingsMembersGuestToggleState()).toBe(false);
    await driver.settingsMembersGuestToggle();
    expect(click).toHaveBeenCalled();
    expect(await driver.settingsMembersGuestToggleDisabled()).toBe(false);
  });
});
