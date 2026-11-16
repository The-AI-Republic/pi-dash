// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-393): the five dedicated project feature
// tabs (saved views, timeboxing, triage inbox, sub-project grouping,
// free-form documents). Each tab carries one switch PATCHing its own
// project flag under promise toasts; flipping a flag hides or restores
// the area's sidebar link product-wide; non-admins get the
// not-authorized view with no switch at all. The tabs read the
// already-loaded project, so they settle on the switch directly with no
// loaders of their own (PSET-061 slice). The post-create feature step
// plus combined five-switch list (PSET-048) were intentionally hidden
// upstream (bd4f157e), so that row proves the absence: creation closes
// with a toast and no feature step ever shows.
// Rows: PSET-043–048, PSET-061.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  parityProjectIdentifier,
  projectByName,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCompleteOnboarding,
  serverCreateProjectWithFlags,
  serverPatchProject,
  serverProjectFeatureFlags,
  serverProvisionWorkspaceMember,
  setLastWorkspaceForProjects,
  signInSession,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const FEATURES = [
  {
    row: "PSET-043",
    tab: "views",
    area: "views",
    flag: "issueViewsView",
    title: "saved-views tab flips its flag and hides or restores shared views",
  },
  {
    row: "PSET-044",
    tab: "cycles",
    area: "cycles",
    flag: "cycleView",
    title: "timeboxing tab flips its flag and hides or restores planning periods",
  },
  {
    row: "PSET-045",
    tab: "intake",
    area: "intake",
    flag: "inboxView",
    title: "triage-inbox tab flips its flag and hides or restores the intake area",
  },
  {
    row: "PSET-046",
    tab: "modules",
    area: "modules",
    flag: "moduleView",
    title: "sub-project-grouping tab flips its flag and hides or restores modules",
  },
  {
    row: "PSET-047",
    tab: "pages",
    area: "pages",
    flag: "pageView",
    title: "free-form-documents tab flips its flag and hides or restores pages",
  },
] as const;

for (const feature of FEATURES) {
  test(specTitle([feature.row], feature.title), { tag: specTags([feature.row]) }, async ({ driver, seed }) => {
    const tag = `NF393 ${feature.tab} ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, {
      issue_views_view: true,
      cycle_view: true,
      inbox_view: true,
      module_view: true,
      page_view: true,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.projectFeaturesOpenTab(seed.workspaceSlug, projectId, feature.tab);

      await test.step("an enabled project shows the switch on", async () => {
        expect(await driver.projectFeaturesNotAuthorized()).toBe(false);
        expect(await driver.projectFeaturesSwitchDisabled()).toBe(false);
        expect(await driver.projectFeaturesSwitchOn()).toBe(true);
      });

      await test.step("flipping off clears the flag with a success notice and hides the area", async () => {
        await driver.projectFeaturesToggle();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Project feature updated");
        await expect
          .poll(async () => (await serverProjectFeatureFlags(seed.workspaceSlug, projectId, session))[feature.flag], {
            timeout: 20_000,
          })
          .toBe(false);
        expect(await driver.projectFeaturesSwitchOn()).toBe(false);
        expect(await driver.projectFeaturesNavLinkVisible(seed.workspaceSlug, projectId, feature.area)).toBe(false);
      });

      await test.step("flipping on restores the flag and the area link", async () => {
        await driver.projectFeaturesOpenTab(seed.workspaceSlug, projectId, feature.tab);
        await driver.projectFeaturesToggle();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Project feature updated");
        await expect
          .poll(async () => (await serverProjectFeatureFlags(seed.workspaceSlug, projectId, session))[feature.flag], {
            timeout: 20_000,
          })
          .toBe(true);
        expect(await driver.projectFeaturesSwitchOn()).toBe(true);
        expect(await driver.projectFeaturesNavLinkVisible(seed.workspaceSlug, projectId, feature.area)).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  });
}

test(
  specTitle(
    ["PSET-043", "PSET-044", "PSET-045", "PSET-046", "PSET-047"],
    "non-admin members get the denial view with no switch on any feature tab"
  ),
  { tag: specTags(["PSET-043", "PSET-044", "PSET-045", "PSET-046", "PSET-047"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 deny ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf393deny", session);
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
      for (const feature of FEATURES) {
        await test.step(`${feature.tab} tab denies members`, async () => {
          await driver.projectFeaturesOpenTab(seed.workspaceSlug, projectId, feature.tab);
          expect(await driver.projectFeaturesNotAuthorized()).toBe(true);
          expect(await driver.projectFeaturesSwitchOn()).toBe(false);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-048"], "creating a project closes the modal with no feature step"),
  { tag: specTags(["PSET-048"]) },
  async ({ driver }) => {
    // The post-create feature step was intentionally hidden upstream
    // (bd4f157e: MVP starts work-items-only), and the combined
    // five-switch list served only that step — so a UI create closes
    // with a toast and no feature step ever shows. Settings offers the
    // five dedicated tabs (PSET-043–047) instead.
    const owner = await signUpAuthedSession("nf393featlist");
    const suffix = uniqueSuffixForProjects();
    const wsName = `NF393 features ${suffix}`;
    const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `nf393feat-${suffix}` });
    await drainWorkspaceSeed(owner, ws.slug, wsName);
    await markOnboardedForProjects(owner);
    await setLastWorkspaceForProjects(owner, ws.id);

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));

    await test.step("a UI create closes the modal with a success notice and no feature step", async () => {
      await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
      await driver.clickEmptyStateCreate();
      await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(true);
      await driver.fillCreateProjectName("Feature Step Probe");
      await expect.poll(() => driver.createProjectShortCodeValue()).not.toBe("");
      await driver.submitCreateProject();
      await expect.poll(() => projectByName(owner, ws.slug, "Feature Step Probe")).not.toBeUndefined();
      await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(false);
      await expect.poll(() => driver.isToastVisible("Project created successfully")).toBe(true);
      expect(await driver.isCreateFeatureStepVisible()).toBe(false);
    });
  }
);

test(
  specTitle(["PSET-061"], "feature tabs settle on their switches with no loaders of their own"),
  { tag: specTags(["PSET-061"]) },
  async ({ driver, seed }) => {
    const tag = `NF393 noload ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N393"),
      {},
      session
    );
    await serverPatchProject(seed.workspaceSlug, projectId, session, {
      issue_views_view: true,
      cycle_view: true,
      inbox_view: true,
      module_view: true,
      page_view: true,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      for (const feature of FEATURES) {
        await test.step(`${feature.tab} tab shows its switch immediately`, async () => {
          await driver.projectFeaturesOpenTab(seed.workspaceSlug, projectId, feature.tab);
          expect(await driver.projectFeaturesNotAuthorized()).toBe(false);
          expect(await driver.projectFeaturesSwitchDisabled()).toBe(false);
          expect(await driver.projectFeaturesSwitchOn()).toBe(true);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
