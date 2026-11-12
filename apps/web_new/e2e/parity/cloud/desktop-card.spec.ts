// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): desktop cloud card, row CLOUD-019
// (the desktop shell shows the same cloud sign-in card), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): the card's rendering does not
// branch on the shell — with a desktop global present from load, the
// login page shows the same card contract (one continue action into the
// sign-in endpoint, account line, terms and privacy, home link, no
// alert). The desktop entry embeds this same component (its sign-in
// seam re-exports the shared card); rendering that shell needs the
// desktop build, so the oracle pins the shared contract here.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["CLOUD-019"], "desktop shell shows the same cloud sign-in card"),
  { tag: specTags(["CLOUD-019"]) },
  async ({ driver }) => {
    await driver.cloudStubTauri();
    await driver.openPath("/login");
    const tauriPresent = await driver.page.evaluate(
      () => typeof (window as unknown as Record<string, unknown>).__TAURI__ !== "undefined"
    );
    expect(tauriPresent).toBe(true);
    const card = await driver.cloudLoginCardFacts();
    expect(card.heading).toContain("Sign in");
    expect(card.accountLine).toContain("AI Republic");
    expect(card.continueActions).toBe(1);
    expect(card.continueHref).toBe("/api/auth/sign-in/");
    expect(card.termsHref).toBe("https://airepublic.com/terms");
    expect(card.privacyHref).toBe("https://airepublic.com/privacy");
    expect(card.homeHref).toBe("/home");
    expect(card.alertText).toBeNull();
  }
);
