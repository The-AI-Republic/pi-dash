// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenario: agent-status failure-policy pause (PDASHOSS01-183, ISS-164).
// A ticker disarmed by failure_needs_human or repeated_failure shows a
// paused-after-failure card on both apps with a warning badge and a detail
// pointing at the failure comment and Re-tick — not the generic "off" card.
import { test, expect } from "../fixtures";
import { fetchIssue, runsShell, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

const ROWS = ["ISS-164"];

for (const disarmReason of ["failure_needs_human", "repeated_failure"]) {
  test(
    specTitle(ROWS, `agent status is paused after a failure (${disarmReason})`),
    { tag: specTags(ROWS) },
    async ({ driver, seed }) => {
      await signIn(driver, seed);
      const session = await signInSession(seed.email, seed.password);
      const issue = await ownIssue(seed, session, `Oracle pause ${disarmReason} ${Date.now()}`);
      try {
        await test.step(`seed ticker disarmed by ${disarmReason}`, async () => {
          await runsShell(
            `from pi_dash.db.models.issue_agent_ticker import IssueAgentTicker\n` +
              `ticker, _ = IssueAgentTicker.objects.get_or_create(issue_id=${JSON.stringify(issue.id)}, defaults={\n` +
              `    "user_disabled": False, "next_run_at": None, "used": 0, "granted": 0,\n` +
              `    "enabled": False, "disarm_reason": ""})\n` +
              `ticker.enabled = False\n` +
              `ticker.disarm_reason = ${JSON.stringify(disarmReason)}\n` +
              `ticker.save(update_fields=["enabled", "disarm_reason"])\n` +
              `print("PARITY_TICKER_DISARM:" + ticker.disarm_reason)\n`
          );
          // The retrieve carries the ticker + disarm_reason for the panel.
          const record = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
          const ticker =
            (record["agent_ticker"] as Record<string, unknown> | null) ??
            (record["agent_status"] as Record<string, unknown>)?.["ticker"];
          expect(ticker).not.toBeNull();
          // disarm_reason and enabled are the stored clock state.
          expect((ticker as Record<string, unknown>)["disarm_reason"]).toBe(disarmReason);
          expect((ticker as Record<string, unknown>)["enabled"]).toBe(false);
        });
        await test.step("open the work item and see the paused card", async () => {
          await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
          // Both apps render the failure-policy card with a warning title containing paused/failure language,
          // a Needs attention badge, and a detail mentioning the failure comment and Re-tick.
          // The exact copy differs by app (old: "AI agent ticking is paused after a failure", new: "Agent ticking is paused after a failure"),
          // so assert the semantic intent: paused + failure + attention + failure comment + Re-tick.
          await expect
            .poll(async () => driver.page.getByText(/paused after a failure/i).count(), { timeout: 30_000 })
            .toBeGreaterThan(0);
          await expect
            .poll(
              async () =>
                (await driver.page
                  .getByText(/paused after a failure/i)
                  .first()
                  .textContent()) ?? "",
              { timeout: 30_000 }
            )
            .toMatch(/paused after a failure/i);
          await expect
            .poll(async () => driver.page.getByText(/Needs attention/i).count(), { timeout: 30_000 })
            .toBeGreaterThan(0);
          const detail = (await driver.page.locator("text=/failure comment/i").first().textContent()) ?? "";
          expect(detail).toMatch(/failure comment/i);
          await expect
            .poll(async () => driver.page.getByText(/failure comment/i).count(), { timeout: 30_000 })
            .toBeGreaterThan(0);
          await expect
            .poll(async () => driver.page.getByText(/Re-tick/i).count(), { timeout: 30_000 })
            .toBeGreaterThan(0);
          // Generic "off" card must not win for failure stops.
          expect(await driver.page.getByText(/Agent ticking off/i).count()).toBe(0);
        });
      } finally {
        await dropIssue(seed, session, issue.id);
      }
    }
  );
}
