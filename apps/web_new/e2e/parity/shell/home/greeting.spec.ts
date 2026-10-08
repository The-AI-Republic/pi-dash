// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-123): home dashboard greeting, clock and tab
// title. Rows: SHELL-003 (dashboard loads with greeting, assistant card,
// widget stack and workspace tab title), SHELL-004 (time-of-day greeting
// with name, date line and live clock), SHELL-005 (fixed date shape while
// the salutation localizes; never a blank fallback).
// Behavior learned from the old dashboard in prose: the greeting derives
// its morning/afternoon/evening variant from the local hour, prints the
// user name beside it, and shows a weekday-plus-date-plus-clock sub-line.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/index";
import {
  serverHomeMe,
  serverHomeUser,
  serverPatchProfile,
  serverPatchUser,
  serverSetTourCompleted,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

async function reloadUntilHeading(driver: ParityDriver): Promise<string> {
  // A loaded paint can miss the greeting past one budget; reconcile
  // once before failing honestly.
  for (let round = 0; round < 2; round += 1) {
    if (round > 0) await driver.homeReload();
    try {
      await expect.poll(() => driver.homeGreetingHeading(), { timeout: 30_000 }).not.toBeNull();
      return (await driver.homeGreetingHeading()) ?? "";
    } catch {
      // One more fresh paint.
    }
  }
  return (await driver.homeGreetingHeading()) ?? "";
}

const ROWS = ["SHELL-003", "SHELL-004", "SHELL-005"];
const ROWS_LOCALE = ["SHELL-005"];
const DATE_SHAPE = /[A-Za-z]+, [A-Za-z]{3} \d{1,2} \d{1,2}:\d{2}/;

function expectedVariant(hour: number): RegExp {
  if (hour < 12) return /morning/i;
  if (hour < 18) return /afternoon/i;
  return /evening/i;
}

test(specTitle(ROWS, "home dashboard greets the signed-in user"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  const session = await signInSessionRetry(seed.email, seed.password);
  const me = await serverHomeMe(session);
  // The tour overlay covers the dashboard for fresh users, so clear it
  // through the server before asserting on the dashboard itself.
  await serverSetTourCompleted(session, true);
  await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);

  await test.step("sign in and open home", async () => {
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.homeOpen(seed.workspaceSlug);
  });

  await test.step("greeting names the user with the time-of-day variant", async () => {
    const heading = await driver.homeGreetingHeading();
    expect(heading ?? "").toContain(me.first_name);
    expect(heading ?? "").toMatch(expectedVariant(new Date().getHours()));
  });

  await test.step("date line keeps a fixed weekday-date-clock shape", async () => {
    const line = await driver.homeDateLine();
    expect(line ?? "").toMatch(DATE_SHAPE);
  });

  await test.step("assistant card and widget stack render with the workspace tab title", async () => {
    expect(await driver.homeAssistantState()).not.toBe("hidden");
    expect(await driver.homeWidgetTitles()).toEqual(expect.arrayContaining(["Quicklinks", "Recents"]));
    await expect(driver.page).toHaveTitle(new RegExp(seed.workspaceName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  });

  await test.step("the clock advances without a reload", async () => {
    const before = await driver.homeDateLine();
    await expect.poll(() => driver.homeDateLine(), { timeout: 75_000 }).not.toBe(before);
  });
});

test(
  specTitle(ROWS_LOCALE, "unknown locales fall back to English without blanking"),
  { tag: specTags(ROWS_LOCALE) },
  async ({ driver, seed }) => {
    // Oracle note (SHELL-005): the OSS build ships English strings only,
    // so a translated salutation cannot be exercised on the seeded stack;
    // what the stack proves is the fallback side — unknown locales keep
    // the English salutation with its fixed date shape, never blank. The
    // language lives on the profile record (the app reads it on boot);
    // each variant is echoed back before asserting the UI.
    const session = await signInSessionRetry(seed.email, seed.password);
    const me = await serverHomeMe(session);
    const baseline = await serverHomeUser(session);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    try {
      await serverPatchProfile(session, { language: "en" });
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.homeOpen(seed.workspaceSlug);

      await test.step("an unshipped locale keeps the English salutation", async () => {
        expect((await serverPatchProfile(session, { language: "de" })).ok).toBe(true);
        expect((await serverHomeUser(session)).language).toBe("de");
        await driver.homeReload();
        const heading = await reloadUntilHeading(driver);
        expect(heading ?? "").toContain(me.first_name);
        expect(heading ?? "").toMatch(expectedVariant(new Date().getHours()));
        expect((await driver.homeDateLine()) ?? "").toMatch(DATE_SHAPE);
      });

      await test.step("a malformed locale never blanks the greeting", async () => {
        expect((await serverPatchProfile(session, { language: "xx-broken!!!" })).ok).toBe(true);
        expect((await serverHomeUser(session)).language).toBe("xx-broken!!!");
        await driver.homeReload();
        const heading = await reloadUntilHeading(driver);
        expect((heading ?? "").trim().length).toBeGreaterThan(0);
        expect(heading ?? "").toContain(me.first_name);
        expect((await driver.homeDateLine()) ?? "").toMatch(DATE_SHAPE);
      });

      await test.step("a broken timezone is rejected or degrades without blanking", async () => {
        const outcome = await serverPatchUser(session, { user_timezone: "Mars/Olympus_Mons" });
        await driver.homeReload();
        await reloadUntilHeading(driver);
        const line = await driver.homeDateLine();
        expect((line ?? "").trim().length).toBeGreaterThan(0);
        expect((await driver.homeGreetingHeading()) ?? "").toContain(me.first_name);
        if (!outcome.ok) {
          // The API validates timezones, so the UI can never observe one:
          // record the rejection as the graceful path.
          expect(outcome.status).toBeGreaterThanOrEqual(400);
        }
      });
    } finally {
      await serverPatchProfile(session, { language: baseline.language ?? "en" }).catch(() => undefined);
      await serverPatchUser(session, { user_timezone: baseline.user_timezone ?? "UTC" }).catch(() => undefined);
    }
  }
);
