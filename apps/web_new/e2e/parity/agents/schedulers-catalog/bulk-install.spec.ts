// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): installing one definition onto many
// projects at once from the catalog, with a shared schedule. The picker
// lists joined projects with search and select-all while locking
// already-installed ones; an empty selection is refused; per-target results
// partition into success/failure notices; failures stay selected for retry;
// install counts refresh; each target needs project standing. Row: AGT-006.
// Note: the partial failure is a concurrent install racing the picker's
// detection snapshot — another tab installs target B after the dialog opens,
// so its POST answers 400 while A succeeds. The retry uninstalls B first.
import { test, expect } from "../../fixtures";
import {
  ROLE,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverCreateBindingStatus,
  serverDeleteBinding,
  serverProjectMembers,
  serverSchedulers,
  signInSessionRetry,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { outsiderUser, schedulerHarness } from "../support";

const ROWS = ["AGT-006"];

test(
  specTitle(ROWS, "bulk install partitions per-target results; failures stay selected for retry"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    test.setTimeout(600_000);
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt6"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const slug = `agt6-bulk-${workspaceSlug}`;

    const { projectA, projectB } = await test.step("owner prepares projects and definitions", async () => {
      const projectA = await ensureProject(
        workspaceSlug,
        ownerSession,
        `Bulk Alpha ${tag}`,
        parityProjectIdentifier("AG6A")
      );
      const projectB = await ensureProject(
        workspaceSlug,
        ownerSession,
        `Bulk Beta ${tag}`,
        parityProjectIdentifier("AG6B")
      );
      const projectC = await ensureProject(
        workspaceSlug,
        ownerSession,
        `Bulk Gamma ${tag}`,
        parityProjectIdentifier("AG6C")
      );
      for (const project of [projectA, projectB, projectC]) {
        const membership = (await serverProjectMembers(workspaceSlug, project.id, ownerSession)).find(
          (row) => row.userId === owner.userId
        );
        expect(membership?.role).toBe(ROLE.ADMIN);
      }
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug,
        name: "AGT6 Bulk Definition",
        description: "Installed onto many projects at once.",
        prompt: "Run everywhere.",
        is_enabled: true,
      });
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt6-off-${workspaceSlug}`,
        name: "AGT6 Disabled Definition",
        prompt: "Cannot be installed.",
        is_enabled: false,
      });
      // Pre-installed target: the picker must show it locked.
      await serverCreateBinding(workspaceSlug, projectC.id, ownerSession, {
        scheduler: definition.id,
        project: projectC.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return { projectA, projectB };
    });

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("disabled definitions offer no install action", async () => {
      expect(await driver.schedulerRowActions(`agt6-off-${workspaceSlug}`)).toEqual(["Edit", "Delete"]);
      await driver.schedulerOpenInstall(slug);
    });

    await test.step("the picker lists joined projects with search and select-all", async () => {
      const projectAName = `Bulk Alpha ${tag}`;
      const projectBName = `Bulk Beta ${tag}`;
      const projectCName = `Bulk Gamma ${tag}`;
      const options = await driver.schedulerInstallPickerOptions();
      const byName = new Map(options.map((option) => [option.name, option]));
      expect(byName.get(projectAName)).toMatchObject({ checked: false, locked: false });
      expect(byName.get(projectBName)).toMatchObject({ checked: false, locked: false });
      expect(byName.get(projectCName)).toMatchObject({ checked: true, locked: true });
      // Workspace creation also provisions sample projects outside this
      // scenario; they are eligible like any joined project, so select-all
      // assertions discover them instead of hardcoding a count.
      const extraEligible = options.filter(
        (option) => option.name !== projectAName && option.name !== projectBName && option.name !== projectCName
      );
      expect(extraEligible.length).toBeGreaterThan(0);
      expect(extraEligible.every((option) => !option.locked)).toBe(true);
      await driver.schedulerInstallSearch("Beta");
      await expect
        .poll(() => driver.schedulerInstallPickerOptions(), { timeout: 30_000 })
        .toEqual([expect.objectContaining({ name: projectBName })]);
      await driver.schedulerInstallSearch("");
      await driver.schedulerInstallToggleProject(projectAName);
      await driver.schedulerInstallToggleProject(projectBName);
      await expect
        .poll(() => driver.schedulerInstallSelectedSummary(), { timeout: 30_000 })
        .toBe("2 projects selected");
      // Select-all adds every remaining eligible project, then clears all.
      await driver.schedulerInstallToggleSelectAll();
      await expect
        .poll(
          async () =>
            (await driver.schedulerInstallPickerOptions())
              .filter((option) => option.checked && !option.locked)
              .map((option) => option.name)
              .sort(),
          { timeout: 30_000 }
        )
        .toEqual([projectAName, projectBName, ...extraEligible.map((option) => option.name)].sort());
      await driver.schedulerInstallToggleSelectAll();
      await expect.poll(() => driver.schedulerInstallSelectedSummary(), { timeout: 30_000 }).toBe("Select projects");
    });

    await test.step("an empty selection is refused", async () => {
      expect(await driver.schedulerInstallSelectedSummary()).toBe("Select projects");
      await driver.schedulerInstallSubmit();
      await expect
        .poll(() => driver.schedulerVisibleToasts(), { timeout: 60_000 })
        .toEqual([{ title: "Select a project", message: expect.stringContaining("at least one") }]);
      expect(await driver.schedulerInstallOpen()).toBe(true);
      expect(await serverBindings(workspaceSlug, projectA.id, ownerSession)).toEqual([]);
    });

    await test.step("per-target results partition into success and failure", async () => {
      await driver.schedulerInstallToggleProject(`Bulk Alpha ${tag}`);
      await driver.schedulerInstallToggleProject(`Bulk Beta ${tag}`);
      await expect
        .poll(() => driver.schedulerInstallSelectedSummary(), { timeout: 30_000 })
        .toBe("2 projects selected");
      // A concurrent tab installs B after the picker's detection snapshot, so
      // its POST answers 400 (duplicate) while A succeeds.
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      expect(definition).toBeDefined();
      await serverCreateBinding(workspaceSlug, projectB.id, ownerSession, {
        scheduler: definition?.id ?? "",
        project: projectB.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      await driver.schedulerInstallSubmit();
      await expect
        .poll(() => driver.schedulerVisibleToasts(), { timeout: 60_000 })
        .toEqual(
          expect.arrayContaining([
            { title: "Installed on 1 project", message: expect.stringContaining("configured schedule") },
            {
              title: "1 project failed",
              message: expect.stringContaining(`Bulk Beta ${tag}`),
            },
          ])
        );
      expect(await driver.schedulerInstallOpen()).toBe(true);
      // Failures stay selected for retry; successes lock as installed.
      await expect.poll(() => driver.schedulerInstallSelectedSummary(), { timeout: 30_000 }).toBe("1 project selected");
      const options = await driver.schedulerInstallPickerOptions();
      const byName = new Map(options.map((option) => [option.name, option]));
      expect(byName.get(`Bulk Alpha ${tag}`)).toMatchObject({ checked: true, locked: true });
      expect(byName.get(`Bulk Beta ${tag}`)).toMatchObject({ checked: true, locked: false });
      expect(await serverBindings(workspaceSlug, projectA.id, ownerSession)).toHaveLength(1);
    });

    await test.step("a target without project standing is refused and stores nothing", async () => {
      const outsider = await outsiderUser();
      const outsiderSession = await signInSessionRetry(outsider.email, outsider.password);
      const definition = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      const refused = await serverCreateBindingStatus(workspaceSlug, projectA.id, outsiderSession, {
        scheduler: definition?.id ?? "",
        project: projectA.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      expect(refused.status).toBe(403);
      expect(await serverBindings(workspaceSlug, projectA.id, ownerSession)).toHaveLength(1);
    });

    await test.step("the retry succeeds and install counts refresh", async () => {
      const raced = await serverBindings(workspaceSlug, projectB.id, ownerSession);
      expect(raced).toHaveLength(1);
      await serverDeleteBinding(workspaceSlug, projectB.id, raced[0]?.id ?? "", ownerSession);
      await driver.schedulerInstallSubmit();
      await expect
        .poll(() => driver.schedulerVisibleToasts(), { timeout: 60_000 })
        .toEqual(
          expect.arrayContaining([
            { title: "Installed on 1 project", message: expect.stringContaining("configured schedule") },
          ])
        );
      await expect.poll(() => driver.schedulerInstallOpen(), { timeout: 60_000 }).toBe(false);
      await expect
        .poll(async () => (await driver.schedulerCatalogRows()).find((row) => row.handle === slug)?.installs ?? "", {
          timeout: 60_000,
        })
        .toContain("3");
      const bindingsB = await serverBindings(workspaceSlug, projectB.id, ownerSession);
      expect(bindingsB).toHaveLength(1);
      expect(bindingsB[0]).toMatchObject({
        scheduler_slug: slug,
        rrule: "FREQ=DAILY",
        outcome_mode: "create_issue",
        enabled: true,
      });
      expect(bindingsB[0]?.tzid.length).toBeGreaterThan(0);
      expect(Number.isNaN(Date.parse(bindingsB[0]?.dtstart ?? ""))).toBe(false);
      const stored = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      expect(stored?.active_binding_count).toBe(3);
    });
  }
);
