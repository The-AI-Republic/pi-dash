// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver email-preference methods
// (NEWFRONT-575, NTF-024/025/031). A stub Page stands in for Playwright;
// the methods only use goto, waitForLoadState, getByTestId, getByRole,
// first, getAttribute, click, count, boundingBox, waitFor,
// waitForResponse, route and unroute.
import type { Page, Response, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";
import type { NotificationsEmailPref } from "./parity-driver";
import { WebNewDriver } from "./web-new";

function stubRoute(method: string) {
  return {
    request: () => ({ method: () => method }),
    continue: vi.fn(async () => undefined),
    fulfill: vi.fn(async (_options: unknown) => undefined),
  };
}

function stubResponse(method: string, url: string): Response {
  return { request: () => ({ method: () => method }), url: () => url } as unknown as Response;
}

function formSwitch(getAttribute: (name: string) => Promise<string | null>) {
  return {
    first: () => ({
      getByRole: (_role: string, _options?: unknown) => ({
        first: () => ({ waitFor: async (_options?: unknown) => undefined }),
        getAttribute,
        click: async (_options?: unknown) => undefined,
        boundingBox: async () => null,
      }),
    }),
  };
}

describe("notificationsOpenEmailPreferences", () => {
  function prefsPage(switchWaitFor: (options: unknown) => Promise<void>) {
    const goto = vi.fn(async (_url: unknown, _options: unknown) => undefined);
    const reload = vi.fn(async (_options: unknown) => undefined);
    const waitForLoadState = vi.fn(async (_state: unknown) => undefined);
    const page = {
      goto,
      reload,
      waitForLoadState,
      getByTestId: (id: string) => {
        expect(id).toBe("email-prefs-form");
        return {
          first: () => ({ getByRole: () => ({ first: () => ({ waitFor: switchWaitFor }) }) }),
        };
      },
    };
    return { page: page as unknown as Page, goto, reload, waitForLoadState };
  }

  it("opens the settings page and waits for the toggles", async () => {
    const switchWaitFor = vi.fn(async (_options: unknown) => undefined);
    const stub = prefsPage(switchWaitFor);
    const driver = new WebNewDriver(stub.page);

    await driver.notificationsOpenEmailPreferences();

    expect(stub.goto).toHaveBeenCalledWith("/settings/profile/notifications", { timeout: 60_000 });
    expect(stub.waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    expect(switchWaitFor).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(stub.reload).not.toHaveBeenCalled();
  });

  it("reloads once and waits again when the first visit never paints the toggles", async () => {
    let calls = 0;
    const switchWaitFor = vi.fn(async (_options: unknown) => {
      calls += 1;
      if (calls === 1) throw new Error("not painted");
    });
    const stub = prefsPage(switchWaitFor);
    const driver = new WebNewDriver(stub.page);

    await driver.notificationsOpenEmailPreferences();

    expect(stub.reload).toHaveBeenCalledWith({ timeout: 60_000 });
    expect(switchWaitFor).toHaveBeenCalledTimes(2);
  });
});

describe("notificationsEmailPreferencesLoaderShown", () => {
  function loaderPage(loaderResolves: boolean) {
    let handler!: (route: Route) => Promise<void>;
    const route = vi.fn(async (_pattern: string, installed: (route: Route) => Promise<void>) => {
      handler = installed;
    });
    const unroute = vi.fn(async (_pattern: string) => undefined);
    const page = {
      route,
      unroute,
      goto: vi.fn(async (_url: unknown, _options: unknown) => undefined),
      getByTestId: (id: string) => {
        if (id === "email-prefs-loader") {
          return {
            first: () => ({
              waitFor: () => (loaderResolves ? Promise.resolve(undefined) : Promise.reject(new Error("no loader"))),
            }),
          };
        }
        expect(id).toBe("email-prefs-form");
        return formSwitch(async () => null);
      },
    };
    return { page: page as unknown as Page, route, unroute, handler: () => handler };
  }

  it("reports the loader and delays the preferences read", async () => {
    const stub = loaderPage(true);
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsEmailPreferencesLoaderShown()).resolves.toBe(true);

    expect(stub.route).toHaveBeenCalledWith("**/api/users/me/notification-preferences/", expect.any(Function));
    expect(stub.unroute).toHaveBeenCalledWith("**/api/users/me/notification-preferences/");

    // The installed route delays GET reads but passes the rest straight through.
    const delayed = stubRoute("GET");
    await stub.handler()(delayed as unknown as Route);
    expect(delayed.continue).toHaveBeenCalledTimes(1);
    const passthrough = stubRoute("PATCH");
    await stub.handler()(passthrough as unknown as Route);
    expect(passthrough.continue).toHaveBeenCalledTimes(1);
  });

  it("reports a missed loader and still clears the route", async () => {
    const stub = loaderPage(false);
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsEmailPreferencesLoaderShown()).resolves.toBe(false);
    expect(stub.unroute).toHaveBeenCalledWith("**/api/users/me/notification-preferences/");
  });
});

