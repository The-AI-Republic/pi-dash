// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-393): the project automations tab. The
// auto-archive row applies a one-month default when enabled and expands
// to a month picker with presets plus a custom whole-months dialog that
// rejects out-of-range values; the auto-close row applies a one-month
// default plus a default cancelled state and adds a state picker limited
// to cancelled-group states; non-admins get the denial view. The
// PSET-061 loader slice is a bug: scenario: the row skeletons never
// render on a fresh open (the route wrapper blanks until the retrieve
// that also fills the rows' store), so the tab settles on rows with no
// loader phase (NEWFRONT-397).
// Rows: PSET-053, PSET-054, PSET-061.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCompleteOnboarding,
  serverCreateProjectWithFlags,
  serverPatchProject,
  serverProjectAutomationFields,
  serverProjectStates,
  serverProvisionWorkspaceMember,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["PSET-053"], "admins configure auto-archive with presets and a validated custom range"),
  { tag: specTags(["PSET-053"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 archive ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, { archive_in: 0, close_in: 0 });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectAutomationsOpenTab(seed.workspaceSlug, projectId);

      await test.step("the row starts switched off with no delay picker", async () => {
        expect(await driver.projectAutomationsNotAuthorized()).toBe(false);
        expect(await driver.projectAutomationsArchiveOn()).toBe(false);
        expect(await driver.projectAutomationsDelayTriggerText()).toBeNull();
      });

      await test.step("enabling applies the one-month default", async () => {
        await driver.projectAutomationsToggleArchive();
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn, {
            timeout: 20_000,
          })
          .toBe(1);
        expect(await driver.projectAutomationsArchiveOn()).toBe(true);
        expect(await driver.projectAutomationsDelayTriggerText()).toBe("1 month");
      });

      await test.step("the picker offers presets and a custom action", async () => {
        await driver.projectAutomationsOpenDelayOptions();
        expect(await driver.projectAutomationsDelayOptionLabels()).toEqual(
          expect.arrayContaining(["1 month", "3 months", "6 months", "9 months", "12 months"])
        );
      });

      await test.step("a preset persists its month count", async () => {
        await driver.projectAutomationsPickDelayOption("3 months");
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn, {
            timeout: 20_000,
          })
          .toBe(3);
        expect(await driver.projectAutomationsDelayTriggerText()).toBe("3 months");
      });

      await test.step("the custom dialog rejects out-of-range months", async () => {
        await driver.projectAutomationsOpenDelayOptions();
        await driver.projectAutomationsOpenCustomize();
        expect(await driver.projectAutomationsMonthDialogOpen()).toBe(true);
        // The number input carries native min/max, so 13 never submits:
        // no request fires, the dialog stays open, and the app's range
        // message (which only the empty case triggers) stays absent.
        await driver.projectAutomationsMonthDialogSubmit("13");
        expect(await driver.projectAutomationsMonthDialogOpen()).toBe(true);
        expect(await driver.projectAutomationsMonthDialogError()).toBeNull();
        expect((await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn).toBe(3);
      });

      await test.step("an empty value shows the range message", async () => {
        await driver.projectAutomationsMonthDialogSubmit("");
        expect(await driver.projectAutomationsMonthDialogOpen()).toBe(true);
        expect(await driver.projectAutomationsMonthDialogError()).toBe("Select a month between 1 and 12.");
        expect((await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn).toBe(3);
      });

      await test.step("a valid custom value persists and closes the dialog", async () => {
        await driver.projectAutomationsMonthDialogSubmit("6");
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn, {
            timeout: 20_000,
          })
          .toBe(6);
        expect(await driver.projectAutomationsMonthDialogOpen()).toBe(false);
        expect(await driver.projectAutomationsDelayTriggerText()).toBe("6 months");
      });

      await test.step("disabling clears the delay and collapses the picker", async () => {
        await driver.projectAutomationsToggleArchive();
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).archiveIn, {
            timeout: 20_000,
          })
          .toBe(0);
        expect(await driver.projectAutomationsArchiveOn()).toBe(false);
        expect(await driver.projectAutomationsDelayTriggerText()).toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-054"], "admins configure auto-close with a cancelled default state"),
  { tag: specTags(["PSET-054"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 close ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, {
      archive_in: 0,
      close_in: 0,
      default_state: null,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectAutomationsOpenTab(seed.workspaceSlug, projectId);
      expect(await driver.projectAutomationsCloseOn()).toBe(false);

      await test.step("enabling applies the one-month default plus a cancelled state", async () => {
        await driver.projectAutomationsToggleClose();
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).closeIn, {
            timeout: 20_000,
          })
          .toBe(1);
        const settled = await serverProjectAutomationFields(seed.workspaceSlug, projectId, session);
        expect(typeof settled.defaultState).toBe("string");
        const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
        expect(states.find((row) => row.id === settled.defaultState)?.group).toBe("cancelled");
        expect(await driver.projectAutomationsCloseOn()).toBe(true);
        expect(await driver.projectAutomationsDelayTriggerText()).toBe("1 month");
      });

      await test.step("the state picker names the cancelled default", async () => {
        const settled = await serverProjectAutomationFields(seed.workspaceSlug, projectId, session);
        const states = await serverProjectStates(seed.workspaceSlug, projectId, session);
        const current = states.find((row) => row.id === settled.defaultState)?.name ?? "";
        expect(current).not.toBe("");
        expect(await driver.projectAutomationsCloseStateText()).toContain(current);
        // The picker disables itself when there is nothing to choose
        // between: exactly one cancelled state means no choice.
        const cancelledCount = states.filter((row) => row.group === "cancelled").length;
        expect(await driver.projectAutomationsCloseStateDisabled()).toBe(cancelledCount < 2);
      });

      await test.step("a delay preset persists its month count", async () => {
        await driver.projectAutomationsOpenDelayOptions();
        await driver.projectAutomationsPickDelayOption("6 months");
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).closeIn, {
            timeout: 20_000,
          })
          .toBe(6);
        expect(await driver.projectAutomationsDelayTriggerText()).toBe("6 months");
      });

      await test.step("disabling clears the delay and the default state", async () => {
        await driver.projectAutomationsToggleClose();
        await expect
          .poll(async () => (await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).closeIn, {
            timeout: 20_000,
          })
          .toBe(0);
        expect((await serverProjectAutomationFields(seed.workspaceSlug, projectId, session)).defaultState).toBeNull();
        expect(await driver.projectAutomationsCloseOn()).toBe(false);
        expect(await driver.projectAutomationsCloseStateText()).toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-053", "PSET-054"], "non-admin members get the denial view with no automation rows"),
  { tag: specTags(["PSET-053", "PSET-054"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 autdeny ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf393autdeny", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    try {
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.projectAutomationsOpenTab(seed.workspaceSlug, projectId);
      expect(await driver.projectAutomationsNotAuthorized()).toBe(true);
      expect(await driver.projectAutomationsArchiveOn()).toBe(false);
      expect(await driver.projectAutomationsCloseOn()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-061"], "bug: NEWFRONT-397 the automations tab settles on rows with no loader phase"),
  { tag: specTags(["PSET-061"]) },
  async ({ driver, seed }) => {
    // The row skeletons are unreachable on a fresh open: the route
    // wrapper blanks the content until the project retrieve resolves,
    // and that retrieve fills the shared map the rows read — so the
    // rows mount with data and the Loader branch never renders. This
    // scenario stretches the loading window with a held retrieve and
    // proves no skeleton appears anywhere inside it; NEWFRONT-397
    // tracks the intended loader-until-loaded behavior.
    const tag = `NF393 autload ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectAutomationsDelayDetailsFetch(4_000);
      const opened = driver.projectAutomationsOpenTab(seed.workspaceSlug, projectId);
      let sawLoader = false;
      for (let sample = 0; sample < 20 && !sawLoader; sample++) {
        sawLoader = await driver.projectAutomationsLoaderVisible();
        await new Promise((resolve) => setTimeout(resolve, 200));
      }
      await opened;
      if (driver.target === "web_new") {
        // Intended (NEWFRONT-397, NEWFRONT-606): the skeleton shows while
        // the held retrieve is in flight, then yields to the rows.
        expect(sawLoader).toBe(true);
      } else {
        expect(sawLoader).toBe(false);
      }
      expect(await driver.projectAutomationsLoaderVisible()).toBe(false);
      expect(await driver.projectAutomationsArchiveOn()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
