// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-438): return-target validation, row
// CLOUD-013 (targets survive login but cannot escape the app), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud login area lands there.
//
// Observed behavior notes (cloud oracle): a deep path plus its query
// survives the round trip verbatim as the handoff's return target, while
// protocol-relative, absolute, backslashed, or non-relative values are
// discarded — the card renders and no handoff leaves. (The companion
// guard, signing in from the login page itself returning to the app root
// instead of looping, is proven in login-card.spec.ts.)
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

/** Targets that must never become a handoff destination. */
const UNSAFE_TARGETS = [
  "//evil.example/steal",
  "https://evil.example/steal",
  "http://localhost:13038/downgrade",
  "/\\evil.example/steal",
  "workspace/issues/1",
  "/%2e%2e//evil.example/x",
  "/a/%2e%2e/%2f/evil.example",
  "/%5c%5cevil.example/steal",
];

test(
  specTitle(["CLOUD-013"], "deep target with query survives the round trip"),
  { tag: specTags(["CLOUD-013"]) },
  async ({ driver }) => {
    const target = "/cloud-ws/projects/abc/issues?peek=activity&tab=comments";
    await driver.cloudArmSignInCapture();
    await driver.openPath(`/login?next_path=${encodeURIComponent(target)}`);
    const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
    expect(handoff.searchParams.get("return_to")).toBe(target);
  }
);

for (const unsafe of UNSAFE_TARGETS) {
  test(
    specTitle(["CLOUD-013"], `unsafe target is discarded: ${unsafe}`),
    { tag: specTags(["CLOUD-013"]) },
    async ({ driver }) => {
      await driver.cloudArmSignInCapture();
      await driver.openPath(`/login?next_path=${encodeURIComponent(unsafe)}`);
      const card = await driver.cloudLoginCardFacts();
      expect(card.continueActions).toBe(1);
      expect(await driver.cloudSignInRequests()).toHaveLength(0);
    }
  );
}
