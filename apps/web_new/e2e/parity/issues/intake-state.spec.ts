// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): intake-state dropdown — the
// intake-create modal's state picker is a searchable single-select over
// the project's intake states with no clear row, while the triage screen
// renders the same State row disabled. Rows: ISS-217 (intake-state
// dropdown).
//
// Observed behavior notes (inventory row carries them at update time):
// fresh projects carry exactly one intake state ("Triage", group triage,
// default flag false — so the trigger shows the first-state fallback);
// the state endpoint answers a single object and rejects POST; the
// create-project endpoint ignores inbox_view, which needs an explicit
// PATCH (like estimate activation); inbox detail/delete endpoints take
// the nested issue id, not the inbox row id.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupInboxIssue,
  serverCleanupProject,
  serverCreateInboxIssue,
  serverCreateProjectWithFlags,
  serverInboxIssue,
  serverInboxIssues,
  serverIntakeStates,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-217"], "intake-create state picker searches, picks and persists the state"),
  { tag: specTags(["ISS-217"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 intake ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { inboxView: true },
      session
    );
    const states = await serverIntakeStates(seed.workspaceSlug, projectId, session);
    expect(states.length).toBeGreaterThan(0);
    const triage = states[0]!;
    const title = `${tag} issue`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.intakeCreateOpen(seed.workspaceSlug, projectId);

      await test.step("the trigger defaults to the first intake state", async () => {
        await expect.poll(() => driver.intakeStateValue(), { timeout: 15_000 }).toContain(triage.name);
      });

      await test.step("the picker lists states with no clear row and searches them", async () => {
        await driver.intakeStateOpenPicker();
        const options = await driver.intakeStateOptionTexts();
        expect(options).toEqual(states.map((s) => expect.stringContaining(s.name)));
        expect(options.some((o) => /clear|none/i.test(o))).toBe(false);
        await driver.intakeStateSearch(triage.name.slice(0, 3));
        await expect.poll(() => driver.intakeStateOptionTexts(), { timeout: 10_000 }).toEqual([triage.name]);
        await driver.intakeStateSearch("-no-such-state-");
        await expect.poll(() => driver.intakeStateEmptyText(), { timeout: 10_000 }).toBe("No matching results");
        await driver.pickerPressEscape();
      });

      await test.step("picking a state sticks and the created issue carries it", async () => {
        await driver.intakeStateOpenPicker();
        await driver.intakeStatePick(triage.name);
        await expect.poll(() => driver.intakeStateValue(), { timeout: 10_000 }).toContain(triage.name);
        await driver.intakeCreateFillTitle(title);
        await driver.intakeCreateSubmit();
        let issueId = "";
        await expect
          .poll(
            async () => {
              const found = (await serverInboxIssues(seed.workspaceSlug, projectId, session)).find(
                (r) => r.name === title
              );
              if (found !== undefined) issueId = found.issueId;
              return found?.issueId;
            },
            { timeout: 15_000 }
          )
          .not.toBe(undefined);
        const created = await serverInboxIssue(seed.workspaceSlug, projectId, issueId, session);
        expect(created.stateId).toBe(triage.id);
      });
    } finally {
      const doomed = (await serverInboxIssues(seed.workspaceSlug, projectId, session)).find((r) => r.name === title);
      if (doomed !== undefined && doomed.issueId !== "")
        await serverCleanupInboxIssue(seed.workspaceSlug, projectId, doomed.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-217"], "triage screen renders the state row disabled"),
  { tag: specTags(["ISS-217"]) },
  async ({ driver, seed }) => {
    const tag = `NF122 triage ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      { inboxView: true },
      session
    );
    const inbox = await serverCreateInboxIssue(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.rulesOpenIntakeIssue(seed.workspaceSlug, projectId, inbox.issueId);
      await expect.poll(() => driver.rulesIntakeTriageVisible(), { timeout: 30_000 }).toBe(true);
      expect(await driver.intakeTriageStateDisabled()).toBe(true);
    } finally {
      await serverCleanupInboxIssue(seed.workspaceSlug, projectId, inbox.issueId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
