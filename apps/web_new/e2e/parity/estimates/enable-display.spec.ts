// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-322): estimates enable switch (EST-004),
// the active system display with humanized durations (EST-005), the
// read-only archived section with its outbound link (EST-006), and
// admin-only row controls with the OSS delete-only split (EST-007).
// Rows: EST-004–EST-007.
//
// One browser role per test: the shared sign-in helper keeps an
// existing session, so switching users mid-test would silently keep
// the first session. Fresh contexts per test give each role a clean
// sign-in instead.
//
// Non-admin note (EST-004/EST-007): the settings route itself renders
// the not-authorized view for viewers without project admin rights, so
// there is no reachable disabled-switch or read-only-row state to pin
// down — the scenarios assert the denial view carries no switch and no
// row controls at all.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateArchivedEstimate,
  serverCreateEstimate,
  serverCreateProjectWithFlags,
  serverEstimatesList,
  serverProjectEstimateId,
  serverProjectMembers,
  serverProvisionWorkspaceMember,
  serverSetProjectEstimate,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

/** Remove a provisioned workspace member; lenient for finally-blocks. */
async function dropProvisionedMember(seed: ParitySeedFacts, userId: string, session: string): Promise<void> {
  const membership = (await serverWorkspaceMembers(seed.workspaceSlug, session).catch(() => [])) as {
    membershipId: string;
    userId: string;
  }[];
  const mine = membership.find((row) => row.userId === userId);
  if (mine) await serverCleanupWorkspaceMember(seed.workspaceSlug, mine.membershipId, session);
}

