// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): cloud sign-out, row CLOUD-016 (signing
// out ends the app session and lands on sign-in), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): signing out from the account
// menu fetches a CSRF token, posts the API logout view, clears both
// session cookies, records the explicit-logout marker, and performs a
// full navigation to the login card. The upstream provider session is
// intentionally left alone — the app never visits the provider logout
// URL the API returns.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudSignOutServer } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-016"], "sign-out ends the app session and lands on sign-in"),
  { tag: specTags(["CLOUD-016"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(session.cookie)
    );
    await driver.dismissWelcomeDialog();

    const authRequests: string[] = [];
    const destinations: string[] = [];
    driver.page.on("request", (request) => {
      const url = request.url();
      if (url.includes("/api/auth/get-csrf-token/") || url.includes("/api/auth/sign-out/")) {
        authRequests.push(`${request.method()} ${new URL(url).pathname}`);
      }
      destinations.push(url);
    });
    await driver.cloudSignOutViaAccountMenu();

    expect(authRequests).toEqual(["GET /api/auth/get-csrf-token/", "POST /api/auth/sign-out/"]);
    expect(await driver.currentPath()).toBe("/login");
    expect(await driver.cloudExplicitLogoutMarker()).toBe(true);
    const cookies = await driver.cloudCookieNames();
    expect(cookies).not.toContain("pidash_access");
    expect(cookies).not.toContain("pidash_refresh");
    const card = await driver.cloudLoginCardFacts();
    expect(card.continueActions).toBe(1);
    // The provider logout destination is never visited: the upstream SSO
    // session is left alone.
    expect(destinations.some((url) => url.includes("/auth/logout"))).toBe(false);

    // Server state: the logout view answers 200, expires both session
    // cookies, and names the (unvisited) provider logout URL.
    const answer = await cloudSignOutServer((await cloudDevLogin(seed.email)).cookie);
    expect(answer.status).toBe(200);
    expect(answer.setCookies.some((line) => line.startsWith("pidash_access=") && /Max-Age=0/i.test(line))).toBe(true);
    expect(answer.setCookies.some((line) => line.startsWith("pidash_refresh=") && /Max-Age=0/i.test(line))).toBe(true);
    expect(typeof answer.payload["airepublic_logout_url"]).toBe("string");
    expect((answer.payload["airepublic_logout_url"] as string).length).toBeGreaterThan(0);
  }
);
