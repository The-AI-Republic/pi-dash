// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): passwordless cloud onboarding, row
// CLOUD-020 (cloud onboarding asks for no password), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): a fresh cloud identity walks
// the standard onboarding steps, but the profile step's password slot
// renders no fields, the submit never calls the password endpoint, and
// the profile stores normally — cloud identities come from the
// provider, so no password is ever set.
import { test, expect } from "../fixtures";
import { cloudBrowserCookies, cloudDevLogin, currentUser, uniqueEmail } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-020"], "onboarding profile step asks for no password"),
  { tag: specTags(["CLOUD-020"]) },
  async ({ driver }) => {
    const email = uniqueEmail("parity-cloud-onb");
    const session = await cloudDevLogin(email, "Cloud Newcomer");
    const passwordCalls: string[] = [];
    driver.page.on("request", (request) => {
      // Endpoint paths only (NEWFRONT-662): the dev server also serves
      // source modules like set-password.tsx, which are not endpoint
      // calls. The oracle serves no such modules, so it is unaffected.
      if (request.url().includes("/set-password/"))
        passwordCalls.push(`${request.method()} ${new URL(request.url).pathname}`);
    });
    await driver.openAuthenticated("/onboarding", cloudBrowserCookies(session.cookie));
    await driver.advanceCliInstall();
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("Create your profile");
    expect(await driver.cloudProfilePasswordFieldCount()).toBe(0);
    await driver.submitProfileStep("Cloud Newcomer");
    // The flow advances to the role step without any password step.
    await expect.poll(() => driver.pageText(), { timeout: 60_000 }).toContain("What's your role?");
    expect(passwordCalls).toEqual([]);

    // Server state: the profile stored without any password step. (The
    // profile_complete progress flag flips only after the later use-case
    // step on cloud, so the stored name is the step's own proof.)
    const user = await currentUser(session.cookie);
    expect(user.first_name).toContain("Cloud");
  }
);
