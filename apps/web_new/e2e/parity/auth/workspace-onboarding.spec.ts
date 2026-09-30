// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Parity scenarios: auth workspace onboarding finish + tour (NEWFRONT-111,
// rows AUTH-034..043). Each scenario drives the old app (the oracle) through
// the parity driver and proves both what the user sees and what the server
// stored. Sign-in is another area's row, so onboarding users enter the app
// pre-authenticated via injected session cookies; every scenario mints its
// own fresh, not-yet-onboarded user and uses unique slugs so parallel runs
// and reruns never collide. Green on apps/web first; apps/web_new implements
// the same behavior later (its driver stubs throw until it lands).
import { test, expect } from "../fixtures";
import {
  browserCookies,
  createJoinRequest,
  createWorkspaceViaApi,
  getOnboardingProgress,
  markOnboarded,
  seedAtWorkspaceStep,
  sendWorkspaceInvites,
  setLastWorkspace,
  signUpFreshUser,
  slugAvailable,
  uniqueEmail,
  uniqueSlug,
  userInvitationWorkspaceSlugs,
  userJoinRequestAdminEmails,
  userWorkspaceRole,
  userWorkspaceSlugs,
  workspacePendingInviteEmails,
} from "../helpers/api";
import { setWorkspaceCreationDisabled } from "../helpers/stack";
import { specTags, specTitle } from "../helpers/tags";

