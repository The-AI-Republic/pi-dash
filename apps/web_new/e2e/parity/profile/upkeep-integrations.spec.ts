// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-428): the profile upkeep tab (PROF-024) and
// integrations tab (PROF-025). Rows PROF-024, PROF-025.
//
// Observed behavior notes (the inventory rows carry them at update time):
// the seeded stack schedules nothing, so upkeep shows its empty
// explanation until a job is enabled instance-wide; the master pause
// flips instantly, disables the job switches while paused, and never
// touches individual jobs; both switches roll back with an error toast
// when the write fails; the GitHub App panel is per admin workspace with
// a picker, a missing-config warning, and a disabled Install on the
// seeded stack (the host-side install start is genuinely unobservable
// here — 409 — as is the live refresh check, which burns ~120s in
// network timeouts before its 502, so the suite pins the refresh-failure
// toast against the exact observed payload); install callbacks toast and
// scrub their markers while keeping other query params; non-admins see
// the no-admin view.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  ROLE,
  seatFreshMember,
  serverAutoPmGet,
  serverAutoPmSetJobRaw,
  serverDeleteWorkspace,
  serverGithubAppInstallStartRaw,
  serverGithubAppRefreshRaw,
  serverGithubAppStatus,
  serverLoopJobSetEnabled,
  serverSeedGithubAppInstallation,
  serverUnseedGithubAppInstallation,
  signInAuthedSession,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** The seeded builtin upkeep job (enabled=False until an operator flips it). */
const JOB_SLUG = "auto-close-merged";
const JOB_NAME = "Close issues when their PR merges";

/** Seat a fresh workspace member, onboarded and ready to browse settings. */
async function freshSettingsUser(
  admin: AuthedSession,
  seed: ParitySeedFacts,
  prefix: string
): Promise<{ user: AuthedSession; cookies: ReturnType<typeof browserSessionCookies> }> {
  const user = await seatFreshMember(admin, seed.workspaceSlug, ROLE.MEMBER, prefix);
  await markOnboardedForProjects(user);
  return { user, cookies: browserSessionCookies(user) };
}

async function toastPoll(driver: ParityDriver): Promise<{ title: string; message: string } | null> {
  return driver.settingsProfileLastToast();
}

