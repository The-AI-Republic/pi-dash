// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver members-roster methods (NEWFRONT-537,
// WSET-008/011/014). A stub Page stands in for Playwright: the assertions
// are about the address each method opens, the selector it reads, and the
// route stub it installs — never about a real browser.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  evaluateAll?: (fn: (elements: Element[]) => unknown) => Promise<unknown>;
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
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    inputValue: async () => "",
    evaluateAll: async () => [],
    ...overrides,
  };
  const defaults: StubLocator = {
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
    ...overrides,
  } as unknown as Page;
}

function handlerFor(route: ReturnType<typeof vi.fn>, pattern: string) {
  const call = route.mock.calls.find((args) => args[0] === pattern);
  if (call === undefined) throw new Error(`no route installed for ${pattern}`);
  return call[1] as (route: Route) => Promise<void>;
}

const ROSTER_ROWS = [
  { email: "a@example.com", role: "Admin", active: true },
  { email: "b@example.com", role: "Member", active: true },
  { email: "c@example.com", role: "Suspended", active: false },
];

function rosterEvaluateAllFor(rows: typeof ROSTER_ROWS) {
  return (fn: (elements: Element[]) => unknown): Promise<unknown> => {
    const fake = rows.map((row) => ({
      getAttribute: (name: string) =>
        name === "data-email" ? row.email : name === "data-active" ? String(row.active) : null,
      querySelectorAll: () => [null, null, null, { innerText: row.role }, null, null],
      querySelector: () => null,
      innerText: `${row.email} ${row.role}`,
    }));
    return Promise.resolve(fn(fake as unknown as Element[]));
  };
}

const rosterEvaluateAll = rosterEvaluateAllFor(ROSTER_ROWS);
const suspendedEvaluateAll = rosterEvaluateAllFor(ROSTER_ROWS.filter((row) => !row.active));

function stubRoute(method: string) {
  return {
    request: () => ({ method: () => method }),
    continue: vi.fn(async () => undefined),
  } as unknown as Route;
}

