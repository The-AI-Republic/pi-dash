// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-393): the estimates settings entry and the
// github sync tab. The estimates tab gates on project admin and embeds
// the shared component (loader, illustrated empty state with a creation
// action, active system with deletion only and no edit affordance in
// OSS per EST-014, read-only archived systems, enable switch owning the
// active reference). The github tab is member-visible:
// a skeleton while the binding fetch is unresolved, then a bound card
// (provider family, repository link, mirroring contract, four-hour
// cadence, failure line) with an admin-only sync switch and Unbind, or
// an unbound guidance card pointing at General settings with no branch
// or direction pickers.
//
// Seed coupling (deliberate, documented): binding a repository through
// the API needs a provider OAuth token the seeded stack cannot mint, so
// the bound-card scenarios run read-only against the seed project's
// planted `parity/parity-seed` binding, and the sync toggle flips twice
// to restore seed state. The destructive Unbind action runs on a
// scratch project with a shell-planted binding instead: removing the
// seed binding would break the git-synced comment scenarios (CMT-005)
// sharing this stack.
// Rows: PSET-049–052, PSET-061.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCompleteOnboarding,
  serverCreateArchivedEstimate,
  serverCreateIssue,
  serverCreateProjectWithFlags,
  serverEstimatesList,
  serverIssueDetails,
  serverPatchIssue,
  serverPlantRepositoryBinding,
  serverProjectEstimateId,
  serverProvisionWorkspaceMember,
  serverRepositoryBinding,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

function requireMention(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.mentionMember) throw new Error("[parity] seed has no mention member; rerun parity-up.sh.");
  return { email: seed.mentionMember.email, password: seed.mentionMember.password };
}