describe("notificationsEmailPreferences", () => {
  it("reads every toggle state by its accessible name", async () => {
    const seen: string[] = [];
    const states: Record<string, string | null> = {
      "Property changes": "true",
      "State changes": "false",
      "Completed items only": "true",
      Comments: null,
      Mentions: "false",
    };
    const page = {
      getByTestId: (id: string) => {
        expect(id).toBe("email-prefs-form");
        return {
          first: () => ({
            getByRole: (_role: string, options: { name: string; exact: boolean }) => {
              expect(options.exact).toBe(true);
              seen.push(options.name);
              return { getAttribute: async (_attr: string) => states[options.name] ?? null };
            },
          }),
        };
      },
    };
    const driver = new WebNewDriver(page as unknown as Page);

    await expect(driver.notificationsEmailPreferences()).resolves.toEqual({
      property_change: true,
      state_change: false,
      issue_completed: true,
      comment: false,
      mention: false,
    });
    expect(seen).toEqual(["Property changes", "State changes", "Completed items only", "Comments", "Mentions"]);
  });
});

describe("notificationsEmailPreferencesToggle", () => {
  function togglePage(toasts: number) {
    let predicate!: (response: Response) => boolean;
    const click = vi.fn(async (_options: unknown) => undefined);
    let openToasts = toasts;
    const dismiss = vi.fn(async (_options: unknown) => {
      openToasts = Math.max(0, openToasts - 1);
    });
    const dismissButton = {
      first: () => ({
        count: async () => openToasts,
        click: dismiss,
      }),
    };
    const page = {
      waitForResponse: (match: (response: Response) => boolean, _options: unknown) => {
        predicate = match;
        return Promise.resolve(stubResponse("PATCH", "https://api.test/api/users/me/notification-preferences/"));
      },
      getByTestId: (id: string) => {
        if (id === "toast-stack") {
          return {
            getByRole: (_role: string, options: { name: string }) => {
              expect(options.name).toBe("Dismiss notification");
              return dismissButton;
            },
          };
        }
        expect(id).toBe("email-prefs-form");
        return {
          first: () => ({
            getByRole: (_role: string, options: { name: string; exact: boolean }) => {
              expect(options).toEqual({ name: "Comments", exact: true });
              return { click };
            },
          }),
        };
      },
    };
    return { page, click, dismiss, matches: (response: Response) => predicate(response) };
  }

  it("clicks the named toggle and waits for its save", async () => {
    const { page, click, dismiss, matches } = togglePage(0);
    const driver = new WebNewDriver(page as unknown as Page);

    const pref: NotificationsEmailPref = "comment";
    await driver.notificationsEmailPreferencesToggle(pref);

    expect(dismiss).not.toHaveBeenCalled();
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(matches(stubResponse("PATCH", "https://api.test/api/users/me/notification-preferences/"))).toBe(true);
    expect(matches(stubResponse("PATCH", "https://api.test/api/other/"))).toBe(false);
    expect(matches(stubResponse("GET", "https://api.test/api/users/me/notification-preferences/"))).toBe(false);
  });

  it("dismisses leftover toasts through their own control before pressing", async () => {
    const { page, click, dismiss } = togglePage(2);
    const driver = new WebNewDriver(page as unknown as Page);

    await driver.notificationsEmailPreferencesToggle("comment");

    expect(dismiss).toHaveBeenCalledTimes(2);
    expect(dismiss).toHaveBeenNthCalledWith(1, { timeout: 2_000 });
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("notificationsEmailPreferencesCompletedNested", () => {
  function nestedPage(count: number, stateBox: unknown, completedBox: unknown) {
    const failBox = (box: unknown) => async () => {
      if (box instanceof Error) throw box;
      return box;
    };
    const page = {
      getByTestId: (id: string) => {
        if (id === "email-prefs-completed-nest") {
          return {
            first: () => ({
              getByRole: (role: string, options?: { name: string; exact: boolean }) => {
                if (options === undefined) {
                  expect(role).toBe("switch");
                  return { count: async () => count };
                }
                expect(role).toBe("heading");
                expect(options).toEqual({ name: "Completed items only", exact: true });
                return { boundingBox: failBox(completedBox) };
              },
            }),
          };
        }
        expect(id).toBe("email-prefs-form");
        return {
          first: () => ({
            getByRole: (role: string, options: { name: string; exact: boolean }) => {
              expect(role).toBe("heading");
              expect(options).toEqual({ name: "State changes", exact: true });
              return { boundingBox: failBox(stateBox) };
            },
          }),
        };
      },
    };
    return new WebNewDriver(page as unknown as Page);
  }

  const gone = new Error("detached");

  it("rejects a nest without exactly one switch", async () => {
    await expect(nestedPage(0, { x: 0 }, { x: 10 }).notificationsEmailPreferencesCompletedNested()).resolves.toBe(
      false
    );
  });

  it("rejects a missing state heading box", async () => {
    await expect(nestedPage(1, gone, { x: 10 }).notificationsEmailPreferencesCompletedNested()).resolves.toBe(false);
  });

  it("rejects a missing completed heading box", async () => {
    await expect(nestedPage(1, { x: 0 }, gone).notificationsEmailPreferencesCompletedNested()).resolves.toBe(false);
  });

  it("accepts an indented completed heading", async () => {
    await expect(
      nestedPage(
        1,
        { x: 8, y: 0, width: 4, height: 4 },
        { x: 32, y: 0, width: 4, height: 4 }
      ).notificationsEmailPreferencesCompletedNested()
    ).resolves.toBe(true);
  });

  it("rejects a completed heading level with the state heading", async () => {
    await expect(
      nestedPage(
        1,
        { x: 8, y: 0, width: 4, height: 4 },
        { x: 8, y: 0, width: 4, height: 4 }
      ).notificationsEmailPreferencesCompletedNested()
    ).resolves.toBe(false);
  });
});

describe("notificationsFailEmailPreferenceSaves", () => {
  it("fails PATCH saves and passes the rest through", async () => {
    let handler!: (route: Route) => Promise<void>;
    const route = vi.fn(async (_pattern: string, installed: (route: Route) => Promise<void>) => {
      handler = installed;
    });
    const driver = new WebNewDriver({ route } as unknown as Page);

    await driver.notificationsFailEmailPreferenceSaves(500);

    expect(route).toHaveBeenCalledWith("**/api/users/me/notification-preferences/", expect.any(Function));
    const failed = stubRoute("PATCH");
    await handler(failed as unknown as Route);
    expect(failed.fulfill).toHaveBeenCalledWith({
      status: 500,
      body: JSON.stringify({ detail: "parity prefs failure" }),
    });
    expect(failed.continue).not.toHaveBeenCalled();
    const passed = stubRoute("GET");
    await handler(passed as unknown as Route);
    expect(passed.continue).toHaveBeenCalledTimes(1);
    expect(passed.fulfill).not.toHaveBeenCalled();
  });
});

describe("notificationsClearEmailPreferenceSaveFailure", () => {
  it("clears the preferences route", async () => {
    const unroute = vi.fn(async (_pattern: string) => undefined);
    const driver = new WebNewDriver({ unroute } as unknown as Page);

    await driver.notificationsClearEmailPreferenceSaveFailure();

    expect(unroute).toHaveBeenCalledWith("**/api/users/me/notification-preferences/");
  });
});
