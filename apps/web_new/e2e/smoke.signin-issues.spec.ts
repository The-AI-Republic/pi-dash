// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Smoke: sign-in → issue list → palette → sign-out (NEWFRONT-17). Runs
// against a scratch Django seeded with the api-client contract seed, so
// the workspace, project and issues below always exist. Skips without
// PIDASH_E2E_BASE_URL.
import { expect, test } from "@playwright/test";

const BASE_URL = process.env["PIDASH_E2E_BASE_URL"] ?? "";
const EMAIL = process.env["PIDASH_E2E_EMAIL"] ?? "";
const PASSWORD = process.env["PIDASH_E2E_PASSWORD"] ?? "";
const WORKSPACE = process.env["PIDASH_E2E_WORKSPACE"] ?? "";
const PROJECT_ID = process.env["PIDASH_E2E_PROJECT_ID"] ?? "";

const enabled =
  BASE_URL.length > 0 && EMAIL.length > 0 && PASSWORD.length > 0 && WORKSPACE.length > 0 && PROJECT_ID.length > 0;

test.describe("sign-in to issue list", () => {
  test.skip(!enabled, "needs PIDASH_E2E_BASE_URL/EMAIL/PASSWORD/WORKSPACE/PROJECT_ID");
  test("signs in, lists issues, opens the palette, signs out", async ({ page }) => {
    await page.goto("/sign-in");
    await expect(page.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();

    await page.getByLabel("Work email").fill(EMAIL);
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(page.getByLabel("Password")).toBeVisible();
    await page.getByLabel("Password").fill(PASSWORD);
    await page.getByRole("button", { name: "Sign in" }).click();
    // Success follows the server landing URL off the sign-in page.
    await expect(page).not.toHaveURL(/sign-in/);

    await page.goto(`/${WORKSPACE}/projects/${PROJECT_ID}/issues`);
    await expect(page.getByRole("region", { name: "Issues" })).toBeVisible();
    const seededRow = page.getByRole("article").filter({ hasText: "Contract issue one" });
    await expect(seededRow).toBeVisible();
    await expect(page.getByText("Contract", { exact: true }).first()).toBeVisible();

    await page.keyboard.press("Control+K");
    await expect(page.getByRole("dialog", { name: "Commands" })).toBeVisible();
    await page.getByLabel("Filter commands").fill("sign out");
    await page.getByRole("option", { name: "Sign out" }).click();
    await expect(page).toHaveURL(/sign-in/);
    await expect(page.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();
  });
});
