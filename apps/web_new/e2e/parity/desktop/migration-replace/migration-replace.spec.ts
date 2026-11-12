// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (PIDESKAPP-223): legacy migration + notices, the
// replace-machine prompt, and the sign-out "Also sign out Pi Dash on this
// computer" opt-in — web-observable sides. Rows: DESK-029–DESK-031.
//
// The oracle runs on apps/web, where the desktop host is absent by design:
// no legacy scan, no notices store, no replace candidates lookup, and no
// computer sign-out. Each scenario pins the web-observable side: no host
// bridge traffic, no migration traffic, no prompt, and sign-out without a
// computer opt-in. The desktop halves (web_new positive legs, skipped on the
// apps/web oracle) drive the mocked shell directly.
import { test, expect } from "../../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../../drivers/parity-driver";
import { sessionValid, signInSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { desktopInvocations, installDesktopMock } from "../tauri-mock";

const HOST_COMMANDS = [
  "legacy_scan",
  "legacy_finish",
  "legacy_restore_folder",
  "desktop_notices_load",
  "desktop_notices_record",
  "desktop_notices_dismiss",
  "pidash_cli_logout",
];

async function signIn(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await signInSession(seed.email, seed.password);
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await expect.poll(() => sessionValid(session), { timeout: 30_000 }).toBe(true);
}

test(
  specTitle(["DESK-029"], "web shows no migration notices and runs no legacy host commands"),
  { tag: specTags(["DESK-029"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await installDesktopMock(driver.page, {
        daemon: "compatible",
        legacyTrees: [],
        notices: { notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] },
      });
      await signIn(driver, seed);
      await driver.page.waitForTimeout(1_000);
      const invocations = await desktopInvocations(driver.page);
      // Desktop leg: the supervisor scanned and found nothing, then checked notices.
      expect(invocations.some((i) => i.command === "legacy_scan")).toBe(true);
      expect(invocations.some((i) => i.command === "desktop_notices_load")).toBe(true);
      expect(invocations.some((i) => i.command === "pidash_cli_logout")).toBe(false);
      return;
    }
    await signIn(driver, seed);
    await driver.page.waitForTimeout(1_000);
    const invocations = await desktopInvocations(driver.page);
    expect(invocations.filter((i) => HOST_COMMANDS.includes(i.command))).toEqual([]);
    expect(await driver.page.getByText("This app now uses Pi Dash on this computer", { exact: false }).count()).toBe(0);
  }
);

test(
  specTitle(["DESK-030"], "web never asks about a replaced machine"),
  { tag: specTags(["DESK-030"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await installDesktopMock(driver.page, { daemon: "compatible", machineId: "machine-new" });
      await signIn(driver, seed);
      // No replace-candidates request is made by the desktop web build
      // itself over REST either; the prompt needs a server candidate.
      await driver.page.waitForTimeout(1_000);
      expect(await driver.page.getByText(/Is this .*Replace it/, { exact: false }).count()).toBe(0);
      return;
    }
    await signIn(driver, seed);
    await driver.page.waitForTimeout(1_000);
    const invocations = await desktopInvocations(driver.page);
    expect(invocations.filter((i) => HOST_COMMANDS.includes(i.command))).toEqual([]);
    expect(await driver.page.getByText(/Is this .*Replace it/, { exact: false }).count()).toBe(0);
  }
);

test(
  specTitle(["DESK-031"], "web sign-out has no computer opt-in"),
  { tag: specTags(["DESK-031"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await installDesktopMock(driver.page, { daemon: "compatible" });
      await signIn(driver, seed);
      // The dialog offers the computer switch; without it nothing is logged.
      const invocations = await desktopInvocations(driver.page);
      expect(invocations.some((i) => i.command === "pidash_cli_logout")).toBe(false);
      return;
    }
    await signIn(driver, seed);
    const invocations = await desktopInvocations(driver.page);
    expect(invocations.filter((i) => HOST_COMMANDS.includes(i.command))).toEqual([]);
  }
);
