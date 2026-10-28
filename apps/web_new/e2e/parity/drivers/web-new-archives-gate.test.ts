// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the driver methods NEWFRONT-704's area gate implemented
// on top of the archives slices: the shared list row opener, the
// subscription toggle, the archived-detail entry, the list-query waiter,
// the row/detail menus and their lifecycle dialogs, the project archive
// dialog's entry, the display panel's sections, and the conditional
// filter removal. A stub Page replays each selector chain, so the
// assertions pin the gesture or the read the driver relies on.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { signInSessionRetry } from "../helpers/api";

import { WebNewDriver } from "./web-new";

vi.mock("../helpers/api", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  signInSessionRetry: vi.fn(async () => "session-id=sess-1"),
}));

interface Stub {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  getAttribute?: (name: string) => Promise<string | null>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  dispatchEvent?: (type: string, init?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  filter?: (options?: unknown) => Locator;
  or?: (other: Locator) => Locator;
  nth?: (index: number) => Locator;
  [key: string]: unknown;
}

function loc(overrides: Stub = {}): Locator {
  const node: Stub = {
    count: async () => 1,
    innerText: async () => "",
    isVisible: async () => true,
    isDisabled: async () => false,
    getAttribute: async () => null,
    click: async () => undefined,
    waitFor: async () => undefined,
    dispatchEvent: async () => undefined,
    hover: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    ...overrides,
  };
  const self = node as unknown as Locator;
  node["first"] = () => overrides.nth?.(0) ?? self;
  node["last"] = () => self;
  node["nth"] = (index: number) => overrides.nth?.(index) ?? self;
  node["locator"] = (selector: string) => overrides.locator?.(selector) ?? loc();
  node["getByTestId"] = (id: string) => overrides.getByTestId?.(id) ?? loc();
  node["getByRole"] = (role: string, options?: unknown) => overrides.getByRole?.(role, options) ?? loc();
  node["filter"] = (options?: unknown) => overrides.filter?.(options) ?? self;
  node["or"] = (other: Locator) => overrides.or?.(other) ?? self;
  return self;
}

interface PageCalls {
  gotos: string[];
  keys: string[];
  filters: unknown[];
  testIds: string[];
  permissions: string[][];
  waited: boolean[];
}

function stubPage(config: {
  url?: string | (() => string);
  locatorFor?: (selector: string, options?: unknown) => Locator;
  roleFor?: (role: string, options?: unknown) => Locator;
  testIdFor?: (id: string) => Locator;
  textFor?: (text: string) => Locator;
  responseUrl?: string | (() => string);
  context?: unknown;
}): { page: Page; calls: PageCalls } {
  const calls: PageCalls = { gotos: [], keys: [], filters: [], testIds: [], permissions: [], waited: [] };
  const page = {
    url: () => (typeof config.url === "function" ? config.url() : (config.url ?? "http://new.test/")),
    goto: async (url: string) => {
      calls.gotos.push(url);
    },
    locator: (selector: string, options?: unknown) => {
      calls.filters.push({ selector, options });
      return config.locatorFor?.(selector, options) ?? loc();
    },
    getByRole: (role: string, options?: unknown) => config.roleFor?.(role, options) ?? loc(),
    getByTestId: (id: string) => {
      calls.testIds.push(id);
      return config.testIdFor?.(id) ?? loc();
    },
    getByText: (text: string) => config.textFor?.(text) ?? loc(),
    waitForResponse: async (predicate: (response: unknown) => boolean) => {
      const url =
        typeof config.responseUrl === "function"
          ? config.responseUrl()
          : (config.responseUrl ??
            "http://new.test/api/workspaces/acme/projects/p1/archived-issues/?per_page=50&group_by=state");
      const response = { url: () => url, request: () => ({ method: () => "GET" }) };
      const accepted = predicate(response);
      calls.waited.push(accepted);
      if (!accepted) throw new Error("[test] the waiter rejected the response under test");
      return response;
    },
    waitForFunction: async () => undefined,
    waitForTimeout: async () => undefined,
    keyboard: {
      press: async (key: string) => {
        calls.keys.push(key);
      },
    },
    context: () =>
      config.context ?? {
        grantPermissions: async (permissions: string[]) => {
          calls.permissions.push(permissions);
        },
      },
  };
  return { page: page as unknown as Page, calls };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("clickListRow", () => {
  it("clicks the visible shared list row that names the item", async () => {
    const clicked: unknown[] = [];
    const { page, calls } = stubPage({
      locatorFor: () =>
        loc({
          filter: (options) => {
            calls.filters.push(options);
            return loc({ click: async (opts) => void clicked.push(opts) });
          },
        }),
    });

    await new WebNewDriver(page).clickListRow("Persist urgent");

    expect(calls.filters).toContainEqual({ hasText: "Persist urgent" });
    expect(clicked).toEqual([{ timeout: WebNewDriver.WAIT_MS }]);
  });
});

describe("subscribeToggle", () => {
  it("reads the bell's label when it shows", async () => {
    const { page } = stubPage({ roleFor: () => loc({ innerText: async () => " Unsubscribe " }) });
    expect(await new WebNewDriver(page).subscribeToggle()).toBe("Unsubscribe");
  });

  it("reports nothing when the bell is absent or unlabelled", async () => {
    const absent = stubPage({ roleFor: () => loc({ count: async () => 0 }) });
    expect(await new WebNewDriver(absent.page).subscribeToggle()).toBeNull();
    const blank = stubPage({ roleFor: () => loc({ innerText: async () => "  " }) });
    expect(await new WebNewDriver(blank.page).subscribeToggle()).toBeNull();
  });

  it("presses the bell to flip the subscription", async () => {
    const clicked: unknown[] = [];
    const { page } = stubPage({ roleFor: () => loc({ click: async (options) => void clicked.push(options) }) });

    await new WebNewDriver(page).clickSubscribeToggle();

    expect(clicked).toEqual([{ timeout: WebNewDriver.WAIT_MS }]);
  });
});

describe("openArchivedIssueDetail", () => {
  it("settles the loading body without navigating when already on the address", async () => {
    const waits: unknown[] = [];
    const { page, calls } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues/i1",
      textFor: () => loc({ count: async () => 0 }),
      locatorFor: () =>
        loc({
          getByRole: () => loc({ waitFor: async (options) => void waits.push(options) }),
        }),
    });

    await new WebNewDriver(page).openArchivedIssueDetail("acme", "p1", "i1");

    expect(calls.gotos).toEqual([]);
    expect(waits).toEqual([{ state: "hidden", timeout: 120_000 }]);
  });

  it("opens the archives address when the page is elsewhere", async () => {
    const { page, calls } = stubPage({
      url: "http://new.test/acme/projects/p1/issues",
      textFor: () => loc({ count: async () => 0 }),
    });

    await new WebNewDriver(page).openArchivedIssueDetail("acme", "p1", "i9");

    expect(calls.gotos).toEqual(["/acme/projects/p1/archives/issues/i9"]);
  });
});

