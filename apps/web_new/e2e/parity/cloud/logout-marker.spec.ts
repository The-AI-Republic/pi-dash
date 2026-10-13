// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): explicit-logout marker, row CLOUD-014
// (signing out disables silent sign-in until the next manual attempt),
// green on the cloud-edition old app first. The same file must go green
// on apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): signing out through the UI
// records the tab-scoped marker, and a later login visit with a valid
// return target then renders the card instead of resuming. Starting a
// manual sign-in clears the marker and carries the pending target, so
// the visit after that resumes silently again.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-014"], "sign-out suppresses resume until the next manual attempt"),
  { tag: specTags(["CLOUD-014"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${seed.projectId}/issues`,
      cloudBrowserCookies(session.cookie)
    );
    await driver.dismissWelcomeDialog();
    await driver.cloudSignOutViaAccountMenu();
    expect(await driver.cloudExplicitLogoutMarker()).toBe(true);

    await driver.cloudArmSignInCapture();
    await driver.openPath("/login?next_path=%2Fdocs");
    const card = await driver.cloudLoginCardFacts();
    expect(card.continueActions).toBe(1);
    expect(await driver.cloudSignInRequests()).toHaveLength(0);

    // The manual attempt carries the pending target and clears the marker.
    await driver.cloudClickLoginContinue();
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.searchParams.get("return_to")).toBe("/docs");
    expect(await driver.cloudExplicitLogoutMarker()).toBe(false);

    // With the marker cleared, the next targeted visit resumes silently.
    await driver.openPath("/login?next_path=%2Fdocs");
    const resumed = new URL(await driver.cloudAwaitSignInRequest(1));
    expect(resumed.searchParams.get("return_to")).toBe("/docs");
    expect(await driver.cloudSignInRequests()).toHaveLength(2);
  }
);
