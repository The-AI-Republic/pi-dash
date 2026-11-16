// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-445): instance-admin loop (auto project
// management) jobs. Rows ADM-051 (jobs table), ADM-052 (inline enable
// toggle, optimistic plus rollback), ADM-053 (list empty state), ADM-054
// (list skeleton), ADM-055 (create modal), ADM-056 (edit modal, builtin
// slug locked), ADM-057 (modal validation plus error surfacing), ADM-058
// (detail stats plus targets table), ADM-059 (targets skip-reason
// filter), ADM-060 (delete with no confirmation) and ADM-061 (detail
// skeleton).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the loop page carries a titled header with a New-job
// action over a table of name (linking to the detail), slug, recurrence
// rule, minimum role (numeric roles shown as admin/member/guest labels),
// builtin yes/no and an inline enabled switch. The switch flips
// immediately and persists, rolling back with an error notice when the
// write fails. With no jobs an inline paragraph prompts creation; while
// loading, two row-height skeleton bars show. The create/edit modal
// carries slug, admin name, user-facing name and description, prompt,
// minimum role (default member), timezone (default UTC) and a recurrence
// rule (default daily), gating save on the slug, both names and the
// prompt being non-blank; a failed save toasts the server-provided
// detail, then error, then a generic fallback, and the modal body
// scrolls within a capped height. Edit pre-fills every field and locks
// the slug of builtin jobs. The detail page shows the job with four
// stat tiles (targets plus 24h completed/failed/skipped), a skip-reason
// filter that refetches the targets, and a targets table (workspace,
// user, next run, last-run status, last-run tokens, last skip reason)
// with a placeholder row when empty; a danger-styled delete action
// removes the job immediately with no confirmation, toasts, and returns
// to the list. While the job loads, skeleton bars show under a
// placeholder header.
import { test, expect } from "../fixtures";
import {
  serverAdminCreateLoopJob,
  serverAdminCreateLoopTarget,
  serverAdminDeleteLoopJob,
  serverAdminDeleteLoopJobsBySlugPrefix,
  serverAdminDeleteLoopTargetsByJobSlugPrefix,
  serverAdminGetLoopJob,
  serverAdminListLoopJobs,
  serverAdminListLoopTargets,
  serverAdminSession,
  uniqueSlug,
  type ServerAdminLoopJobInput,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Slug prefix for jobs this spec creates (cleanup deletes by prefix). */
const SPEC_SLUG_PREFIX = "adm445-";

/** Minimum-role labels as the list table renders them. */
const ROLE_LABELS: Record<number, string> = { 20: "Admin", 15: "Member", 5: "Guest" };

/** Skip-reason filter options as the detail page renders them ("" first for all). */
const SKIP_OPTIONS = [
  "",
  "user_disabled",
  "master_paused",
  "min_role",
  "llm_config_missing",
  "membership_gone",
  "turn_active",
  "dispatch_error",
];

/** Daily 03:00 UTC recurrence, the modal's default. */
const DAILY_RRULE = "FREQ=DAILY;BYHOUR=3;BYMINUTE=0";

/** Fresh create payload for `slug` (all fields explicit). */
function jobInput(slug: string, overrides: Partial<ServerAdminLoopJobInput> = {}): ServerAdminLoopJobInput {
  return {
    slug,
    name: `Probe job ${slug}`,
    publicName: `Probe public ${slug}`,
    publicDescription: `Probe description ${slug}`,
    prompt: `Probe prompt ${slug}`,
    minRole: 15,
    rrule: DAILY_RRULE,
    tzid: "UTC",
    ...overrides,
  };
}

/** Remove every fixture job and target this spec may have left behind. */
async function cleanupFixtures(): Promise<void> {
  await serverAdminDeleteLoopTargetsByJobSlugPrefix(SPEC_SLUG_PREFIX).catch(() => {});
  await serverAdminDeleteLoopJobsBySlugPrefix(SPEC_SLUG_PREFIX).catch(() => {});
}

/** Open the loop list signed in, waiting for the section to load. */
async function openLoopList(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/loop/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect
    .poll(
      async () => {
        const facts = await driver.adminLoopListFacts();
        return facts.tableVisible || facts.emptyVisible;
      },
      { timeout: 15_000 }
    )
    .toBe(true);
}

/** Open a job's detail signed in, waiting for the job to load. */
async function openLoopDetail(driver: ParityDriver, seed: ParitySeedFacts, jobId: string): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath(`/loop/${jobId}/`);
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminLoopDetailFacts()).editVisible, { timeout: 15_000 }).toBe(true);
}