describe("openArchiveProjectDialog", () => {
  it("opens the card menu on the projects list", async () => {
    const menuClicks: unknown[] = [];
    const { page, calls } = stubPage({
      textFor: () => loc({ count: async () => 0 }),
      locatorFor: () => loc(),
      roleFor: (role) =>
        role === "menuitem" ? loc({ click: async (options) => void menuClicks.push(options) }) : loc(),
      testIdFor: () => loc(),
    });

    await new WebNewDriver(page).openArchiveProjectDialog("acme", "p1");

    expect(calls.gotos).toEqual(["/acme/projects/"]);
    expect(menuClicks).toEqual([{ timeout: 30_000 }]);
  });

  it("retries past a card click the lingering tour swallows", async () => {
    let cardClicks = 0;
    let tourShown = false;
    const { page } = stubPage({
      // "Take the tour" is absent until the first card attempt fails.
      textFor: () =>
        loc({
          count: async () => (tourShown ? 1 : 0),
        }),
      locatorFor: () =>
        loc({
          // The press lands on the card's own actions control.
          locator: () =>
            loc({
              click: async () => {
                cardClicks += 1;
                if (cardClicks === 1) throw new Error("tour overlay intercepted the press");
              },
            }),
        }),
      roleFor: () => loc({ click: async () => void (tourShown = true) }),
      testIdFor: () => loc(),
    });

    await new WebNewDriver(page).openArchiveProjectDialog("acme", "p1");

    expect(cardClicks).toBe(2);
    expect(tourShown).toBe(true);
  });
});

