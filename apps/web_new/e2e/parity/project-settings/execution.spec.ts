// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): execution target and automated-run
// policy on the General tab — no default execution target is offered
// (Runner is the only executor, built-in engine design §6), the master switch leaves budget and cadence
// visible but inert while off, the budget validates as whole numbers with
// a remembering no-cap toggle, and the cadence picker carries presets plus
// a custom marker. Rows: PSET-009.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateProjectWithFlags,
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
  specTitle(["PSET-009"], "no default execution target is offered; the project stays on runners"),
  { tag: specTags(["PSET-009"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 exec ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await throwawayProject(seed, session, tag);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("the executor choice is gone", async () => {
        // Runner is the only executor (built-in engine design §6): the
        // Cloud Agent and built-in agent are no longer choices, so the tab
        // renders no default execution-target control.
        expect(await driver.settingsGeneralExecutorPresent()).toBe(false);
      });

      await test.step("a save keeps the project on runners", async () => {
        await driver.settingsGeneralFill("description", `${tag} touch`);
        await driver.settingsGeneralSave();
        await expect
          .poll(
            async () => String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["description"]),
            { timeout: 30_000 }
          )
          .toBe(`${tag} touch`);
        const after = await serverProjectRecord(seed.workspaceSlug, projectId, session);
        expect(String(after["default_agent_executor"] ?? "local_runner")).toBe("local_runner");
      });
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