/** Second seeded identity for a second target edge on the same job. */
function secondEmail(seed: ParitySeedFacts): string {
  const email = seed.botEmail ?? seed.mentionMember?.email ?? seed.guestEmail;
  expect(email).toBeDefined();
  return email as string;
}

test(
  specTitle(["ADM-051"], "list shows every job with columns matching the server"),
  { tag: specTags(["ADM-051"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug, { minRole: 20 }));
      expect(created.slug).toBe(slug);
      const server = await serverAdminListLoopJobs(session);
      expect(server.length).toBeGreaterThan(0);
      await openLoopList(driver, seed);
      const facts = await driver.adminLoopListFacts();
      expect(facts.titleVisible).toBe(true);
      expect(facts.descriptionVisible).toBe(true);
      expect(facts.createVisible).toBe(true);
      expect(facts.skeletonVisible).toBe(false);
      expect(facts.emptyVisible).toBe(false);
      expect(facts.tableVisible).toBe(true);
      expect(facts.headers).toEqual(["Name", "Slug", "Recurrence", "Min role", "Builtin", "Enabled"]);
      const rows = await driver.adminLoopRows();
      expect(rows.length).toBe(server.length);
      for (const job of server) {
        const row = rows.find((candidate) => candidate.slug === job.slug);
        expect(row).toBeDefined();
        expect(row?.name).toBe(job.name);
        expect(row?.href).toContain(`/loop/${job.id}`);
        expect(row?.rrule).toBe(job.rrule);
        expect(row?.minRoleLabel).toBe(ROLE_LABELS[job.minRole] ?? String(job.minRole));
        expect(row?.builtin).toBe(job.isBuiltin);
        expect(row?.enabled).toBe(job.enabled);
      }
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-052"], "toggle persists the new state with an enable-flag write"),
  { tag: specTags(["ADM-052"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug, { enabled: false }));
      expect(created.enabled).toBe(false);
      await openLoopList(driver, seed);
      await driver.adminStartLoopJobUpdateSpy();
      await driver.adminLoopSetEnabled(slug, true);
      await expect
        .poll(async () => (await serverAdminGetLoopJob(session, created.id))?.enabled, {
          timeout: 15_000,
        })
        .toBe(true);
      await expect
        .poll(async () => (await driver.adminLoopRows()).find((row) => row.slug === slug)?.enabled, {
          timeout: 15_000,
        })
        .toBe(true);
      const bodies = await driver.adminLoopJobUpdateBodies();
      expect(bodies.length).toBeGreaterThan(0);
      expect(bodies[bodies.length - 1]).toEqual({ enabled: true });
      expect(await driver.adminLastToast()).toBe(null);
    } finally {
      await driver.adminClearLoopJobUpdateFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-052"], "toggle flips before the write lands"),
  { tag: specTags(["ADM-052"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug, { enabled: false }));
      await openLoopList(driver, seed);
      await driver.adminDelayLoopJobUpdate(3000);
      await driver.adminLoopSetEnabled(slug, true);
      // The switch flips while the write is still in flight (optimistic).
      await expect
        .poll(async () => (await driver.adminLoopRows()).find((row) => row.slug === slug)?.enabled, {
          timeout: 15_000,
        })
        .toBe(true);
      // The delayed write then lands and the server agrees.
      await expect
        .poll(async () => (await serverAdminGetLoopJob(session, created.id))?.enabled, {
          timeout: 15_000,
        })
        .toBe(true);
    } finally {
      await driver.adminClearLoopJobUpdateFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-052"], "failed toggle rolls back with an error notice"),
  { tag: specTags(["ADM-052"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug, { enabled: false }));
      await openLoopList(driver, seed);
      await driver.adminStartLoopJobUpdateSpy();
      await driver.adminFailLoopJobUpdate();
      await driver.adminLoopSetEnabled(slug, true);
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Update failed");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Could not change the job state.");
      await expect
        .poll(async () => (await driver.adminLoopRows()).find((row) => row.slug === slug)?.enabled, {
          timeout: 15_000,
        })
        .toBe(false);
      const bodies = await driver.adminLoopJobUpdateBodies();
      expect(bodies.length).toBeGreaterThan(0);
      expect((await serverAdminGetLoopJob(session, created.id))?.enabled).toBe(false);
    } finally {
      await driver.adminClearLoopJobUpdateFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-053"], "empty list prompts creation with no table"),
  { tag: specTags(["ADM-053"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const server = await serverAdminListLoopJobs(session);
    expect(server.length).toBeGreaterThan(0);
    try {
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      await driver.adminStubEmptyLoopJobs();
      await driver.adminOpenPath("/loop/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminLoopListFacts()).emptyVisible, { timeout: 15_000 }).toBe(true);
      const facts = await driver.adminLoopListFacts();
      expect(facts.titleVisible).toBe(true);
      expect(facts.createVisible).toBe(true);
      expect(facts.emptyText).toBe("No jobs yet. Create one to get started.");
      expect(facts.tableVisible).toBe(false);
      expect(facts.skeletonVisible).toBe(false);
      expect(await driver.adminLoopRows()).toEqual([]);
      // The server still holds the jobs; only the read was stubbed.
      expect((await serverAdminListLoopJobs(session)).length).toBe(server.length);
      await driver.adminClearLoopJobsFetchFault();
      // The stubbed read is cached, so re-enter the page for a fresh read.
      await driver.adminOpenPath("/loop/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminLoopListFacts()).tableVisible, { timeout: 15_000 }).toBe(true);
    } finally {
      await driver.adminClearLoopJobsFetchFault().catch(() => {});
    }
  }
);

test(
  specTitle(["ADM-054"], "row-height skeleton bars show while the list loads"),
  { tag: specTags(["ADM-054"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    // Held until the spec lifts the fault, so a cold route compile on the
    // new console cannot outlast the skeleton window.
    await driver.adminDelayLoopJobsFetch(driver.target === "web_new" ? 120_000 : 3000);
    try {
      await driver.adminOpenPath("/loop/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminLoopListFacts()).skeletonVisible, { timeout: 15_000 }).toBe(true);
      const during = await driver.adminLoopListFacts();
      expect(during.skeletonRows).toBe(2);
      expect(during.skeletonHeights).toEqual(["48px", "48px"]);
      expect(during.tableVisible).toBe(false);
      expect(during.emptyVisible).toBe(false);
    } finally {
      await driver.adminClearLoopJobsFetchFault();
    }
    await expect
      .poll(
        async () => {
          const facts = await driver.adminLoopListFacts();
          return facts.tableVisible || facts.emptyVisible;
        },
        { timeout: 15_000 }
      )
      .toBe(true);
    const after = await driver.adminLoopListFacts();
    expect(after.skeletonVisible).toBe(false);
  }
);

test(
  specTitle(["ADM-055"], "create modal opens with member, UTC and daily defaults"),
  { tag: specTags(["ADM-055"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openLoopList(driver, seed);
    await driver.adminLoopOpenCreate();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
    const modal = await driver.adminLoopModalFacts();
    expect(modal.title).toBe("New job");
    expect(modal.fields.slug).toBe("");
    expect(modal.fields.name).toBe("");
    expect(modal.fields.publicName).toBe("");
    expect(modal.fields.publicDescription).toBe("");
    expect(modal.fields.prompt).toBe("");
    expect(modal.fields.minRole).toBe(15);
    expect(modal.fields.tzid).toBe("UTC");
    expect(modal.fields.rrule).toBe(DAILY_RRULE);
    expect(modal.slugDisabled).toBe(false);
    expect(modal.saveDisabled).toBe(true);
    expect(modal.cancelVisible).toBe(true);
    await driver.adminLoopModalCancel();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
  }
);

test(
  specTitle(["ADM-055"], "create saves and the list revalidates with the new job"),
  { tag: specTags(["ADM-055"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await openLoopList(driver, seed);
      await driver.adminStartLoopJobCreateSpy();
      await driver.adminLoopOpenCreate();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
      await driver.adminLoopModalFill({
        slug,
        name: `Probe job ${slug}`,
        publicName: `Probe public ${slug}`,
        publicDescription: `Probe description ${slug}`,
        prompt: `Probe prompt ${slug}`,
        minRole: 20,
        tzid: "UTC",
        rrule: DAILY_RRULE,
      });
      expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(false);
      await driver.adminLoopModalSave();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
      await expect
        .poll(async () => (await driver.adminLoopRows()).some((row) => row.slug === slug), { timeout: 15_000 })
        .toBe(true);
      const bodies = await driver.adminLoopJobCreateBodies();
      expect(bodies.length).toBeGreaterThan(0);
      const last = bodies[bodies.length - 1] as Record<string, unknown>;
      expect(last["slug"]).toBe(slug);
      expect(last["name"]).toBe(`Probe job ${slug}`);
      expect(last["public_name"]).toBe(`Probe public ${slug}`);
      expect(last["public_description"]).toBe(`Probe description ${slug}`);
      expect(last["prompt"]).toBe(`Probe prompt ${slug}`);
      expect(last["min_role"]).toBe(20);
      expect(last["rrule"]).toBe(DAILY_RRULE);
      expect(last["tzid"]).toBe("UTC");
      const server = await serverAdminListLoopJobs(session);
      const stored = server.find((job) => job.slug === slug);
      expect(stored).toBeDefined();
      expect(stored?.minRole).toBe(20);
      expect(stored?.isBuiltin).toBe(false);
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-056"], "edit modal prefills and saves with list revalidation"),
  { tag: specTags(["ADM-056"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await openLoopDetail(driver, seed, created.id);
      await driver.adminLoopOpenEdit();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
      const modal = await driver.adminLoopModalFacts();
      expect(modal.title).toBe("Edit job");
      expect(modal.fields.slug).toBe(slug);
      expect(modal.fields.name).toBe(created.name);
      expect(modal.fields.publicName).toBe(created.publicName);
      expect(modal.fields.publicDescription).toBe(created.publicDescription);
      expect(modal.fields.prompt).toBe(created.prompt);
      expect(modal.fields.minRole).toBe(created.minRole);
      expect(modal.fields.tzid).toBe(created.tzid);
      expect(modal.fields.rrule).toBe(created.rrule);
      expect(modal.slugDisabled).toBe(false);
      expect(modal.saveDisabled).toBe(false);
      const renamed = `Renamed probe ${slug}`;
      await driver.adminLoopModalFill({ name: renamed });
      await driver.adminLoopModalSave();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
      await expect.poll(async () => (await driver.adminLoopDetailFacts()).title, { timeout: 15_000 }).toBe(renamed);
      expect((await serverAdminGetLoopJob(session, created.id))?.name).toBe(renamed);
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-056"], "builtin job slug is locked in the edit modal"),
  { tag: specTags(["ADM-056"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const server = await serverAdminListLoopJobs(session);
    const builtin = server.find((job) => job.isBuiltin);
    expect(builtin).toBeDefined();
    if (builtin === undefined) return;
    const before = await serverAdminGetLoopJob(session, builtin.id);
    await openLoopDetail(driver, seed, builtin.id);
    await driver.adminLoopOpenEdit();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
    const modal = await driver.adminLoopModalFacts();
    expect(modal.title).toBe("Edit job");
    expect(modal.fields.slug).toBe(builtin.slug);
    expect(modal.slugDisabled).toBe(true);
    expect(modal.fields.name).toBe(builtin.name);
    await driver.adminLoopModalCancel();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
    const after = await serverAdminGetLoopJob(session, builtin.id);
    expect(after?.slug).toBe(before?.slug);
    expect(after?.name).toBe(before?.name);
  }
);

test(
  specTitle(["ADM-057"], "save stays disabled until slug, names and prompt are non-blank"),
  { tag: specTags(["ADM-057"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openLoopList(driver, seed);
    await driver.adminLoopOpenCreate();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(true);
    await driver.adminLoopModalFill({ slug: "probe-gate" });
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(true);
    await driver.adminLoopModalFill({ name: "Probe gate" });
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(true);
    await driver.adminLoopModalFill({ publicName: "Probe public gate" });
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(true);
    await driver.adminLoopModalFill({ prompt: "Probe prompt gate" });
    // Description, role, timezone and recurrence keep their defaults and gate nothing.
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(false);
    await driver.adminLoopModalFill({ prompt: "   " });
    expect((await driver.adminLoopModalFacts()).saveDisabled).toBe(true);
    const modal = await driver.adminLoopModalFacts();
    expect(modal.bodyScrollable).toBe(true);
    expect(modal.bodyMaxHeight).not.toBe(null);
    await driver.adminLoopModalCancel();
    await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(false);
  }
);

test(
  specTitle(["ADM-057"], "failed save surfaces the server error and keeps the modal open"),
  { tag: specTags(["ADM-057"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await openLoopList(driver, seed);
      await driver.adminLoopOpenCreate();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
      await driver.adminLoopModalFill({
        slug,
        name: `Probe job ${slug}`,
        publicName: `Probe public ${slug}`,
        prompt: `Probe prompt ${slug}`,
      });
      await driver.adminFailLoopJobCreate();
      await driver.adminLoopModalSave();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Save failed");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("parity-probe-failure");
      expect((await driver.adminLoopModalFacts()).open).toBe(true);
      await driver.adminClearLoopJobCreateFault();
      await driver.adminLoopModalCancel();
      const server = await serverAdminListLoopJobs(session);
      expect(server.some((job) => job.slug === slug)).toBe(false);
    } finally {
      await driver.adminClearLoopJobCreateFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-057"], "invalid recurrence surfaces the server detail on save"),
  { tag: specTags(["ADM-057"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await openLoopList(driver, seed);
      await driver.adminLoopOpenCreate();
      await expect.poll(async () => (await driver.adminLoopModalFacts()).open, { timeout: 15_000 }).toBe(true);
      await driver.adminLoopModalFill({
        slug,
        name: `Probe job ${slug}`,
        publicName: `Probe public ${slug}`,
        prompt: `Probe prompt ${slug}`,
        rrule: "bogus-rrule",
      });
      await driver.adminLoopModalSave();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Save failed");
      const toast = await driver.adminLastToast();
      // The server-provided detail wins over the generic fallback.
      expect(toast?.message).not.toBe("Check the fields and try again.");
      expect(toast?.message?.length ?? 0).toBeGreaterThan(0);
      expect((await driver.adminLoopModalFacts()).open).toBe(true);
      await driver.adminLoopModalCancel();
      const server = await serverAdminListLoopJobs(session);
      expect(server.some((job) => job.slug === slug)).toBe(false);
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-058"], "detail shows stat tiles with an empty-targets placeholder"),
  { tag: specTags(["ADM-058"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      const stored = await serverAdminGetLoopJob(session, created.id);
      expect(stored?.stats).not.toBe(null);
      await openLoopDetail(driver, seed, created.id);
      const facts = await driver.adminLoopDetailFacts();
      expect(facts.title).toBe(created.name);
      expect(facts.description).toBe(created.publicDescription);
      expect(facts.skeletonVisible).toBe(false);
      expect(facts.editVisible).toBe(true);
      expect(facts.deleteVisible).toBe(true);
      expect(facts.stats).toEqual({ targets: 0, completed: 0, failed: 0, skipped: 0 });
      expect(stored?.stats).toEqual({ targetCount: 0, completed: 0, failed: 0, skipped: 0 });
      expect(facts.targetsTableVisible).toBe(true);
      expect(facts.targetsHeaders).toEqual(["Workspace", "User", "Next run", "Last run", "Tokens", "Last skip"]);
      expect(facts.targetsEmptyVisible).toBe(true);
      expect(facts.targetsEmptyText).toBe("No targets yet.");
      expect(await driver.adminLoopDetailRows()).toEqual([]);
      const targets = await serverAdminListLoopTargets(session, created.id);
      expect(targets.page).toBe(1);
      expect(targets.results).toEqual([]);
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-058"], "detail renders populated targets with matching stats"),
  { tag: specTags(["ADM-058"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    const other = secondEmail(seed);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await serverAdminCreateLoopTarget({
        jobSlug: slug,
        workspaceSlug: seed.workspaceSlug,
        userEmail: seed.email,
        skipReason: "user_disabled",
      });
      await serverAdminCreateLoopTarget({
        jobSlug: slug,
        workspaceSlug: seed.workspaceSlug,
        userEmail: other,
      });
      await openLoopDetail(driver, seed, created.id);
      const facts = await driver.adminLoopDetailFacts();
      expect(facts.stats).toEqual({ targets: 2, completed: 0, failed: 0, skipped: 1 });
      expect(facts.targetsEmptyVisible).toBe(false);
      const rows = await driver.adminLoopDetailRows();
      expect(rows.length).toBe(2);
      const skipped = rows.find((row) => row.user === seed.email);
      expect(skipped).toBeDefined();
      expect(skipped?.workspace).toBe(seed.workspaceSlug);
      expect(skipped?.nextRun).not.toBe("—");
      expect(skipped?.lastRun).toBe("—");
      expect(skipped?.tokens).toBe("—");
      expect(skipped?.lastSkip).toBe("user_disabled");
      const fresh = rows.find((row) => row.user === other);
      expect(fresh).toBeDefined();
      expect(fresh?.workspace).toBe(seed.workspaceSlug);
      expect(fresh?.lastSkip).toBe("—");
      const stored = await serverAdminGetLoopJob(session, created.id);
      expect(stored?.stats).toEqual({ targetCount: 2, completed: 0, failed: 0, skipped: 1 });
      const targets = await serverAdminListLoopTargets(session, created.id);
      expect(targets.results.length).toBe(2);
      expect(new Set(targets.results.map((row) => row.userEmail))).toEqual(new Set([seed.email, other]));
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-059"], "skip-reason filter refetches the targets filtered"),
  { tag: specTags(["ADM-059"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    const other = secondEmail(seed);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await serverAdminCreateLoopTarget({
        jobSlug: slug,
        workspaceSlug: seed.workspaceSlug,
        userEmail: seed.email,
        skipReason: "user_disabled",
      });
      await serverAdminCreateLoopTarget({
        jobSlug: slug,
        workspaceSlug: seed.workspaceSlug,
        userEmail: other,
        skipReason: "min_role",
      });
      await openLoopDetail(driver, seed, created.id);
      const facts = await driver.adminLoopDetailFacts();
      expect(facts.skipOptions).toEqual(SKIP_OPTIONS);
      expect(facts.skipSelected).toBe("");
      await driver.adminStartLoopTargetsSpy();
      await driver.adminLoopSetSkipFilter("user_disabled");
      await expect
        .poll(async () => (await driver.adminLoopTargetsUrls()).length, { timeout: 15_000 })
        .toBeGreaterThan(0);
      const filtered = await driver.adminLoopTargetsUrls();
      expect(filtered[filtered.length - 1]).toContain("skip_reason=user_disabled");
      await expect.poll(async () => (await driver.adminLoopDetailRows()).length, { timeout: 15_000 }).toBe(1);
      const rows = await driver.adminLoopDetailRows();
      expect(rows[0]?.user).toBe(seed.email);
      expect((await driver.adminLoopDetailFacts()).skipSelected).toBe("user_disabled");
      const server = await serverAdminListLoopTargets(session, created.id, { skipReason: "user_disabled" });
      expect(server.results.length).toBe(1);
      expect(server.results[0]?.userEmail).toBe(seed.email);
      await driver.adminLoopSetSkipFilter("");
      await expect.poll(async () => (await driver.adminLoopDetailRows()).length, { timeout: 15_000 }).toBe(2);
      expect((await driver.adminLoopDetailFacts()).skipSelected).toBe("");
      // Reset serves the cached unfiltered read, so no refetch is required.
      const reset = await driver.adminLoopTargetsUrls();
      expect(reset.length).toBe(filtered.length);
    } finally {
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-060"], "delete removes the job at once and returns to the list"),
  { tag: specTags(["ADM-060"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await openLoopDetail(driver, seed, created.id);
      await driver.adminStartLoopJobDeleteSpy();
      await driver.adminLoopDelete();
      // No confirmation dialog appears; the delete lands immediately.
      expect((await driver.adminLoopModalFacts()).open).toBe(false);
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Deleted");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Loop job deleted.");
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 15_000 }).toBe("/god-mode/loop/");
      await expect.poll(async () => (await driver.adminLoopListFacts()).tableVisible, { timeout: 15_000 }).toBe(true);
      expect(await driver.adminLoopJobDeleteCount()).toBe(1);
      expect(await serverAdminGetLoopJob(session, created.id)).toBe(null);
      const rows = await driver.adminLoopRows();
      expect(rows.some((row) => row.slug === slug)).toBe(false);
    } finally {
      await driver.adminClearLoopJobDeleteFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-060"], "failed delete surfaces an error and stays on the detail"),
  { tag: specTags(["ADM-060"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await openLoopDetail(driver, seed, created.id);
      await driver.adminFailLoopJobDelete();
      await driver.adminLoopDelete();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Delete failed");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Could not delete the job.");
      const facts = await driver.adminLoopDetailFacts();
      expect(facts.title).toBe(created.name);
      expect(facts.editVisible).toBe(true);
      expect(await driver.adminCurrentPath()).toContain(created.id);
      expect((await serverAdminGetLoopJob(session, created.id))?.slug).toBe(slug);
      await driver.adminClearLoopJobDeleteFault();
      await serverAdminDeleteLoopJob(session, created.id);
      expect(await serverAdminGetLoopJob(session, created.id)).toBe(null);
    } finally {
      await driver.adminClearLoopJobDeleteFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);

test(
  specTitle(["ADM-061"], "skeleton bars show under a placeholder header until the job loads"),
  { tag: specTags(["ADM-061"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      const created = await serverAdminCreateLoopJob(session, jobInput(slug));
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      await driver.adminDelayLoopJobFetch(driver.target === "web_new" ? 120_000 : 3000);
      try {
        await driver.adminOpenPath(`/loop/${created.id}/`);
        await driver.adminWaitForRootBranch("dashboard");
        await expect
          .poll(async () => (await driver.adminLoopDetailFacts()).skeletonVisible, { timeout: 15_000 })
          .toBe(true);
        const during = await driver.adminLoopDetailFacts();
        expect(during.title).toBe("Loop job");
        expect(during.skeletonHeights).toEqual(["48px", "120px"]);
        expect(during.editVisible).toBe(false);
        expect(during.deleteVisible).toBe(false);
      } finally {
        await driver.adminClearLoopJobFetchFault();
      }
      await expect.poll(async () => (await driver.adminLoopDetailFacts()).editVisible, { timeout: 90_000 }).toBe(true);
      const after = await driver.adminLoopDetailFacts();
      expect(after.title).toBe(created.name);
      expect(after.skeletonVisible).toBe(false);
    } finally {
      await driver.adminClearLoopJobFetchFault().catch(() => {});
      await cleanupFixtures();
    }
  }
);