test.describe("auth workspace onboarding finish + tour", () => {
  test(
    specTitle(["AUTH-034"], "workspace create-or-join step: sub-view defaulting and creation"),
    { tag: specTags(["AUTH-034"]) },
    async ({ driver }) => {
      await test.step("a fresh user with no invites or requests defaults to the create view", async () => {
        const user = await signUpFreshUser();
        await seedAtWorkspaceStep(user);
        await driver.openAuthenticated("/onboarding", browserCookies(user));
        await driver.awaitWorkspaceStep();
        expect(await driver.visibleWorkspaceView()).toBe("create");
      });

      await test.step("a pending join request forces the holding view", async () => {
        const user = await signUpFreshUser();
        await seedAtWorkspaceStep(user);
        await createJoinRequest(user, uniqueEmail("parity-anyadmin"));
        await driver.openAuthenticated("/onboarding", browserCookies(user));
        await driver.awaitWorkspaceStep();
        expect(await driver.visibleWorkspaceView()).toBe("pending");
      });

      await test.step("an outstanding invite defaults to the invites view", async () => {
        const admin = await signUpFreshUser("parity-adm");
        const adminSlug = uniqueSlug("adm");
        const ws = await createWorkspaceViaApi(admin, { name: `Admin ${adminSlug}`, slug: adminSlug });
        const invitee = await signUpFreshUser();
        await sendWorkspaceInvites(admin, ws.slug, [{ email: invitee.email, role: 15 }]);
        await seedAtWorkspaceStep(invitee);
        expect(await userInvitationWorkspaceSlugs(invitee)).toContain(ws.slug);
        await driver.openAuthenticated("/onboarding", browserCookies(invitee));
        await driver.awaitWorkspaceStep();
        expect(await driver.visibleWorkspaceView()).toBe("invites");
      });

      await test.step("creating with a valid name, slug and team size makes a workspace and advances", async () => {
        const user = await signUpFreshUser();
        await seedAtWorkspaceStep(user);
        await driver.openAuthenticated("/onboarding", browserCookies(user));
        await driver.awaitWorkspaceStep();
        const slug = uniqueSlug("newf");
        await driver.fillWorkspaceName(`Team ${slug}`);
        await driver.fillWorkspaceSlug(slug);
        await driver.selectTeamSizePill("2-10");
        expect(await driver.isCreateWorkspaceSubmitDisabled()).toBe(false);
        await driver.submitCreateWorkspace();
        await driver.awaitInviteMembersStep();
        // Server state: the workspace exists, the user owns it, and the
        // create flag is stamped.
        expect(await userWorkspaceSlugs(user)).toContain(slug);
        expect(await userWorkspaceRole(user, slug)).toBe(20);
        expect((await getOnboardingProgress(user)).onboarding_step.workspace_create).toBe(true);
      });
    }
  );

  test(
    specTitle(["AUTH-035"], "workspace URL availability and reserved-name enforcement"),
    { tag: specTags(["AUTH-035"]) },
    async ({ driver }) => {
      const user = await signUpFreshUser();
      await seedAtWorkspaceStep(user);
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.awaitWorkspaceStep();

      await test.step("the slug derives from the name but stays editable", async () => {
        await driver.fillWorkspaceName("My Cool Team");
        expect(await driver.workspaceSlugValue()).toBe("my-cool-team");
        await driver.fillWorkspaceSlug("custom-slug");
        expect(await driver.workspaceSlugValue()).toBe("custom-slug");
      });

      await test.step("a malformed slug blocks submit with an inline message", async () => {
        await driver.fillWorkspaceName("Bad One");
        await driver.fillWorkspaceSlug("bad@slug");
        await driver.selectTeamSizePill("2-10");
        expect(await driver.workspaceSlugErrorText()).toContain("URLs can contain only");
        expect(await driver.isCreateWorkspaceSubmitDisabled()).toBe(true);
      });

      await test.step("a reserved slug is rejected inline and creates nothing", async () => {
        await driver.fillWorkspaceName("Api Team");
        await driver.fillWorkspaceSlug("api");
        await driver.selectTeamSizePill("2-10");
        await driver.submitCreateWorkspace();
        expect(await driver.workspaceSlugErrorText()).toContain("already taken");
        expect(await driver.visibleWorkspaceView()).toBe("create");
        expect(await userWorkspaceSlugs(user)).not.toContain("api");
      });

      await test.step("a taken slug is rejected inline and creates nothing", async () => {
        const taken = uniqueSlug("taken");
        const occupier = await signUpFreshUser("parity-occ");
        await createWorkspaceViaApi(occupier, { name: `Occ ${taken}`, slug: taken });
        expect(await slugAvailable(user, taken)).toBe(false);
        await driver.fillWorkspaceName("Taken Team");
        await driver.fillWorkspaceSlug(taken);
        await driver.selectTeamSizePill("2-10");
        await driver.submitCreateWorkspace();
        expect(await driver.workspaceSlugErrorText()).toContain("already taken");
        expect(await userWorkspaceSlugs(user)).not.toContain(taken);
      });
    }
  );

  test(
    specTitle(["AUTH-036"], "workspace access request plus pending view"),
    { tag: specTags(["AUTH-036"]) },
    async ({ driver }) => {
      const user = await signUpFreshUser();
      await seedAtWorkspaceStep(user);
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.awaitWorkspaceStep();
      expect(await driver.visibleWorkspaceView()).toBe("create");

      await driver.gotoJoinByEmailFromCreate();
      expect(await driver.visibleWorkspaceView()).toBe("join_by_email");

      // Any admin email is accepted and parks the user on the holding view;
      // the outcome is identical whether or not the address exists, so it
      // reveals nothing about the address.
      const adminEmail = uniqueEmail("parity-wsadmin");
      await driver.fillWorkspaceAdminEmail(adminEmail);
      await driver.submitJoinRequest();
      expect(await driver.pendingApprovalNamesEmail(adminEmail)).toBe(true);
      expect(await userJoinRequestAdminEmails(user)).toContain(adminEmail);

      // The holding view offers creating a workspace instead.
      await driver.createInsteadFromPending();
      expect(await driver.visibleWorkspaceView()).toBe("create");
    }
  );

  test(specTitle(["AUTH-037"], "invite-members step"), { tag: specTags(["AUTH-037"]) }, async ({ driver }) => {
    const user = await signUpFreshUser();
    await seedAtWorkspaceStep(user);
    await driver.openAuthenticated("/onboarding", browserCookies(user));
    await driver.awaitWorkspaceStep();
    const slug = uniqueSlug("inv");
    await driver.fillWorkspaceName(`Inv ${slug}`);
    await driver.fillWorkspaceSlug(slug);
    await driver.selectTeamSizePill("2-10");
    await driver.submitCreateWorkspace();
    await driver.awaitInviteMembersStep();

    await test.step("starts with blank rows and grows on demand", async () => {
      const initial = await driver.inviteRowCount();
      expect(initial).toBeGreaterThanOrEqual(1);
      await driver.clickAddAnotherInvite();
      expect(await driver.inviteRowCount()).toBe(initial + 1);
    });

    await test.step("send stays disabled until at least one valid address", async () => {
      expect(await driver.isSendInvitesDisabled()).toBe(true);
      await driver.fillInviteRow(0, uniqueEmail("parity-invitee"));
      expect(await driver.isSendInvitesDisabled()).toBe(false);
    });

    await test.step("sending drops empty rows and records exactly the filled invitation", async () => {
      const invitee = uniqueEmail("parity-invitee");
      await driver.fillInviteRow(0, invitee);
      await driver.sendInvites();
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toContain(slug);
      const pending = await workspacePendingInviteEmails(user, slug);
      expect(pending).toContain(invitee);
      expect(pending).toHaveLength(1);
    });
  });

  test(specTitle(["AUTH-038"], "solo-workspace shortcut"), { tag: specTags(["AUTH-038"]) }, async ({ driver }) => {
    const user = await signUpFreshUser();
    await seedAtWorkspaceStep(user);
    await driver.openAuthenticated("/onboarding", browserCookies(user));
    await driver.awaitWorkspaceStep();
    const slug = uniqueSlug("solo");
    await driver.fillWorkspaceName(`Solo ${slug}`);
    await driver.fillWorkspaceSlug(slug);
    await driver.selectTeamSizePill("Just myself");
    await driver.submitCreateWorkspace();

    // Choosing the solo size finishes onboarding immediately, with no
    // invite step, landing in the new workspace.
    await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toContain(slug);
    expect(await driver.isInviteMembersStepVisible()).toBe(false);
    const progress = await getOnboardingProgress(user);
    expect(progress.is_onboarded).toBe(true);
    expect(await userWorkspaceRole(user, slug)).toBe(20);
    expect(await workspacePendingInviteEmails(user, slug)).toHaveLength(0);
  });

  test(
    specTitle(["AUTH-039"], "onboarding completion and landing"),
    { tag: specTags(["AUTH-039"]) },
    async ({ driver }) => {
      const user = await signUpFreshUser();
      await seedAtWorkspaceStep(user);
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.awaitWorkspaceStep();
      const slug = uniqueSlug("fin");
      await driver.fillWorkspaceName(`Fin ${slug}`);
      await driver.fillWorkspaceSlug(slug);
      await driver.selectTeamSizePill("2-10");
      await driver.submitCreateWorkspace();
      await driver.awaitInviteMembersStep();
      await driver.deferInvites();

      // Deferring finishes onboarding: land in the workspace, all progress
      // flags plus the onboarded marker stamped.
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toContain(slug);
      const progress = await getOnboardingProgress(user);
      expect(progress.is_onboarded).toBe(true);
      expect(progress.onboarding_step.profile_complete).toBe(true);
      expect(progress.onboarding_step.workspace_create).toBe(true);
      expect(progress.onboarding_step.workspace_invite).toBe(true);
      expect(await userWorkspaceSlugs(user)).toContain(slug);
    }
  );

  test(
    specTitle(["AUTH-040"], "standalone workspace creation"),
    { tag: specTags(["AUTH-040"]) },
    async ({ driver }) => {
      const user = await signUpFreshUser();
      await markOnboarded(user);
      await driver.openAuthenticated("/create-workspace", browserCookies(user));
      expect(await driver.hasVisibleText("Create your workspace")).toBe(true);

      const slug = uniqueSlug("std");
      await driver.fillWorkspaceName(`Std ${slug}`);
      await driver.fillWorkspaceSlug(slug);
      await driver.selectTeamSizeDropdown("2-10");
      await driver.submitCreateWorkspace();

      // Success enters the new workspace directly, with no invite step.
      await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toContain(slug);
      expect(await driver.isInviteMembersStepVisible()).toBe(false);
      expect(await userWorkspaceRole(user, slug)).toBe(20);
    }
  );

  test(
    specTitle(["AUTH-041"], "workspace-creation-disabled states"),
    { tag: specTags(["AUTH-041"]) },
    async ({ driver }) => {
      const standaloneUser = await signUpFreshUser();
      await markOnboarded(standaloneUser);
      const onboardingUser = await signUpFreshUser();
      await seedAtWorkspaceStep(onboardingUser);

      // The flag is instance-wide and cache-backed, so flip it, assert, and
      // always restore it, keeping the disabled window as small as possible.
      try {
        setWorkspaceCreationDisabled(true);

        await test.step("standalone route shows the admin-only screen", async () => {
          await driver.openAuthenticated("/create-workspace", browserCookies(standaloneUser));
          await expect.poll(() => driver.isStandaloneCreationDisabledVisible(), { timeout: 30_000 }).toBe(true);
          expect(await driver.isRequestInstanceAdminLinkVisible()).toBe(true);
        });

        await test.step("in-onboarding create view shows the restricted notice", async () => {
          await driver.openAuthenticated("/onboarding", browserCookies(onboardingUser));
          await expect.poll(() => driver.isInOnboardingCreationDisabledNoticeVisible(), { timeout: 30_000 }).toBe(true);
        });
      } finally {
        setWorkspaceCreationDisabled(false);
      }
    }
  );

  test(specTitle(["AUTH-042"], "first-run product tour"), { tag: specTags(["AUTH-042"]) }, async ({ driver }) => {
    // AUTH-042 is a cloud-edition row. The workspace-home tour gate itself
    // is edition-independent (it keys off the per-user tour-completed flag),
    // so the tour and its completion are exercised web-observably here on the
    // OSS stack; any cloud-overlay-only tour content is not present in this
    // checkout and is recorded in the hand-off.
    const user = await signUpFreshUser();
    const slug = uniqueSlug("tour");
    const ws = await createWorkspaceViaApi(user, { name: `Tour ${slug}`, slug });
    await markOnboarded(user);
    await setLastWorkspace(user, ws.id);

    await driver.openAuthenticated(`/${slug}`, browserCookies(user));
    await expect.poll(() => driver.isTourWelcomeVisible(), { timeout: 60_000 }).toBe(true);
    expect((await getOnboardingProgress(user)).is_tour_completed).toBe(false);

    await driver.declineTour();
    // Dismissing marks the tour completed for this user (once per user).
    await expect
      .poll(async () => (await getOnboardingProgress(user)).is_tour_completed, { timeout: 30_000 })
      .toBe(true);
  });

  test(specTitle(["AUTH-043"], "in-flow back navigation"), { tag: specTags(["AUTH-043"]) }, async ({ driver }) => {
    await test.step("back on the workspace step returns to the previous step, same URL", async () => {
      const user = await signUpFreshUser();
      await seedAtWorkspaceStep(user);
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.awaitWorkspaceStep();
      expect(await driver.currentPath()).toContain("/onboarding");
      expect(await driver.isOnboardingBackVisible()).toBe(true);

      await driver.clickOnboardingBack();
      // Self-managed sequence: the workspace step steps back to profile.
      await expect.poll(() => driver.hasVisibleText("Create your profile."), { timeout: 15_000 }).toBe(true);
      expect(await driver.visibleWorkspaceView()).toBe("none");
      // Single-URL flow: the browser location does not change with the step.
      expect(await driver.currentPath()).toContain("/onboarding");
    });

    await test.step("the first step hides the back control", async () => {
      const user = await signUpFreshUser();
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await expect.poll(() => driver.isOnboardingBackVisible(), { timeout: 15_000 }).toBe(false);
    });

    await test.step("the last step (invite members) hides the back control", async () => {
      const user = await signUpFreshUser();
      await seedAtWorkspaceStep(user);
      await driver.openAuthenticated("/onboarding", browserCookies(user));
      await driver.awaitWorkspaceStep();
      const slug = uniqueSlug("back");
      await driver.fillWorkspaceName(`Back ${slug}`);
      await driver.fillWorkspaceSlug(slug);
      await driver.selectTeamSizePill("2-10");
      await driver.submitCreateWorkspace();
      await driver.awaitInviteMembersStep();
      expect(await driver.isOnboardingBackVisible()).toBe(false);
    });
  });
});
