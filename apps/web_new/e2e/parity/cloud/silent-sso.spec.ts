// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): silent sign-in, row CLOUD-012
// (returning visitors with a pending target skip the card), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): a valid return target with no
// error and no explicit-logout marker resumes the provider flow
// immediately — the card never renders and the handoff carries the
// target. Any error parameter keeps the card with a resume-failure
// alert instead of resuming. The handoff is captured at the sign-in
// request boundary (see login-card.spec.ts).
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-012"], "pending target resumes the provider flow without the card"),
  { tag: specTags(["CLOUD-012"]) },
  async ({ driver }) => {
    await driver.cloudArmSignInCapture();
    await driver.openPath("/login?next_path=%2Fdocs");
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.pathname).toBe("/api/auth/sign-in/");
    expect(handoff.searchParams.get("return_to")).toBe("/docs");
    expect(await driver.cloudSignInRequests()).toHaveLength(1);
    // The resume replaced the login page with the handoff, not the card.
    expect(await driver.currentPath()).toContain("/api/auth/sign-in/");
  }
);

test(
  specTitle(["CLOUD-012"], "error keeps the card with a resume-failure alert"),
  { tag: specTags(["CLOUD-012"]) },
  async ({ driver }) => {
    await driver.cloudArmSignInCapture();
    await driver.openPath("/login?next_path=%2Fdocs&error=oidc_down&sso=failed");
    const card = await driver.cloudLoginCardFacts();
    expect(card.continueActions).toBe(1);
    expect(card.alertText ?? "").toContain("automatically");
    expect(await driver.cloudSignInRequests()).toHaveLength(0);
    expect(await driver.currentPath()).toMatch(/^\/login\?/);
  }
);