test(
  specTitle(["PROF-024"], "empty instance explains instead of showing switches"),
  { tag: specTags(["PROF-024"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    // Self-healing: a crashed run may have left the job enabled.
    await serverLoopJobSetEnabled(JOB_SLUG, false);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof24e");

    const server = await serverAutoPmGet(user.cookie);
    expect(server.enabled).toBe(true);
    expect(server.jobs).toEqual([]);

    await driver.settingsProfileOpenTab("auto-project-management", cookies);
    const facts = await driver.settingsProfileUpkeepFacts();
    expect(facts.emptyExplanation).toBe("Nothing is scheduled on this instance yet.");
    expect(facts.masterPaused).toBeNull();
    expect(facts.jobs).toEqual([]);
  }
);

test(
  specTitle(["PROF-024"], "master pause and per-job toggles flip instantly with rollback on failure"),
  { tag: specTags(["PROF-024"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof24t");
    await serverLoopJobSetEnabled(JOB_SLUG, true);
    try {
      await test.step("scheduled job renders enabled beside an unpaused master", async () => {
        await driver.settingsProfileOpenTab("auto-project-management", cookies);
        const facts = await driver.settingsProfileUpkeepFacts();
        expect(facts.emptyExplanation).toBeNull();
        expect(facts.masterPaused).toBe(false);
        expect(facts.jobs).toHaveLength(1);
        expect(facts.jobs[0]).toMatchObject({
          name: JOB_NAME,
          interval: "daily",
          enabled: true,
          toggleDisabled: false,
        });
        expect(facts.jobs[0]?.description).toContain("marks an issue Done");
      });

      await test.step("pausing flips instantly, disables the job, and keeps the job on", async () => {
        await driver.settingsProfileUpkeepDelayWrites(3000);
        try {
          const done = driver.settingsProfileUpkeepToggleMaster(true);
          await driver.page.waitForTimeout(500);
          // The write is still in flight, yet the switch already flipped.
          expect((await driver.settingsProfileUpkeepFacts()).masterPaused).toBe(true);
          await done;
        } finally {
          await driver.settingsProfileUpkeepClearRoutes();
        }
        const facts = await driver.settingsProfileUpkeepFacts();
        expect(facts.masterPaused).toBe(true);
        expect(facts.jobs[0]?.toggleDisabled).toBe(true);
        const server = await serverAutoPmGet(user.cookie);
        expect(server.enabled).toBe(false);
        expect(server.jobs[0]?.enabled).toBe(true);
      });

      await test.step("resuming re-enables the job switch", async () => {
        await driver.settingsProfileUpkeepToggleMaster(false);
        const facts = await driver.settingsProfileUpkeepFacts();
        expect(facts.masterPaused).toBe(false);
        expect(facts.jobs[0]?.toggleDisabled).toBe(false);
        expect((await serverAutoPmGet(user.cookie)).enabled).toBe(true);
      });

      await test.step("the job toggles on its own and round-trips", async () => {
        await driver.settingsProfileUpkeepToggleJob(JOB_NAME, false);
        expect((await driver.settingsProfileUpkeepFacts()).jobs[0]?.enabled).toBe(false);
        expect((await serverAutoPmGet(user.cookie)).jobs[0]?.enabled).toBe(false);
        await driver.settingsProfileUpkeepToggleJob(JOB_NAME, true);
        expect((await driver.settingsProfileUpkeepFacts()).jobs[0]?.enabled).toBe(true);
        expect((await serverAutoPmGet(user.cookie)).jobs[0]?.enabled).toBe(true);
      });

      await test.step("unknown job slugs refuse server-side", async () => {
        const raw = await serverAutoPmSetJobRaw(user.cookie, "nope", false);
        expect(raw.status).toBe(404);
      });

      await test.step("a failed master write rolls back with an error toast", async () => {
        await driver.settingsProfileUpkeepFailWrites();
        try {
          await driver.settingsProfileUpkeepToggleMaster(true);
          await expect
            .poll(() => toastPoll(driver), { timeout: 15_000 })
            .toEqual({
              title: "Something went wrong",
              message: "probe_boom",
            });
          // The rollback revalidates after toasting; poll until it lands.
          await expect
            .poll(async () => (await driver.settingsProfileUpkeepFacts()).masterPaused, { timeout: 15_000 })
            .toBe(false);
          expect((await serverAutoPmGet(user.cookie)).enabled).toBe(true);
        } finally {
          await driver.settingsProfileUpkeepClearRoutes();
        }
      });

      await test.step("a failed job write rolls back with an error toast", async () => {
        await driver.settingsProfileUpkeepFailWrites();
        try {
          await driver.settingsProfileUpkeepToggleJob(JOB_NAME, false);
          await expect
            .poll(() => toastPoll(driver), { timeout: 15_000 })
            .toEqual({
              title: "Something went wrong",
              message: "probe_boom",
            });
          // The rollback revalidates after toasting; poll until it lands.
          await expect
            .poll(async () => (await driver.settingsProfileUpkeepFacts()).jobs[0]?.enabled, { timeout: 15_000 })
            .toBe(true);
          expect((await serverAutoPmGet(user.cookie)).jobs[0]?.enabled).toBe(true);
        } finally {
          await driver.settingsProfileUpkeepClearRoutes();
        }
      });
    } finally {
      await driver.settingsProfileUpkeepClearRoutes().catch(() => undefined);
      await serverLoopJobSetEnabled(JOB_SLUG, false);
    }
  }
);

test(
  specTitle(["PROF-025"], "workspace picker shows per-workspace state; members see the no-admin view"),
  { tag: specTags(["PROF-025"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const stamp = Date.now().toString(36).slice(-6);
    const ws2 = await createWorkspaceForProjects(admin, { name: `Prof25 ${stamp}`, slug: `prof25-${stamp}` });
    try {
      const { user: member, cookies: memberCookies } = await freshSettingsUser(admin, seed, "parity-prof25m");

      await test.step("non-admins get the no-admin view with no workspaces", async () => {
        const server = await serverGithubAppStatus(member.cookie);
        expect(server.workspaces).toEqual([]);
        await driver.settingsProfileOpenTab("integrations", memberCookies);
        const facts = await driver.settingsProfileIntegrationsFacts();
        expect(facts.loading).toBe(false);
        expect(facts.noAdmin).toBe(true);
        expect(facts.workspaceOptions).toEqual([]);
        expect(facts.panelHeading).toBeNull();
      });

      await test.step("admins see the picker with a disabled install and no refresh", async () => {
        await driver.settingsProfileOpenTab("integrations", browserSessionCookies(admin));
        const facts = await driver.settingsProfileIntegrationsFacts();
        expect(facts.loading).toBe(false);
        expect(facts.noAdmin).toBe(false);
        expect(facts.configuredWarning).toBe("GitHub App configuration is missing on this Pi Dash instance.");
        const slugs = facts.workspaceOptions.map((option) => option.slug);
        expect(slugs).toContain(seed.workspaceSlug);
        expect(slugs).toContain(ws2.slug);
        expect(facts.installLabel).toBe("Install");
        expect(facts.installDisabled).toBe(true);
        expect(facts.refreshVisible).toBe(false);
        expect(facts.connected).toBe(false);
        expect(facts.unconnectedCopy).toContain("No GitHub App installation is connected");
        const server = await serverGithubAppStatus(admin.cookie);
        expect(server.configured).toBe(false);
        const serverSlugs = server.workspaces.map((row) => row.slug);
        expect(serverSlugs).toContain(seed.workspaceSlug);
        expect(serverSlugs).toContain(ws2.slug);
        expect(server.workspaces.every((row) => row.githubApp.connected === false)).toBe(true);
      });

      await test.step("picking the second workspace swaps the panel to it", async () => {
        await driver.settingsProfileIntegrationsSelect(ws2.slug);
        const facts = await driver.settingsProfileIntegrationsFacts();
        expect(facts.selectedSlug).toBe(ws2.slug);
        expect(facts.panelLead).toContain(`for Prof25 ${stamp}.`);
        expect(facts.connected).toBe(false);
      });

      await test.step("install start refuses while the instance config is missing", async () => {
        const raw = await serverGithubAppInstallStartRaw(admin.cookie, seed.workspaceSlug);
        expect(raw.status).toBe(409);
      });
    } finally {
      await serverDeleteWorkspace(ws2.slug, admin.cookie);
    }
  }
);

test(
  specTitle(["PROF-025"], "install callbacks toast and scrub their markers"),
  { tag: specTags(["PROF-025"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const cookies = browserSessionCookies(admin);

    await test.step("success toasts, selects the workspace, and scrubs the markers", async () => {
      await driver.settingsProfileOpenTabWithQuery(
        "integrations",
        `github_app=connected&workspace_slug=${seed.workspaceSlug}`,
        cookies
      );
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "GitHub connected",
          message: "The GitHub App installation was verified.",
        });
      expect(await driver.settingsProfileCurrentQuery()).toBe("");
      expect((await driver.settingsProfileIntegrationsFacts()).selectedSlug).toBe(seed.workspaceSlug);
    });

    await test.step("a known error code toasts its message and scrubs", async () => {
      await driver.settingsProfileOpenTabWithQuery("integrations", "github_app=error&error=actor_mismatch", cookies);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "GitHub connection failed",
          message: "This install session was started by a different Pi Dash user.",
        });
      expect(await driver.settingsProfileCurrentQuery()).toBe("");
    });

    await test.step("an unknown error code falls back and scrubs", async () => {
      await driver.settingsProfileOpenTabWithQuery("integrations", "github_app=error&error=zzz_unknown", cookies);
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "GitHub connection failed",
          message: "The GitHub App installation could not be completed.",
        });
      expect(await driver.settingsProfileCurrentQuery()).toBe("");
    });

    await test.step("unrelated query params survive the scrub", async () => {
      await driver.settingsProfileOpenTabWithQuery(
        "integrations",
        "github_app=error&error=login_required&foo=bar",
        cookies
      );
      await expect
        .poll(() => toastPoll(driver), { timeout: 15_000 })
        .toEqual({
          title: "GitHub connection failed",
          message: "Sign in to Pi Dash and start the GitHub App install again.",
        });
      expect(await driver.settingsProfileCurrentQuery()).toBe("foo=bar");
    });
  }
);

test(
  specTitle(["PROF-025"], "connected panel renders identifiers with a manual refresh"),
  { tag: specTags(["PROF-025"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user: member } = await freshSettingsUser(admin, seed, "parity-prof25r");
    await serverSeedGithubAppInstallation(seed.workspaceSlug, "parity-org");
    try {
      await test.step("the panel shows the connection identifiers", async () => {
        await driver.settingsProfileOpenTab("integrations", browserSessionCookies(admin));
        const facts = await driver.settingsProfileIntegrationsFacts();
        expect(facts.connected).toBe(true);
        expect(facts.panelHeading).toBe("GitHub App");
        expect(facts.panelLead).toBe(`Installed for parity-org on Parity Workspace.`);
        expect(facts.installLabel).toBe("Reconnect");
        expect(facts.installDisabled).toBe(true);
        expect(facts.refreshVisible).toBe(true);
        expect(facts.unconnectedCopy).toBeNull();
        expect(facts.suspendedBanner).toBeNull();
        expect(facts.lastCheckError).toBeNull();
        const fields = new Map(facts.statusFields.map((field) => [field.label, field.value]));
        expect(fields.get("Account")).toBe("parity-org");
        expect(fields.get("Account type")).toBe("Organization");
        expect(fields.get("Repository access")).toBe("Selected repositories");
        expect(fields.get("Repositories visible")).toBe("3");
        expect(fields.get("Verified")).not.toBe("Never");
        expect(fields.get("Last checked")).not.toBe("Never");
        const server = await serverGithubAppStatus(admin.cookie);
        const self = server.workspaces.find((row) => row.slug === seed.workspaceSlug)?.githubApp;
        expect(self?.connected).toBe(true);
        expect(self?.accountLogin).toBe("parity-org");
        expect(self?.repositoryCount).toBe(3);
      });

      await test.step("refresh failure toasts the server message and keeps the panel", async () => {
        await driver.settingsProfileIntegrationsFailRefresh();
        try {
          await driver.settingsProfileIntegrationsRefresh();
          await expect
            .poll(() => toastPoll(driver), { timeout: 15_000 })
            .toEqual({
              title: "Refresh failed",
              message: "GitHub App config missing: app_id, app_slug, private_key",
            });
        } finally {
          await driver.settingsProfileIntegrationsClearRoutes();
        }
        const facts = await driver.settingsProfileIntegrationsFacts();
        expect(facts.connected).toBe(true);
        expect(facts.refreshVisible).toBe(true);
        const self = (await serverGithubAppStatus(admin.cookie)).workspaces.find(
          (row) => row.slug === seed.workspaceSlug
        )?.githubApp;
        expect(self?.connected).toBe(true);
        expect(self?.lastCheckError).toBe("");
      });

      await test.step("refresh guards refuse without an installation or adminship", async () => {
        const stamp = Date.now().toString(36).slice(-6);
        const ws2 = await createWorkspaceForProjects(admin, { name: `Prof25r ${stamp}`, slug: `prof25r-${stamp}` });
        try {
          const missing = await serverGithubAppRefreshRaw(admin.cookie, ws2.slug);
          expect(missing.status).toBe(404);
        } finally {
          await serverDeleteWorkspace(ws2.slug, admin.cookie);
        }
        const forbidden = await serverGithubAppRefreshRaw(member.cookie, seed.workspaceSlug);
        expect(forbidden.status).toBe(403);
      });
    } finally {
      await driver.settingsProfileIntegrationsClearRoutes().catch(() => undefined);
      await serverUnseedGithubAppInstallation(seed.workspaceSlug);
    }
  }
);