describe("members roster driver", () => {
  it("opens the members address and waits for rows or a settled empty state", async () => {
    const goto = vi.fn(async () => null);
    const waitForLoadState = vi.fn(async () => undefined);
    const locatorSpy = vi.fn((selector: string) =>
      selector === '[data-testid="member-row"]' ? locator({ count: async () => 1 }) : locator()
    );
    const page = pageOf({ goto, waitForLoadState, locator: locatorSpy });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsMembersOpen("acme");
    expect(goto).toHaveBeenCalledWith("/acme/settings/members");
    expect(waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    // The heading gate and the roster settle each bound their waits.
    expect(page.getByRole).toHaveBeenCalledWith("heading", { name: /^Members/ });
    expect(locatorSpy).toHaveBeenCalledWith('[data-testid="member-row"]');
  });

  it("fails loudly when the roster never settles", async () => {
    vi.useFakeTimers();
    const driver = new WebNewDriver(pageOf({ waitForTimeout: async () => undefined }));
    vi.setSystemTime(new Date(0));
    const advancing = vi.fn(async () => {
      vi.setSystemTime(new Date(Date.now() + 31_000));
    });
    const page = pageOf({
      waitForTimeout: advancing,
      locator: (selector: string) =>
        selector === '[data-testid="members-no-match"]' || selector === '[data-testid="members-load-failed"]'
          ? locator({ isVisible: async () => false })
          : locator({ count: async () => 0 }),
    });
    const failing = new WebNewDriver(page);
    await expect(failing.wsSettingsMembersOpen("acme")).rejects.toThrow("members roster never settled");
    void driver;
    vi.useRealTimers();
  });

  it("reads the count chip, emails, roles and suspended rows from the body", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="members-count"]') {
          return locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "3" });
        }
        if (selector.includes('[data-active="false"]')) {
          return locator({ count: async () => 1, evaluateAll: suspendedEvaluateAll });
        }
        if (selector.includes("member-row")) {
          return locator({ count: async () => 3, evaluateAll: rosterEvaluateAll });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsMembersCountChip()).toBe(3);
    expect(await driver.wsSettingsMembersVisibleEmails()).toEqual(["a@example.com", "b@example.com", "c@example.com"]);
    expect(await driver.wsSettingsMembersVisibleRoles()).toEqual([
      { email: "a@example.com", role: "Admin" },
      { email: "b@example.com", role: "Member" },
      { email: "c@example.com", role: "Suspended" },
    ]);
    expect(await driver.wsSettingsMembersSuspendedEmails()).toEqual(["c@example.com"]);
  });

  it("reads no-match text, the filter badge and the loader state", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="members-no-match"]') {
          return locator({
            count: async () => 1,
            isVisible: async () => true,
            innerText: async () => "  No matching members ",
          });
        }
        if (selector === '[data-testid="members-filter-active"]') {
          return locator({ count: async () => 1, isVisible: async () => true });
        }
        if (selector === '[data-testid="members-skeleton"]') {
          return locator({ count: async () => 1, isVisible: async () => true });
        }
        if (selector === '[data-testid="member-row"]') return locator({ count: async () => 0 });
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsMembersNoMatchVisible()).toBe(true);
    expect(await driver.wsSettingsMembersNoMatchText()).toBe("No matching members");
    expect(await driver.wsSettingsMembersFilterBadge()).toBe(true);
    expect(await driver.wsSettingsMembersLoaderVisible()).toBe(true);
  });

  it("reports no loader once a row paints", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="members-skeleton"]'
          ? locator({ count: async () => 1, isVisible: async () => true })
          : selector === '[data-testid="member-row"]'
            ? locator({ count: async () => 1 })
            : locator(),
    });
    expect(await new WebNewDriver(page).wsSettingsMembersLoaderVisible()).toBe(false);
  });

  it("fills and reads the persistent search", async () => {
    const fill = vi.fn(async () => undefined);
    const page = pageOf({
      getByPlaceholder: () => locator({ fill, inputValue: async () => "ada" }),
      locator: (selector: string) =>
        selector === '[data-testid="member-row"]' ? locator({ count: async () => 1 }) : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsMembersSearch("ada");
    expect(fill).toHaveBeenCalledWith("ada");
    expect(await driver.wsSettingsMembersSearchValue()).toBe("ada");
  });

  it("probes the search with Escape and keeps its value and presence", async () => {
    const fill = vi.fn(async () => undefined);
    const focus = vi.fn(async () => undefined);
    const press = vi.fn(async () => undefined);
    const page = pageOf({
      getByPlaceholder: () =>
        locator({ fill, focus, inputValue: async () => "zzz-no-match", isVisible: async () => true }),
      keyboard: { press },
    });
    const probe = await new WebNewDriver(page).wsSettingsMembersSearchEscapeProbe("zzz-no-match");
    expect(fill).toHaveBeenCalledWith("zzz-no-match", expect.anything());
    expect(focus).toHaveBeenCalled();
    expect(press).toHaveBeenCalledWith("Escape");
    expect(probe).toEqual({ value: "zzz-no-match", visible: true });
  });

  it("opens the filter panel and toggles the four role options", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: (role: string, options?: { name?: string }) => {
        if (role === "button" && options?.name === "Filters") return locator({ click });
        return locator();
      },
      locator: (selector: string) =>
        selector === '[data-testid="members-filters-panel"]'
          ? locator({
              getByRole: () =>
                locator({
                  evaluateAll: async (fn: (elements: Element[]) => unknown) =>
                    fn(
                      ["Admin", "Member", "Guest", "Suspended"].map((label) => ({
                        innerText: label,
                      })) as unknown as Element[]
                    ),
                  click,
                }),
            })
          : selector === '[data-testid="member-row"]'
            ? locator({ count: async () => 1 })
            : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsMembersOpenFilters();
    expect(click).toHaveBeenCalledTimes(1);
    expect(await driver.wsSettingsMembersFilterOptions()).toEqual(["Admin", "Member", "Guest", "Suspended"]);
    await driver.wsSettingsMembersToggleFilter("Guest");
    // The Filters trigger click plus the role toggle; the roster then
    // settles on rows, so no further clicks happen.
    expect(click).toHaveBeenCalledTimes(2);
  });

  it("closes the filter panel with Escape", async () => {
    let hidden = false;
    const press = vi.fn(async () => {
      hidden = true;
    });
    const page = pageOf({
      keyboard: { press },
      locator: (selector: string) =>
        selector === '[data-testid="members-filters-panel"]'
          ? locator({ waitFor: async () => (hidden ? undefined : new Promise(() => undefined)) })
          : locator(),
    });
    await new WebNewDriver(page).wsSettingsMembersCloseFilters();
    expect(press).toHaveBeenCalledWith("Escape");
  });

  it("reads row facts for an active member with a profile link", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector.includes("member-row")) {
          return locator({
            waitFor: async () => undefined,
            locator: (inner: string) => {
              if (inner === "td") {
                return locator({
                  nth: (index: number) =>
                    index === 0
                      ? locator({
                          locator: (inner: string) => {
                            if (inner === '[data-testid="member-full-name"]') {
                              return locator({ first: () => locator({ innerText: async () => "Ada L" }) });
                            }
                            if (inner === "[data-avatar-kind]") {
                              return locator({ first: () => locator({ getAttribute: async () => "initial" }) });
                            }
                            if (inner === "a") {
                              return locator({ count: async () => 1, getAttribute: async () => "/acme/profile/u1/" });
                            }
                            return locator();
                          },
                        })
                      : locator({
                          innerText: async () =>
                            ["", "Ada", "ada@example.com", "Member", "Email", "Jan 2, 2026"][index] ?? "",
                        }),
                });
              }
              if (inner === "[data-avatar-kind]") {
                return locator({ first: () => locator({ getAttribute: async () => "initial" }) });
              }
              if (inner === "a")
                return locator({ count: async () => 1, getAttribute: async () => "/acme/profile/u1/" });
              return locator();
            },
          });
        }
        return locator();
      },
    });
    expect(await new WebNewDriver(page).wsSettingsMemberRowFacts("ada@example.com")).toEqual({
      fullName: "Ada L",
      displayName: "Ada",
      email: "ada@example.com",
      role: "Member",
      auth: "Email",
      joiningDate: "Jan 2, 2026",
      avatarKind: "initial",
      profileHref: "/acme/profile/u1/",
    });
  });

  it("reads a suspended row as such even without a link or avatar marker", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector.includes("member-row")) {
          return locator({
            locator: (inner: string) => {
              if (inner === "td") {
                return locator({
                  nth: (index: number) =>
                    index === 0
                      ? locator({
                          locator: (inner: string) => {
                            if (inner === '[data-testid="member-full-name"]') {
                              return locator({ first: () => locator({ innerText: async () => "Gone" }) });
                            }
                            if (inner === "[data-avatar-kind]") {
                              return locator({ first: () => locator({ getAttribute: async () => null }) });
                            }
                            if (inner === "a") return locator({ count: async () => 0 });
                            return locator();
                          },
                        })
                      : locator({
                          innerText: async () => ["", "Gone", "gone@example.com", "Suspended", "", ""][index] ?? "",
                        }),
                });
              }
              if (inner === "[data-avatar-kind]") {
                return locator({ first: () => locator({ getAttribute: async () => null }) });
              }
              if (inner === "a") return locator({ count: async () => 0 });
              return locator();
            },
          });
        }
        return locator();
      },
    });
    const facts = await new WebNewDriver(page).wsSettingsMemberRowFacts("gone@example.com");
    expect(facts.role).toBe("Suspended");
    expect(facts.auth).toBeNull();
    expect(facts.joiningDate).toBeNull();
    expect(facts.avatarKind).toBe("suspended");
    expect(facts.profileHref).toBeNull();
  });

  it("holds the first members and invitations GETs when a delay is armed", async () => {
    vi.useFakeTimers();
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.wsSettingsMembersDelayNextList(5);
    const members = handlerFor(route, "**/api/workspaces/*/members/");
    const invitations = handlerFor(route, "**/api/workspaces/*/invitations/");
    const membersRoute = stubRoute("GET");
    const pending = members(membersRoute);
    const invitationsRoute = stubRoute("GET");
    const invitationsPending = invitations(invitationsRoute);
    await vi.advanceTimersByTimeAsync(10);
    await pending;
    await invitationsPending;
    expect(membersRoute.continue).toHaveBeenCalledTimes(1);
    expect(invitationsRoute.continue).toHaveBeenCalledTimes(1);
    // A second GET is not held, and other methods pass straight through.
    const second = stubRoute("GET");
    await members(second);
    expect(second.continue).toHaveBeenCalledTimes(1);
    const post = stubRoute("POST");
    await invitations(post);
    expect(post.continue).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("reports add, activity and billing entries by their markers", async () => {
    const page = pageOf({
      getByRole: (role: string) =>
        role === "button" ? locator({ count: async () => 1, isVisible: async () => true }) : locator(),
      locator: () => locator({ count: async () => 0, isVisible: async () => false }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.wsSettingsMembersAddVisible()).toBe(true);
    expect(await driver.wsSettingsMembersActivityVisible()).toBe(false);
    expect(await driver.wsSettingsMembersBillingVisible()).toBe(false);
  });
});
