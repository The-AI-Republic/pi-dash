// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): session refresh and replay, row
// CLOUD-015 (expired sessions refresh transparently; only real auth
// failures bounce), green on the cloud-edition old app first. The same
// file must go green on apps/web_new once the cloud login area lands
// there.
//
// Observed behavior notes (cloud oracle): the first 401 triggers exactly
// one shared cookie rotation plus a replay of the original request, so a
// page whose access cookie expired keeps rendering without bouncing.
// Only a rejected refresh sends the user to sign-in, with the current
// location preserved as the return target (captured at the handoff
// boundary, since the login page then resumes silently). Public pages
// never bounce, and a refresh that dies in transport leaves an in-page
// action where it is — no navigation, no handoff. (A full-page load with
// a dead refresh still routes through the signed-out wrapper, which
// cannot render without data; the keep-in-place rule covers actions on
// an already rendered page.)
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, cloudRefreshStatus } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-015"], "expired access rotates once and the page keeps rendering"),
  { tag: specTags(["CLOUD-015"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const issuesPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
    await driver.openAuthenticated(issuesPath, cloudBrowserCookies(session.cookie));
    await driver.dismissWelcomeDialog();
    await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain("Cloud first issue");

    await driver.cloudArmRefreshCounter();
    await driver.cloudExpireAccessCookie();
    await driver.page.reload();
    await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain("Cloud first issue");
    expect(await driver.currentPath()).toBe(issuesPath);
    // One rotation shared across the reload's concurrent calls — not one
    // per 401 — and the rotated access cookie is back in the jar.
    expect(await driver.cloudRefreshHits()).toBe(1);
    expect(await driver.cloudCookieNames()).toContain("pidash_access");

    // Server state: a live refresh cookie rotates the pair; anything else
    // is rejected.
    const fresh = await cloudDevLogin(seed.email);
    const rotated = await cloudRefreshStatus(fresh.cookie);
    expect(rotated.status).toBe(200);
    expect(rotated.setCookies.some((line) => line.startsWith("pidash_access="))).toBe(true);
    expect(rotated.setCookies.some((line) => line.startsWith("pidash_refresh="))).toBe(true);
    expect((await cloudRefreshStatus("pidash_refresh=dead")).status).toBe(401);
    expect((await cloudRefreshStatus("")).status).toBe(401);
  }
);

test(
  specTitle(["CLOUD-015"], "dead session bounces with the location preserved"),
  { tag: specTags(["CLOUD-015"]) },
  async ({ driver, seed }) => {
    const issuesPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
    await driver.cloudArmSignInCapture();
    await driver.openPath(issuesPath);
    // The bounce routes through login, which then resumes silently; the
    // handoff's return target proves the original location survived.
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.searchParams.get("return_to")).toBe(issuesPath);
  }
);

test(specTitle(["CLOUD-015"], "public pages never bounce"), { tag: specTags(["CLOUD-015"]) }, async ({ driver }) => {
  await driver.openPath("/home");
  await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/home");
  expect(await driver.cloudHomeHeroActionHrefs()).toEqual(["/login", "/downloads", "/docs"]);
});

test(
  specTitle(["CLOUD-015"], "transport failure leaves the page in place"),
  { tag: specTags(["CLOUD-015"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const issuesPath = `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`;
    await driver.openAuthenticated(issuesPath, cloudBrowserCookies(session.cookie));
    await driver.dismissWelcomeDialog();
    // Order the sabotage after the first render (NEWFRONT-662): expiring
    // the access cookie mid-boot turns the boot itself into the refused
    // call, which is the rotation scenario's shape, not this one's. The
    // oracle boots before the sabotage either way, so it is unaffected.
    await expect.poll(() => driver.visibleIssueNames(), { timeout: 60_000 }).toContain("Cloud first issue");
    await driver.cloudExpireAccessCookie();
    await driver.cloudFailRefresh();
    await driver.cloudArmSignInCapture();
    await driver.openCreateModal();
    await driver.fillCreateTitle("transient probe");
    await driver.submitCreateModal();
    // The creation cannot complete, but nothing bounces: the page stays
    // and no sign-in handoff leaves.
    await driver.page.waitForTimeout(5_000);
    expect(await driver.currentPath()).toBe(issuesPath);
    expect(await driver.cloudSignInRequests()).toHaveLength(0);
  }
);
