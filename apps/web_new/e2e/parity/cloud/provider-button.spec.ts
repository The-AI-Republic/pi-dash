// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): provider button on generic cards, row
// CLOUD-017 (generic auth screens gain an organizational provider
// button), green on the cloud-edition old app first. The same file must
// go green on apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): the sign-up card offers the
// provider button alongside the standard continue action (the legacy
// sign-in form routes do not render in the cloud edition — /accounts
// paths 404 or forward here — so this card is the generic surface). The
// handoff preserves the current page plus its query as the return
// target and drops the fragment. Inside a desktop shell without a baked
// API origin the button refuses loudly and never navigates.
import { test, expect } from "../fixtures";
import { cloudSignInRedirect } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-017"], "provider button preserves the page and drops the fragment"),
  { tag: specTags(["CLOUD-017"]) },
  async ({ driver }) => {
    await driver.openPath("/sign-up");
    const facts = await driver.cloudProviderButtonFacts();
    expect(facts.visible).toBe(true);
    expect(facts.label).toContain("AI Republic");
    expect(facts.standardContinueVisible).toBe(true);

    await driver.cloudArmSignInCapture();
    await driver.openPath("/sign-up?next_path=%2Fdocs#frag");
    await driver.cloudClickProviderButton();
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.searchParams.get("return_to")).toBe("/sign-up?next_path=%2Fdocs");

    // Server state: the endpoint honors a generic-card return target.
    const answer = await cloudSignInRedirect({ return_to: "/sign-up?next_path=%2Fdocs" });
    expect(answer.status).toBe(302);
  }
);

test(
  specTitle(["CLOUD-017"], "desktop shell without a baked origin refuses loudly"),
  { tag: specTags(["CLOUD-017"]) },
  async ({ driver }) => {
    await driver.cloudStubTauri();
    await driver.cloudArmConsoleErrors();
    await driver.cloudArmSignInCapture();
    await driver.openPath("/sign-up");
    await driver.cloudClickProviderButton();
    await driver.page.waitForTimeout(3_000);
    expect(await driver.currentPath()).toBe("/sign-up");
    expect(await driver.cloudSignInRequests()).toHaveLength(0);
    expect((await driver.cloudConsoleErrors()).some((line) => line.includes("VITE_API_BASE_URL"))).toBe(true);
  }
);