test(
  specTitle(["PSET-049"], "estimates entry runs loader, empty-create, active delete-only and archived read-only"),
  { tag: specTags(["PSET-049"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 est ${Date.now()}`;
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

      await test.step("a loader covers the section while the list fetch is held", async () => {
        await driver.estimatesDelayListFetch(3_000);
        const opened = driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.estimatesSkeletonVisible(), { timeout: 30_000 }).toBe(true);
        await opened;
      });

      await test.step("the empty state offers creation", async () => {
        expect(await driver.estimatesEmptyVisible()).toBe(true);
        expect(await driver.estimatesCreateDialogOpen()).toBe(false);
        await driver.estimatesEmptyAddClick();
        await expect.poll(() => driver.estimatesCreateDialogOpen(), { timeout: 20_000 }).toBe(true);
      });

      let createdId = "";
      await test.step("creating through the dialog yields an active system with deletion only", async () => {
        await driver.estimatesPickStarter("Custom");
        await driver.estimatesSubmit();
        expect(await driver.estimatesDialogOpen()).toBe(false);
        const systems = await serverEstimatesList(seed.workspaceSlug, projectId, session);
        expect(systems).toHaveLength(1);
        expect(systems[0]?.name ?? "").not.toBe("");
        createdId = systems[0]?.id ?? "";
        expect(createdId).not.toBe("");
        // The list refetch lands after the dialog closes (same as the
        // states create in PSET-029), so poll for the active row.
        await expect
          .poll(() => driver.estimatesActiveSystemName(), { timeout: 20_000 })
          .toContain(systems[0]?.name ?? "");
        // OSS shape per the estimates area (EST-014): the active row
        // carries a single delete control and no edit affordance.
        expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
        expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(false);
        await driver.estimatesActiveRowDeleteClick();
        await expect
          .poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 })
          .toContain("Delete");
        expect(await driver.estimatesEditDeleteConfirmDialogTitle()).not.toMatch(/update/i);
        await driver.estimatesConfirmDialogCancel();
        expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
        expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
          systems[0]?.id
        );
      });

      await test.step("archived systems list read-only", async () => {
        const archivedName = `${tag} old`;
        await serverCreateArchivedEstimate(seed.workspaceSlug, projectId, archivedName, ["1", "2"], session);
        await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
        expect(await driver.estimatesArchivedSectionVisible()).toBe(true);
        expect(await driver.estimatesArchivedNames()).toContain(archivedName);
        expect(await driver.estimatesArchivedRowButtonCount(archivedName)).toBe(0);
      });

      await test.step("the enable switch sets and clears the active reference", async () => {
        // The switch links the store's current active id (the created
        // system), not list position: an archived system now shares the
        // list, so the id captured at creation is the expectation.
        expect(createdId).not.toBe("");
        expect(await serverProjectEstimateId(seed.workspaceSlug, projectId, session)).toBeNull();
        expect(await driver.estimatesEnableSwitchOn()).toBe(false);
        await driver.estimatesEnableSwitchToggle();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("enabled");
        await expect
          .poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session), { timeout: 20_000 })
          .toBe(createdId);
        await driver.estimatesEnableSwitchToggle();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("disabled");
        await expect
          .poll(() => serverProjectEstimateId(seed.workspaceSlug, projectId, session), { timeout: 20_000 })
          .toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-049"], "non-admin members get no estimates settings"),
  { tag: specTags(["PSET-049"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 estdeny ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf393estdeny", session);
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
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-050"], "bound projects show provider, link, contract, cadence and sync times"),
  { tag: specTags(["PSET-050"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const binding = await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session);
    expect(binding.bound).toBe(true);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.projectGithubOpenTab(seed.workspaceSlug, seed.projectId);

    await test.step("the card names the provider family and links the repository", async () => {
      expect(await driver.projectGithubNotAuthorized()).toBe(false);
      expect(await driver.projectGithubBoundVisible()).toBe(true);
      expect(await driver.projectGithubProviderLabel()).toBe("GitHub");
      expect(await driver.projectGithubRepoLinkText()).toBe("parity/parity-seed");
      expect(binding.provider).toBe("github");
      expect(binding.fullName).toBe("parity/parity-seed");
    });

    await test.step("the mirroring contract and cadence render as plain text", async () => {
      expect(await driver.projectGithubContractVisible()).toBe(true);
      expect(await driver.projectGithubCadenceText()).toContain("Sync issues every 4 hours");
    });

    await test.step("the failure line renders as plain error text when present", async () => {
      // Re-run safe: a sync tripped by an earlier toggle run may have
      // recorded a failure since. Absent maps to no line; present maps to
      // the plain "Last error:" text.
      const apiError = (binding.lastSyncError as string | null) ?? "";
      const error = await driver.projectGithubLastError();
      if (apiError === "") {
        expect(error).toBeNull();
      } else {
        expect(error ?? "").toContain("Last error:");
      }
    });
  }
);

test(
  specTitle(["PSET-050"], "members may view the bound card with admin controls disabled"),
  { tag: specTags(["PSET-050"]) },
  async ({ driver, seed }) => {
    const mention = requireMention(seed);
    await driver.rulesEnsureSignedIn(mention.email, mention.password, seed.workspaceSlug);
    await driver.projectGithubOpenTab(seed.workspaceSlug, seed.projectId);
    expect(await driver.projectGithubNotAuthorized()).toBe(false);
    expect(await driver.projectGithubBoundVisible()).toBe(true);
    expect(await driver.projectGithubProviderLabel()).toBe("GitHub");
    expect(await driver.projectGithubSyncDisabled()).toBe(true);
    expect(await driver.projectGithubUnbindVisible()).toBe(true);
    expect(await driver.projectGithubUnbindDisabled()).toBe(true);
  }
);

test(
  specTitle(["PSET-051"], "admins flip repository sync with the binding left intact"),
  { tag: specTags(["PSET-051"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    // Flip twice and restore: whatever state an earlier run left, this
    // test returns the seed binding to it.
    const initial = (await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).syncEnabled;
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.projectGithubOpenTab(seed.workspaceSlug, seed.projectId);
    expect(await driver.projectGithubSyncDisabled()).toBe(false);
    expect(await driver.projectGithubSyncOn()).toBe(initial);

    await driver.projectGithubToggleSync();
    await expect
      .poll(async () => (await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).syncEnabled, {
        timeout: 20_000,
      })
      .toBe(!initial);
    expect(await driver.projectGithubSyncOn()).toBe(!initial);

    await driver.projectGithubToggleSync();
    await expect
      .poll(async () => (await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).syncEnabled, {
        timeout: 20_000,
      })
      .toBe(initial);
    expect(await driver.projectGithubSyncOn()).toBe(initial);
    // The seed binding survives: the destructive unbind half runs on a
    // scratch project with a planted binding in the next scenario.
    expect(await driver.projectGithubUnbindVisible()).toBe(true);
    expect(await driver.projectGithubUnbindDisabled()).toBe(false);
    expect((await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).bound).toBe(true);
  }
);

test(
  specTitle(["PSET-051"], "admins unbind confirm-free and mirrored issues stay editable"),
  { tag: specTags(["PSET-051"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 unbind ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    const issueId = await serverCreateIssue(seed.workspaceSlug, projectId, session, `${tag} mirrored`);
    await serverPlantRepositoryBinding(seed.workspaceSlug, projectId, issueId);
    try {
      expect((await serverRepositoryBinding(seed.workspaceSlug, projectId, session)).bound).toBe(true);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectGithubOpenTab(seed.workspaceSlug, projectId);

      await test.step("the planted binding renders its bound card", async () => {
        expect(await driver.projectGithubBoundVisible()).toBe(true);
        expect(await driver.projectGithubProviderLabel()).toBe("GitHub");
        expect(await driver.projectGithubRepoLinkText()).toBe("parity/parity-seed");
        expect(await driver.projectGithubUnbindDisabled()).toBe(false);
      });

      await test.step("one click unbinds with no confirmation", async () => {
        await driver.projectGithubUnbind();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Mirrored issues remain editable");
        await expect
          .poll(async () => (await serverRepositoryBinding(seed.workspaceSlug, projectId, session)).bound, {
            timeout: 20_000,
          })
          .toBe(false);
        expect(await driver.projectGithubBoundVisible()).toBe(false);
        expect(await driver.projectGithubGuidanceVisible()).toBe(true);
      });

      await test.step("the mirrored issue stays editable afterwards", async () => {
        const renamed = `${tag} renamed`;
        await serverPatchIssue(seed.workspaceSlug, projectId, issueId, { name: renamed }, session);
        expect((await serverIssueDetails(seed.workspaceSlug, projectId, issueId, session)).name).toBe(renamed);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-052"], "unbound projects point at General settings with no pickers on this tab"),
  { tag: specTags(["PSET-052"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 ghfree ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    try {
      expect((await serverRepositoryBinding(seed.workspaceSlug, projectId, session)).bound).toBe(false);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectGithubOpenTab(seed.workspaceSlug, projectId);
      expect(await driver.projectGithubNotAuthorized()).toBe(false);
      expect(await driver.projectGithubBoundVisible()).toBe(false);
      expect(await driver.projectGithubGuidanceVisible()).toBe(true);
      const text = await driver.projectGithubTabText();
      expect(text).not.toMatch(/branch/i);
      expect(text).not.toMatch(/direction/i);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-061"], "the github tab shows a skeleton while the binding fetch is unresolved"),
  { tag: specTags(["PSET-061"]) },
  async ({ driver, seed }) => {
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.projectGithubDelayBindingFetch(5_000);
    const opened = driver.projectGithubOpenTab(seed.workspaceSlug, seed.projectId);
    await expect.poll(() => driver.projectGithubLoaderVisible(), { timeout: 30_000 }).toBe(true);
    await opened;
    expect(await driver.projectGithubLoaderVisible()).toBe(false);
    expect(await driver.projectGithubBoundVisible()).toBe(true);
  }
);