describe("archivesWaitForListQuery", () => {
  it("answers on a list read and folds repeated parameters", async () => {
    const { page } = stubPage({
      responseUrl:
        "http://new.test/api/workspaces/acme/projects/p1/archived-issues/?per_page=50&group_by=state&filters=a&filters=b",
    });

    const query = await new WebNewDriver(page).archivesWaitForListQuery();

    expect(query.params).toEqual({ per_page: "50", group_by: "state", filters: "a,b" });
  });

  it("refuses the badge's bare read, which carries no list query", async () => {
    const { page } = stubPage({
      responseUrl: "http://new.test/api/workspaces/acme/projects/p1/archived-issues/",
    });

    await expect(new WebNewDriver(page).archivesWaitForListQuery()).rejects.toThrow("rejected the response");
  });
});

describe("row menu", () => {
  function menu(items: Array<{ title: string; note?: string; disabled?: boolean }>) {
    const clicks: string[] = [];
    const keys: string[] = [];
    const entries = items.map((item) =>
      loc({
        innerText: async () => item.title,
        getAttribute: async (name) => (name === "aria-disabled" && item.disabled === true ? "true" : null),
        locator: (selector) =>
          selector === "[data-menu-title]"
            ? loc({ count: async () => 1, innerText: async () => item.title })
            : loc({
                count: async () => (item.note === undefined ? 0 : 1),
                innerText: async () => item.note ?? "",
              }),
        click: async () => void clicks.push(item.title),
        dispatchEvent: async () => void clicks.push(`dispatch:${item.title}`),
      })
    );
    const menuItems = loc({
      count: async () => entries.length,
      nth: (index) => entries[index] ?? entries[0]!,
      filter: () => menuItems,
    });
    const stub = stubPage({
      locatorFor: () => loc(),
      testIdFor: () => loc(),
      roleFor: (role) => (role === "menuitem" ? menuItems : loc()),
    });
    const keyboard = stub.page.keyboard as unknown as { press: (key: string) => Promise<void> };
    keyboard.press = async (key: string) => {
      keys.push(key);
    };
    return { ...stub, clicks, keys };
  }

  it("lists the open menu's titles and closes it again", async () => {
    const { page, keys } = menu([{ title: "Restore" }, { title: "Copy link" }]);

    expect(await new WebNewDriver(page).layoutsRowMenuItems("Ship it")).toEqual(["Restore", "Copy link"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("Picks an entry and lets its menu close", async () => {
    const { page, clicks } = menu([{ title: "Restore" }]);

    await new WebNewDriver(page).layoutsRowMenuChoose("Ship it", "Restore");

    expect(clicks).toEqual(["Restore"]);
  });

  it("grants the clipboard before picking Copy link", async () => {
    const { page, calls } = menu([{ title: "Copy link" }]);

    await new WebNewDriver(page).layoutsRowMenuChoose("Ship it", "Copy link");

    expect(calls.permissions).toEqual([["clipboard-read", "clipboard-write"]]);
  });

  it("reports disabled and enabled entries", async () => {
    const disabled = menu([{ title: "Delete", disabled: true }]);
    expect(await new WebNewDriver(disabled.page).layoutsRowMenuItemDisabled("Ship it", "Delete")).toBe(true);
    const enabled = menu([{ title: "Delete", disabled: false }]);
    expect(await new WebNewDriver(enabled.page).layoutsRowMenuItemDisabled("Ship it", "Delete")).toBe(false);
  });

  it("reads an entry's note, and nothing when it carries none", async () => {
    const withNote = menu([{ title: "Archive", note: " Completed cycles only " }]);
    expect(await new WebNewDriver(withNote.page).layoutsRowMenuItemNote("Sprint 9", "Archive")).toBe(
      "Completed cycles only"
    );
    const without = menu([{ title: "Archive" }]);
    expect(await new WebNewDriver(without.page).layoutsRowMenuItemNote("Sprint 9", "Archive")).toBeNull();
  });

  it("refuses an entry the open menu does not carry", async () => {
    const { page } = menu([{ title: "Restore" }]);

    await expect(new WebNewDriver(page).layoutsRowMenuChoose("Ship it", "Archive")).rejects.toThrow(
      'row menu has no item "Archive"'
    );
  });
});

describe("detail and lifecycle dialogs", () => {
  it("lists the detail header menu from its own trigger", async () => {
    const keys: string[] = [];
    const menuitem = loc({
      count: async () => 1,
      locator: (selector) =>
        selector === "[data-menu-title]"
          ? loc({ count: async () => 1, innerText: async () => "Archive" })
          : loc({ count: async () => 0 }),
    });
    const { page } = stubPage({
      testIdFor: (id) => (id === "detail-menu-trigger" ? loc() : loc()),
      roleFor: (role) => (role === "menuitem" ? menuitem : loc()),
    });
    const keyboard = page.keyboard as unknown as { press: (key: string) => Promise<void> };
    keyboard.press = async (key: string) => {
      keys.push(key);
    };

    expect(await new WebNewDriver(page).layoutsDetailMenuItems()).toEqual(["Archive"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("reads a new-tab entry's address without leaving the popup loaded", async () => {
    let closed = 0;
    const popup = {
      url: () => "http://new.test/acme/projects/p1/issues/i1",
      close: async () => {
        closed += 1;
      },
    };
    const menuitem = loc({ count: async () => 1, filter: () => menuitem });
    const { page } = stubPage({
      locatorFor: () => loc(),
      testIdFor: () => loc(),
      roleFor: (role) => (role === "menuitem" ? menuitem : loc()),
      context: { waitForEvent: async () => popup },
    });

    expect(await new WebNewDriver(page).layoutsRowMenuOpenNewTabUrl("Ship it")).toBe(
      "http://new.test/acme/projects/p1/issues/i1"
    );
    expect(closed).toBe(1);
  });

  it("confirms a delete and returns once the dialog closes", async () => {
    let open = true;
    const dialog = loc({
      count: async () => (open ? 1 : 0),
      getByRole: () =>
        loc({
          click: async () => {
            open = false;
          },
        }),
    });
    const { page } = stubPage({ roleFor: () => dialog, locatorFor: () => loc({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);

    expect(await driver.layoutsDeleteModalVisible()).toBe(true);
    await driver.layoutsDeleteModalConfirm();
    expect(open).toBe(false);
  });

  it("returns on the failure toast while the dialog stays open", async () => {
    let toasts = 0;
    const dialog = loc({
      count: async () => 1,
      isVisible: async () => true,
      getByRole: () =>
        loc({
          click: async () => {
            toasts += 1;
          },
        }),
    });
    const { page } = stubPage({
      roleFor: () => dialog,
      locatorFor: () => loc({ count: async () => toasts }),
    });

    await new WebNewDriver(page).layoutsDeleteModalConfirm();

    expect(toasts).toBe(1);
  });

  it("reads the archive dialog's presence and confirms it", async () => {
    const absent = stubPage({ roleFor: () => loc({ count: async () => 0 }) });
    expect(await new WebNewDriver(absent.page).layoutsArchiveModalVisible()).toBe(false);
    let confirmed = 0;
    const { page } = stubPage({
      roleFor: () =>
        loc({
          count: async () => 1,
          isVisible: async () => true,
          getByRole: () =>
            loc({
              click: async () => {
                confirmed += 1;
              },
            }),
        }),
    });
    const driver = new WebNewDriver(page);

    expect(await driver.layoutsArchiveModalVisible()).toBe(true);
    await driver.layoutsArchiveModalConfirm();
    expect(confirmed).toBe(1);
  });
});

describe("display panel sections", () => {
  function panelPage(overrides: { pressed?: string | null; dispatch?: () => void } = {}) {
    const sections: string[] = [];
    const row = loc({
      dispatchEvent: async () => overrides.dispatch?.(),
      getAttribute: async () => overrides.pressed ?? "false",
    });
    const scope = loc({ getByRole: () => loc({ or: () => row }) });
    const { page } = stubPage({
      testIdFor: (id) => {
        if (id !== "display-options-panel") return loc({ count: async () => 0 });
        return loc({
          isVisible: async () => true,
          getByTestId: (section) => {
            sections.push(section);
            return scope;
          },
        });
      },
    });
    return { page, sections };
  }

  it("picks an ordering row from the order section", async () => {
    let dispatched = 0;
    const { page, sections } = panelPage({ dispatch: () => void (dispatched += 1) });

    await new WebNewDriver(page).setDisplayOrderBy("Start date");

    expect(sections).toEqual(["display-section-order"]);
    expect(dispatched).toBe(1);
  });

  it("toggles a property pill and reads whether it is pressed", async () => {
    let pressed = false;
    const { page, sections } = panelPage({ pressed: "true", dispatch: () => void (pressed = true) });

    const driver = new WebNewDriver(page);
    await driver.toggleDisplayProperty("Start date");
    expect(await driver.isDisplayPropertyActive("Start date")).toBe(true);
    expect(pressed).toBe(true);
    expect(sections).toEqual(["display-section-properties", "display-section-properties"]);
  });
});

describe("removeRichCondition", () => {
  it("presses the remove control at the given index", async () => {
    const indexes: number[] = [];
    const clicks: unknown[] = [];
    const remove = loc({ click: async (options) => void clicks.push(options) });
    const buttons = loc({
      nth: (index) => {
        indexes.push(index);
        return remove;
      },
    });
    const { page } = stubPage({ testIdFor: () => loc({ getByRole: () => buttons }) });

    await new WebNewDriver(page).removeRichCondition(1);

    expect(indexes).toEqual([1]);
    expect(clicks).toEqual([{ timeout: WebNewDriver.WAIT_MS }]);
  });
});

describe("rulesEnsureSignedIn on a build with no password form", () => {
  // The desktop build and a cloud build paint no credential card, so the
  // ensure-signed-in path cannot fill one; it plants a minted session and
  // lands on the workspace instead (the area gate's desktop and cloud legs).
  function entryPage(opts: { passwordField?: boolean; desktopCard?: boolean; cloudCard?: boolean; bounced?: boolean }) {
    let url = "http://new.test/";
    const cookies: { cleared: number; added: unknown[] } = { cleared: 0, added: [] };
    const gotos: string[] = [];
    const page = {
      url: () => url,
      goto: async (target: string) => {
        gotos.push(target);
        url = target === "/" && opts.bounced === true ? "http://new.test/acme/" : `http://new.test${target}`;
      },
      waitForLoadState: async () => undefined,
      waitForTimeout: async () => undefined,
      waitForURL: async () => "http://new.test/acme/",
      getByPlaceholder: (name: string) =>
        loc({ isVisible: async () => (name === "name@company.com" ? opts.passwordField === true : false) }),
      getByRole: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name ?? "";
        const visible = async () =>
          role === "heading" && name.startsWith("The desktop app") && opts.desktopCard === true;
        // `.or(...)` narrows to whichever entry screen this build paints.
        return loc({
          isVisible: visible,
          or: (other: Locator) => loc({ isVisible: async () => (await visible()) || (await other.isVisible()) }),
        });
      },
      locator: (selector: string) => {
        const visible = async () => selector.includes("Cloud sign-in") && opts.cloudCard === true;
        return loc({
          isVisible: visible,
          or: (other: Locator) => loc({ isVisible: async () => (await visible()) || (await other.isVisible()) }),
        });
      },
      getByText: () => loc({ count: async () => 0 }),
      context: () => ({
        clearCookies: async () => {
          cookies.cleared += 1;
        },
        addCookies: async (added: unknown[]) => {
          cookies.added.push(...added);
        },
      }),
    };
    return { page: page as unknown as Page, cookies, gotos };
  }

  it("plants a session on the desktop build instead of filling a form", async () => {
    const { page, cookies, gotos } = entryPage({ desktopCard: true });

    await new WebNewDriver(page).rulesEnsureSignedIn("owner@example.com", "secret", "acme");

    const minted = vi.mocked(signInSessionRetry);
    expect(minted).toHaveBeenCalledWith("owner@example.com", "secret");
    expect(cookies.cleared).toBeGreaterThanOrEqual(1);
    expect(cookies.added).toEqual([
      { name: "session-id", value: "sess-1", domain: "localhost", path: "/", httpOnly: true, sameSite: "Lax" },
    ]);
    expect(gotos).toContain("/acme/");
  });

  it("takes the same path on a cloud build, whose card has no password", async () => {
    const { page, cookies } = entryPage({ cloudCard: true });

    await new WebNewDriver(page).rulesEnsureSignedIn("owner@example.com", "secret", "acme");

    expect(cookies.added).toHaveLength(1);
  });

  it("takes the same path when a signed-out visit never paints the entry", async () => {
    const { page, cookies } = entryPage({ bounced: true });

    await new WebNewDriver(page).rulesEnsureSignedIn("owner@example.com", "secret", "acme");

    expect(cookies.added).toHaveLength(1);
  });

  it("keeps the credential form on a web build", async () => {
    const { page, cookies } = entryPage({ passwordField: true });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "isSignedOut").mockResolvedValue(true);
    const form = vi.spyOn(driver, "signInWithPassword").mockImplementation(async () => {
      (page as unknown as { url: () => string }).url = () => "http://new.test/acme/";
    });

    await driver.rulesEnsureSignedIn("owner@example.com", "secret", "acme");

    expect(form).toHaveBeenCalledWith("owner@example.com", "secret");
    expect(cookies.added).toEqual([]);
  });
});
