// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-439): catalog search and paging, row
// CLOUD-023 (search filters after a short debounce and resets paging;
// previous/next step through cursor windows with a page indicator),
// green on the cloud-edition old app first. The same file must go green
// on apps/web_new once the cloud Apps area lands there.
//
// Observed behavior notes (cloud oracle): the initial catalog read
// carries only a limit; typing filters after a short debounce (one
// request per settled query, never one per keystroke) and drops any
// held cursor, so a mid-window query change can never mix result sets;
// the pager walks cursor windows with a page indicator, and revisiting
// a window re-reads it without a cursor.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { installOpenhubStub, marketplacePage, stubApp } from "./openhub-stub";

function marketCalls(stub: { log: { method: string; path: string }[] }): URLSearchParams[] {
  return stub.log
    .filter((entry) => entry.method === "GET" && entry.path.startsWith("/api/openhub/apps/marketplace/"))
    .map((entry) => new URL(entry.path, "http://oracle.invalid").searchParams);
}

test(
  specTitle(["CLOUD-023"], "typing filters the catalog once per settled query"),
  { tag: specTags(["CLOUD-023"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: (_route, logged) => {
        const query = new URL(logged.path, "http://oracle.invalid").searchParams.get("q") ?? "";
        const items =
          query === ""
            ? [
                stubApp({ appId: "app-notes", slug: "notes", name: "Notes Pro" }),
                stubApp({ appId: "app-crm", slug: "crm", name: "CRM Thing" }),
              ]
            : [stubApp({ appId: "app-notes", slug: "notes", name: "Notes Pro" })];
        return Promise.resolve({ status: 200, body: marketplacePage(items) });
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Notes Pro", "CRM Thing"]);
      // The initial read carries only a limit.
      const initial = marketCalls(stub);
      expect(initial.length).toBe(1);
      expect(initial[0]?.get("limit")).toBe("24");
      expect(initial[0]?.get("q")).toBeNull();
      expect(initial[0]?.get("cursor")).toBeNull();

      // Six keystrokes, one request: the debounce collapses the burst.
      await driver.cloudAppsSearchTypeText("notes");
      await expect
        .poll(() => marketCalls(stub).filter((params) => params.get("q") === "notes").length, {
          timeout: 60_000,
        })
        .toBe(1);
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Notes Pro"]);
      await driver.page.waitForTimeout(1_000);
      expect(marketCalls(stub).filter((params) => params.get("q") === "notes").length).toBe(1);
    } finally {
      await stub.uninstall();
    }
  }
);

test(
  specTitle(["CLOUD-023"], "pager steps through windows and a new query resets it"),
  { tag: specTags(["CLOUD-023"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const stub = await installOpenhubStub(driver.page.context(), {
      marketplace: (_route, logged) => {
        const params = new URL(logged.path, "http://oracle.invalid").searchParams;
        if ((params.get("q") ?? "") !== "") {
          return Promise.resolve({
            status: 200,
            body: marketplacePage([stubApp({ appId: "app-query", slug: "query", name: "Query Hit" })]),
          });
        }
        if (params.get("cursor") === "cursor-2") {
          return Promise.resolve({
            status: 200,
            body: marketplacePage([stubApp({ appId: "app-c", slug: "c", name: "Window Two" })]),
          });
        }
        return Promise.resolve({
          status: 200,
          body: marketplacePage(
            [
              stubApp({ appId: "app-a", slug: "a", name: "Window One A" }),
              stubApp({ appId: "app-b", slug: "b", name: "Window One B" }),
            ],
            "cursor-2"
          ),
        });
      },
    });
    try {
      await driver.openAuthenticated(`/${seed.workspaceSlug}/apps`, cloudBrowserCookies(session.cookie));
      await driver.dismissWelcomeDialog();
      await expect
        .poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 })
        .toEqual(["Window One A", "Window One B"]);
      // Settle the mount debounce: its timer also resets paging, and under
      // load it can fire after the first pager click, wiping the window.
      // Typing cancels the pending mount timer (effect cleanup) and the
      // round-trip below proves the replacement timers elapsed; clearing
      // renders back from cache, so the cards — not a new request — are
      // the signal.
      await driver.cloudAppsSearchFill("zzz-settle");
      await expect
        .poll(() => marketCalls(stub).filter((params) => params.get("q") === "zzz-settle").length, {
          timeout: 60_000,
        })
        .toBe(1);
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Query Hit"]);
      await driver.cloudAppsSearchFill("");
      await expect
        .poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 })
        .toEqual(["Window One A", "Window One B"]);
      expect(await driver.cloudAppsPagerVisible()).toBe(true);
      expect(await driver.cloudAppsPageIndicator()).toBe("Page 1");

      await driver.cloudAppsNextPage();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Window Two"]);
      expect(await driver.cloudAppsPageIndicator()).toBe("Page 2");
      const pageTwo = marketCalls(stub).at(-1);
      expect(pageTwo?.get("cursor")).toBe("cursor-2");
      expect(pageTwo?.get("limit")).toBe("24");

      await driver.cloudAppsPrevPage();
      await expect
        .poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 })
        .toEqual(["Window One A", "Window One B"]);
      expect(await driver.cloudAppsPageIndicator()).toBe("Page 1");

      // A new query mid-window drops the held cursor: the filtered read
      // carries the query and no cursor, and the pager hides (fresh window,
      // single page of hits).
      await driver.cloudAppsNextPage();
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Window Two"]);
      await driver.cloudAppsSearchFill("query");
      await expect.poll(() => driver.cloudAppsCardNames(), { timeout: 60_000 }).toEqual(["Query Hit"]);
      expect(await driver.cloudAppsPagerVisible()).toBe(false);
      const filtered = marketCalls(stub).filter((params) => params.get("q") === "query");
      expect(filtered.length).toBe(1);
      expect(filtered[0]?.get("cursor")).toBeNull();
    } finally {
      await stub.uninstall();
    }
  }
);
