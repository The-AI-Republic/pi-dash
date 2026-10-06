// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the not-found page (NEWFRONT-173). Behavior learned
// from the running old app, in prose: an unknown address inside a
// workspace renders an illustrated page — a heading, an explanatory note
// and a way home — instead of a blank or broken screen; the tab title
// carries the missing-page marker and crawlers are told to stay out. Row:
// SHELL-108.
import { test, expect } from "../../fixtures";
import { serverSetTourCompleted, signInSessionRetry } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-108"];

const MISSING_PATH = "__parity_missing__";
const STACK_PATTERN = /at\s+\S+\s+\([^()]*:\d+:\d+\)/;

test(
  specTitle(ROWS, "unknown addresses render a not-found page with a way home"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const badAddress = `/${seed.workspaceSlug}/${MISSING_PATH}`;

    await test.step("signed-out visitors see the illustrated page", async () => {
      await driver.page.goto(badAddress);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.notFoundFacts(), { timeout: 30_000 }).not.toBeNull();
      const surface = await driver.notFoundFacts();
      expect(surface?.title ?? "").toContain("404");
      expect(surface?.heading ?? "").not.toHaveLength(0);
      expect(surface?.body ?? "").not.toHaveLength(0);
      expect((surface?.body ?? "").length).toBeGreaterThan(20);
      expect(`${surface?.heading ?? ""}\n${surface?.body ?? ""}`).not.toMatch(STACK_PATTERN);
      expect(surface?.homeHref).toBe("/");
      expect(surface?.homeLabel ?? "").not.toHaveLength(0);
      expect(surface?.illustration).not.toBeNull();
      expect(surface?.illustration?.src ?? "").not.toHaveLength(0);
      expect(surface?.illustration?.alt ?? "").not.toHaveLength(0);
      expect(surface?.illustration?.status).toBe(200);
      expect(surface?.robots).toBe("noindex, nofollow");
    });

    await test.step("the way home leaves the missing address", async () => {
      await driver.notFoundGoHome();
      await expect.poll(() => driver.notFoundFacts(), { timeout: 30_000 }).toBeNull();
      expect(new URL(driver.page.url()).pathname).toBe("/");
    });

    await test.step("signed-in visitors see the same surface", async () => {
      const session = await signInSessionRetry(seed.email, seed.password);
      await serverSetTourCompleted(session, true);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.page.goto(badAddress);
      await driver.page.waitForLoadState("domcontentloaded");
      await expect.poll(() => driver.notFoundFacts(), { timeout: 30_000 }).not.toBeNull();
      const surface = await driver.notFoundFacts();
      expect(surface?.title ?? "").toContain("404");
      expect(surface?.heading ?? "").not.toHaveLength(0);
      expect(surface?.homeHref).toBe("/");
      expect(surface?.illustration?.status).toBe(200);
      await driver.notFoundGoHome();
      await expect.poll(() => driver.notFoundFacts(), { timeout: 30_000 }).toBeNull();
    });
  }
);
