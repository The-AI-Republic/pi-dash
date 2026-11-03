// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): cloud sign-in card, row CLOUD-011 (the
// login card offers a single organizational-account button carrying the
// return target), green on the cloud-edition old app first. The same file
// must go green on apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): the card carries exactly one
// continue action anchored at the sign-in endpoint, names the account
// type, references the terms and privacy pages, and links back to the
// marketing home; no resume-failure alert renders on a plain visit.
// Activating the action leaves the app for the provider flow with the
// return target attached — from the login page itself the target is the
// app root (the loop guard: returning to the login page would read as
// "login does nothing"), and same-origin setups send no return origin.
// The real provider is unreachable from runners, so the handoff is
// captured at the sign-in request boundary and answered with a stub.
import { test, expect } from "../fixtures";
import { cloudSignInRedirect } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-011"], "login card offers one continue action into the provider handoff"),
  { tag: specTags(["CLOUD-011"]) },
  async ({ driver }) => {
    await driver.openPath("/login");
    const card = await driver.cloudLoginCardFacts();
    expect(card.heading).toContain("Sign in");
    expect(card.accountLine).toContain("AI Republic");
    expect(card.continueActions).toBe(1);
    expect(card.continueHref).toBe("/api/auth/sign-in/");
    expect(card.termsHref).toBe("https://airepublic.com/terms");
    expect(card.privacyHref).toBe("https://airepublic.com/privacy");
    expect(card.homeHref).toBe("/home");
    expect(card.alertText).toBeNull();

    await driver.cloudArmSignInCapture();
    await driver.cloudClickLoginContinue();
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.pathname).toBe("/api/auth/sign-in/");
    expect(handoff.searchParams.get("return_to")).toBe("/");
    expect(handoff.searchParams.has("return_origin")).toBe(false);

    // Server state: the endpoint honors the handoff by redirecting to the
    // provider authorize URL with the PKCE + state parameters attached.
    const answer = await cloudSignInRedirect({ return_to: "/" });
    expect(answer.status).toBe(302);
    const provider = new URL(answer.location);
    expect(provider.pathname).toBe("/auth/authorize");
    expect(provider.searchParams.get("response_type")).toBe("code");
    expect(provider.searchParams.get("code_challenge_method")).toBe("S256");
    expect(provider.searchParams.get("code_challenge") ?? "").not.toBe("");
    expect(provider.searchParams.get("state") ?? "").not.toBe("");
    expect(provider.searchParams.get("nonce") ?? "").not.toBe("");
  }
);
