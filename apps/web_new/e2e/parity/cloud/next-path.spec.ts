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
];

/**
 * Encoded spellings the old app does not reject (bug NEWFRONT-777): the
 * parser normalizes them to protocol-relative or encoded paths and the old
 * guard never looks again. The new app rejects them; the oracle pins the
 * old behavior so the row records the defect instead of failing on it.
 */
const OLD_APP_ACCEPTS = ["/%2e%2e//evil.example/x", "/a/%2e%2e/%2f/evil.example", "/%5c%5cevil.example/steal"];

/**
 * Login spellings the old app's loop guard misses (bug NEWFRONT-778): it
 * compares the raw pathname against an exact, case-sensitive set, so the
 * handoff leaves for a target the router folds back onto the login page.
 * The new app decodes and case-folds the pathname, so these hand back the
 * app root; the oracle pins the old result instead of failing on it.
 */
const OLD_APP_LOOPS_ON = ["/%6Cogin", "/LoGiN", "/%6Cogin%3Fnext_path=%2Fdocs"];

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

for (const accepted of OLD_APP_ACCEPTS) {
  test(
    specTitle(["CLOUD-013"], `bug: NEWFRONT-777 old app accepts an encoded target: ${accepted}`),
    { tag: specTags(["CLOUD-013"]) },
    async ({ driver }) => {
      await driver.cloudArmSignInCapture();
      await driver.openPath(`/login?next_path=${encodeURIComponent(accepted)}`);
      // Observed old behavior: the handoff leaves with the normalized value.
      const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
      expect(handoff.searchParams.get("return_to")).not.toBe("/");
    }
  );
}

for (const loop of OLD_APP_LOOPS_ON) {
  test(
    specTitle(["CLOUD-013"], `bug: NEWFRONT-778 old app hands the provider a login target: ${loop}`),
    { tag: specTags(["CLOUD-013"]) },
    async ({ driver }) => {
      await driver.cloudArmSignInCapture();
      await driver.openPath(`/login?next_path=${encodeURIComponent(loop)}`);
      if (driver.target === "web_new") {
        // Intended behavior: the loop guard folds the spelling onto the
        // login alias, so the handoff carries the app root.
        const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
        expect(handoff.searchParams.get("return_to")).toBe("/");
        return;
      }
      // Observed old behavior: the exact-match guard misses the spelling and
      // the handoff leaves with the value the router resolves to /login.
      const handoff = new URL(await driver.cloudAwaitSignInRequest(0));
      expect(handoff.searchParams.get("return_to")).not.toBe("/");
    }
  );
}
