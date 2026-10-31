// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): execution target and automated-run
// policy on the General tab — the picker offers the runner families with
// unavailable ones disabled, the master switch leaves budget and cadence
// visible but inert while off, the budget validates as whole numbers with
// a remembering no-cap toggle, and the cadence picker carries presets plus
// a custom marker. Rows: PSET-009.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverIssueRecord,
  serverPatchProject,
  serverProjectRecord,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

async function throwawayProject(seed: { workspaceSlug: string }, session: string, tag: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N389"),
    {},
    session
  );
}

test(
  specTitle(["PSET-009"], "execution target switches persist; unavailable families stay disabled"),
  { tag: specTags(["PSET-009"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 exec ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await throwawayProject(seed, session, tag);
    try {
      const before = await serverProjectRecord(seed.workspaceSlug, projectId, session);
      const options = (before["agent_executor_options"] as { kind: string; available: boolean }[] | null) ?? [];
      const current = String(before["default_agent_executor"] ?? "local_runner");
      const alternative = options.find((option) => option.available && option.kind !== current)?.kind ?? null;
      // An issue created before the switch keeps its executor afterwards.
      const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
      const issueBefore = await serverIssueRecord(seed.workspaceSlug, projectId, issue.id, session);

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("unavailable families cannot be picked", async () => {
        expect(await driver.settingsGeneralExecutorValue()).toBe(current);
        // Local runner is the unconditional default fallback: it stays
        // selectable even while the stack reports no_local_runner. The
        // provisioned families gate on availability instead.
        expect(await driver.settingsGeneralExecutorOptionDisabled("local_runner")).toBe(false);
        // The family availability arrives with the details fetch, just
        // after first paint; poll until the picker reflects it.
        for (const option of options) {
          if (option.kind !== "local_runner" && !option.available) {
            await expect
              .poll(() => driver.settingsGeneralExecutorOptionDisabled(option.kind), { timeout: 30_000 })
              .toBe(true);
          }
        }
      });

      if (alternative !== null) {
        await test.step("switching target persists for new runs only", async () => {
          await driver.settingsGeneralSetExecutor(alternative);
          await driver.settingsGeneralSave();
          await expect
            .poll(
              async () =>
                String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["default_agent_executor"]),
              { timeout: 30_000 }
            )
            .toBe(alternative);
          const issueAfter = await serverIssueRecord(seed.workspaceSlug, projectId, issue.id, session);
          expect(issueAfter["agent_executor"] ?? null).toBe(issueBefore["agent_executor"] ?? null);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-009"], "ticking switch, budget validation, no-cap memory and cadence round-trip"),
  { tag: specTags(["PSET-009"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 policy ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await throwawayProject(seed, session, tag);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("a zero budget cannot save and the server keeps its value", async () => {
        // The budget input carries a native floor of 1, which blocks the
        // submit before form validation ever runs: no notice, no request.
        const before = await serverProjectRecord(seed.workspaceSlug, projectId, session);
        await driver.settingsGeneralSetBudget("0");
        await driver.settingsGeneralSave();
        expect(await driver.rulesLastToast()).toBeNull();
        expect(
          String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_default_max_ticks"])
        ).toBe(String(before["agent_default_max_ticks"]));
      });

      await test.step("a cleared budget is refused with a field error", async () => {
        const before = await serverProjectRecord(seed.workspaceSlug, projectId, session);
        await driver.settingsGeneralSetBudget("");
        await driver.settingsGeneralSave();
        expect(await driver.settingsGeneralFieldError("budget")).not.toBe("");
        expect(
          String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_default_max_ticks"])
        ).toBe(String(before["agent_default_max_ticks"]));
      });

      await test.step("no-cap persists the sentinel and remembers the typed value", async () => {
        await driver.settingsGeneralSetBudget("7");
        await driver.settingsGeneralSetNoCap(true);
        await driver.settingsGeneralSave();
        await expect
          .poll(
            async () =>
              String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_default_max_ticks"]),
            { timeout: 30_000 }
          )
          .toBe("-1");
        expect(await driver.settingsGeneralBudgetValue()).toBe("");
        await driver.settingsGeneralSetNoCap(false);
        expect(await driver.settingsGeneralBudgetValue()).toBe("7");
      });

      await test.step("switching ticking off leaves budget and cadence visible but inert", async () => {
        await driver.settingsGeneralSetTicking(false);
        await driver.settingsGeneralSave();
        expect(
          String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_ticking_enabled"])
        ).toBe("false");
        expect(await driver.settingsGeneralBudgetValue()).not.toBe("");
        expect(await driver.settingsGeneralFieldDisabled("budget")).toBe(true);
        expect(await driver.settingsGeneralCadenceText()).not.toBe("");
        await driver.settingsGeneralSetTicking(true);
        await driver.settingsGeneralSave();
      });

      await test.step("a cadence preset persists to every ticking stage", async () => {
        await driver.settingsGeneralSetCadence("6 hours");
        await driver.settingsGeneralSave();
        await expect
          .poll(
            async () =>
              String(
                (await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_default_interval_seconds"]
              ),
            { timeout: 30_000 }
          )
          .toBe("21600");
        const record = await serverProjectRecord(seed.workspaceSlug, projectId, session);
        expect(String(record["agent_review_default_interval_seconds"])).toBe("21600");
        expect(String(record["agent_test_default_interval_seconds"])).toBe("21600");
        expect(await driver.settingsGeneralCadenceText()).toBe("6 hours");
      });

      await test.step("an unrelated save leaves out-of-band per-stage rhythms alone", async () => {
        // Reload first: dirty flags survive a save within one form session,
        // so only a fresh load isolates a description-only save.
        await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);
        await serverPatchProject(seed.workspaceSlug, projectId, session, {
          agent_review_default_interval_seconds: 3600,
        });
        await driver.settingsGeneralFill("description", `${tag} touch`);
        await driver.settingsGeneralSave();
        expect(
          String(
            (await serverProjectRecord(seed.workspaceSlug, projectId, session))["agent_review_default_interval_seconds"]
          )
        ).toBe("3600");
      });

      await test.step("an externally set cadence renders a custom marker", async () => {
        await serverPatchProject(seed.workspaceSlug, projectId, session, { agent_default_interval_seconds: 28800 });
        await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.settingsGeneralCadenceText(), { timeout: 30_000 }).toContain("Custom");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
