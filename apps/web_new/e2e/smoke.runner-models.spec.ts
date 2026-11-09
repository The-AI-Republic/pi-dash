// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Smoke: god-mode runner models page (NEWPIDASH-169, DIFF-8). Runs
// against the Rust server (with Django upstream for the sign-in POST)
// seeded with the api-client contract seed admin, so the instance admin
// and the 27-row catalog always exist: sign in as an instance admin,
// manage models on the page, and watch the picker-relevant rows change.
// Skips without PIDASH_E2E_BASE_URL (Rust server) + admin credentials.
//
//   export PIDASH_E2E_BASE_URL="http://127.0.0.1:8124"   # Rust (PIDASH_DJANGO_UPSTREAM=http://127.0.0.1:8123)
//   export PIDASH_E2E_ADMIN_EMAIL="contract.admin@example.com"
//   export PIDASH_E2E_ADMIN_PASSWORD="Contract123!"
//   pnpm --filter web_new test:e2e
import { expect, test } from "@playwright/test";

const BASE_URL = process.env["PIDASH_E2E_BASE_URL"] ?? "";
const EMAIL = process.env["PIDASH_E2E_ADMIN_EMAIL"] ?? "";
const PASSWORD = process.env["PIDASH_E2E_ADMIN_PASSWORD"] ?? "";

const enabled = BASE_URL.length > 0 && EMAIL.length > 0 && PASSWORD.length > 0;

test.describe("god-mode runner models", () => {
  test.skip(!enabled, "needs PIDASH_E2E_BASE_URL (Rust) + PIDASH_E2E_ADMIN_EMAIL/PASSWORD");

  test("lists the catalog, adds a model, and deletes it", async ({ page }) => {
    await page.goto("/god-mode/");
    // Cold dev servers spend the first seconds transforming the route
    // graph; allow the entry gate to settle well past the default.
    await expect(page.getByTestId("admin-sign-in-form"), { timeout: 60_000 }).toBeVisible();
    await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
    await page.getByRole("textbox", { name: "Password" }).fill(PASSWORD);
    await page.getByRole("button", { name: /Sign in/ }).click();
    await expect(page).not.toHaveURL(/sign-in/);

    await page.goto("/god-mode/runner-models/");
    await expect(page.getByTestId("runner-models-page")).toBeVisible();
    await expect(page.getByTestId("runner-models-title")).toHaveText("Runner models");
    await expect(page.getByTestId("runner-models-note")).toContainText(
      "Deleting or disabling a model only removes it from the add-runner picker"
    );
    // One section per agent, the Claude seed row preselected.
    for (const agent of ["claude-code", "codex", "cursor-agent", "open-claw", "grok", "muse-code"]) {
      await expect(page.getByTestId(`runner-models-table-${agent}`)).toBeVisible();
    }
    await expect(page.getByTestId("runner-models-table-claude-code")).toContainText("claude-opus-4-8");
    await expect(page.getByTestId("runner-models-empty-open-claw")).toBeVisible();

    // Add a Grok model through the dialog; it appears without a reload.
    const stamp = Date.now().toString(36);
    const model = `grok-smoke-${stamp}`;
    await page.getByTestId("runner-models-add-grok").click();
    await expect(page.getByTestId("runner-models-dialog")).toBeVisible();
    await page.getByTestId("runner-models-model").fill(model);
    await page.getByTestId("runner-models-label").fill("Grok Smoke");
    await page.getByTestId("runner-models-save").click();
    await expect(page.getByTestId("runner-models-dialog")).toBeHidden();
    const table = page.getByTestId("runner-models-table-grok");
    await expect(table).toContainText(model);

    // Delete it through the two-step confirm; the row leaves the table.
    const createdRow = table.locator("tr", { hasText: model });
    await createdRow.getByTestId(/^runner-models-delete-/).click();
    await createdRow.getByTestId(/^runner-models-confirm-/).click();
    await expect(table).not.toContainText(model);
  });
});