test(
  specTitle(["EST-004"], "admins flip the enable switch on and off with success notices"),
  { tag: specTags(["EST-004"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 flip ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);

      await test.step("an unlinked project shows the switch off", async () => {
        expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
        expect(await driver.estimatesEnableSwitchVisible()).toBe(true);
        expect(await driver.estimatesEnableSwitchDisabled()).toBe(false);
        expect(await driver.estimatesEnableSwitchOn()).toBe(false);
      });

      await test.step("flipping on links the project with a success notice", async () => {
        await driver.estimatesEnableSwitchToggle();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("enabled");
        expect(await driver.estimatesEnableSwitchOn()).toBe(true);
        await expect.poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBe(system.id);
      });

      await test.step("flipping off unlinks the project with a success notice", async () => {
        await driver.estimatesEnableSwitchToggle();
        // Direction-specific: the first flip's notice may still show, so
        // "Success" alone could match the stale toast.
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("disabled");
        expect(await driver.estimatesEnableSwitchOn()).toBe(false);
        await expect.poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-004"], "non-admin members get no enable switch on estimates settings"),
  { tag: specTags(["EST-004"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 gate ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf322gate", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === member.userId)
          ?.role
      ).toBe(15);
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await dropProvisionedMember(seed, member.userId, session);
    }
  }
);

test(
  specTitle(["EST-004"], "a refused project update shows the error notice and leaves state unchanged"),
  { tag: specTags(["EST-004"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 fail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesEnableSwitchOn()).toBe(false);

      // Refuse the project write behind the switch's back: the toggle
      // must report the error and roll back instead of sticking. Only
      // the error title is asserted: the notice body always names the
      // disable direction even when enabling fails (NEWFRONT-341).
      await driver.estimatesFailProjectUpdate();
      await driver.estimatesEnableSwitchToggle();
      // The oracle pins the old "Error" notice title; web_new reports
      // the same refusal in its own words. The error notice itself is
      // the parity point, so each leg asserts its own copy.
      if (driver.target === "web_new") {
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toMatch(/couldn.t change estimates/i);
      } else {
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Error");
      }
      expect(await driver.estimatesEnableSwitchOn()).toBe(false);
      expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
      expect(system.id).not.toBe("");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-005"], "the active system shows its name and every point value"),
  { tag: specTags(["EST-005"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 live ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    const values = ["1", "2", "3", "5", "8"];
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, values, session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      const listed = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      expect(listed.find((row) => row.id === system.id)?.lastUsed).toBe(true);
      expect(
        listed
          .find((row) => row.id === system.id)
          ?.points.map((point) => point.value)
          .sort()
      ).toEqual([...values].sort());

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
      const preview = await driver.estimatesActiveSystemValues();
      for (const value of values) expect(preview).toContain(value);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-005"], "duration-family values render humanized instead of raw numbers"),
  { tag: specTags(["EST-005"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 time ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    // Minutes on the wire; the screen must show hours/minutes form.
    const system = await serverCreateEstimate(
      seed.workspaceSlug,
      projectId,
      `${tag} clock`,
      ["90", "150"],
      session,
      "time"
    );
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      const listed = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      expect(
        listed
          .find((row) => row.id === system.id)
          ?.points.map((point) => point.value)
          .sort()
      ).toEqual(["90", "150"].sort());

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} clock`);
      const preview = await driver.estimatesActiveSystemValues();
      expect(preview).toContain("1h 30m");
      expect(preview).toContain("2h 30m");
      expect(preview).not.toContain("90");
      expect(preview).not.toContain("150");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-006"], "replaced systems list read-only with an explainer and an outbound link"),
  { tag: specTags(["EST-006"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 past ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    const active = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    const first = await serverCreateArchivedEstimate(
      seed.workspaceSlug,
      projectId,
      `${tag} old one`,
      ["1", "3"],
      session
    );
    const second = await serverCreateArchivedEstimate(
      seed.workspaceSlug,
      projectId,
      `${tag} old two`,
      ["2", "5"],
      session
    );
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, active.id, session);
    try {
      await test.step("the server reports one in-use system and two dormant ones", async () => {
        expect(first.lastUsed).toBe(false);
        expect(second.lastUsed).toBe(false);
        const listed = await serverEstimatesList(seed.workspaceSlug, projectId, session);
        expect(listed.find((row) => row.id === active.id)?.lastUsed).toBe(true);
        expect(listed.find((row) => row.id === first.id)?.lastUsed).toBe(false);
        expect(listed.find((row) => row.id === second.id)?.lastUsed).toBe(false);
      });

      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);

      await test.step("the archived section lists both replaced systems without controls", async () => {
        await expect.poll(() => driver.estimatesArchivedNames(), { timeout: 30_000 }).toContain(`${tag} old one`);
        expect(await driver.estimatesArchivedNames()).toContain(`${tag} old two`);
        expect(await driver.estimatesArchivedNames()).not.toContain(`${tag} prime`);
        expect(await driver.estimatesArchivedRowButtonCount(`${tag} old one`)).toBe(0);
        expect(await driver.estimatesArchivedRowButtonCount(`${tag} old two`)).toBe(0);
      });

      await test.step("the explainer and outbound link open outside the app", async () => {
        expect(await driver.estimatesArchivedExplainerText()).toContain("older versions");
        const href = await driver.estimatesArchivedLinkHref();
        expect(href).toMatch(/^https:\/\//);
        expect(await driver.estimatesArchivedLinkTarget()).toBe("_blank");
        const opened = await driver.estimatesArchivedOpenLinkNewPage();
        expect(opened).toContain(new URL(href).host);
        expect(opened).not.toContain("/settings/projects/");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-007"], "admins get a working delete-only control on the active row"),
  { tag: specTags(["EST-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 rows ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);

      // OSS ships deletion only: exactly one row button, no edit action.
      expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
      expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
      await driver.estimatesActiveRowDeleteClick();
      await expect.poll(() => driver.estimatesConfirmDialogOpen(), { timeout: 20_000 }).toBe(true);
      await driver.estimatesConfirmDialogCancel();
      expect(await driver.estimatesConfirmDialogOpen()).toBe(false);

      // Cancelling kept everything: the system still lists server-side.
      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-007"], "non-admin rows expose no edit or delete affordance"),
  { tag: specTags(["EST-007"]) },
  async ({ driver, seed }) => {
    const tag = `NF322 read ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf322read", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N322"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === member.userId)
          ?.role
      ).toBe(15);
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      // The denial view replaces the whole screen: no rows, no switch,
      // and therefore no row control of any kind.
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(false);
      expect(await driver.estimatesActiveSystemName()).toBe("");
      expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await dropProvisionedMember(seed, member.userId, session);
    }
  }
);
