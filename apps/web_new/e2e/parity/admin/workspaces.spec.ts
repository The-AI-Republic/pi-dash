// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-444): instance-admin workspace list and
// create form. Rows ADM-037 (restrict-workspace-creation toggle),
// ADM-038 (workspace list with count and per-row detail), ADM-039 (row
// deep-links into the main web app in a new tab), ADM-040 (load-more
// pagination), ADM-041 (list loading, mutation and empty states),
// ADM-042 (no delete or edit surfaced), ADM-043 (create form with
// derived slug and org size), ADM-044 (slug-availability gate) and
// ADM-045 (create plus redirect with server-assigned owner).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the restrict toggle saves its flag as a single-key write
// with a progress notice settling into success or failure, and the
// switch locks while any write is in flight. The list section shows a
// count badge with a spinner beside it during pagination or refresh; the
// whole section is a four-row skeleton on first load, and an empty
// instance shows a zero count with no rows and no illustration. Each row
// is an external link into the main web app at the workspace slug,
// showing a logo image or letter fallback, name and bracketed slug, owner
// email when present, and project/member counts when present; no row
// exposes delete or edit, and a helper line states workspaces cannot be
// deleted here. Load-more appends the next ten-workspace page with a busy
// disabled state. The create form prefixes the slug with the web base
// URL, derives it from the name (lowercased, spaces to hyphens) while
// allowing independent edits forced lowercase, requires an org size, and
// gates submit on validity with a busy label. Submit checks slug
// availability first (taken or restricted slugs show an inline URL-taken
// error with no create); success toasts and returns to the list with the
// owner assigned server-side, while slug-check or create failures toast
// an error and stay.
import { test, expect } from "../fixtures";
import {
  serverAdminCreateWorkspace,
  serverAdminDeleteWorkspacesBySlugPrefix,
  serverAdminGetConfigurations,
  serverAdminListWorkspaces,
  serverAdminSession,
  serverAdminSlugCheck,
  serverAdminUpdateConfigurations,
  uniqueSlug,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Configuration keys this spec snapshots and restores. */
const WORKSPACE_KEYS = ["DISABLE_WORKSPACE_CREATION"] as const;

/** Slug prefix for workspaces this spec creates (cleanup deletes by prefix). */
const SPEC_SLUG_PREFIX = "adm444-";

/** Snapshot the workspace config rows this spec may rewrite, for teardown restore. */
async function snapshotWorkspaceConfig(session: string): Promise<Record<string, string>> {
  const all = await serverAdminGetConfigurations(session);
  return Object.fromEntries(WORKSPACE_KEYS.map((key) => [key, all[key] ?? ""]));
}

/** Restore workspace config rows and prove the restore landed (keeps retries convergent). */
async function restoreWorkspaceConfig(session: string, snap: Record<string, string>): Promise<void> {
  await serverAdminUpdateConfigurations(session, snap);
  const after = await serverAdminGetConfigurations(session);
  for (const key of WORKSPACE_KEYS) {
    expect(after[key]).toBe(snap[key]);
  }
}

/** Open the workspace list signed in, waiting for the section to load. */
async function openWorkspaceList(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/workspace/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminWorkspaceListFacts()).toggleVisible, { timeout: 15_000 }).toBe(true);
  await expect.poll(async () => (await driver.adminWorkspaceListFacts()).count, { timeout: 15_000 }).not.toBe(null);
}

/** Open the workspace create form signed in, waiting for it to render. */
async function openWorkspaceCreate(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/workspace/create/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect
    .poll(
      async () => {
        try {
          await driver.adminWorkspaceCreateFacts();
          return true;
        } catch {
          return false;
        }
      },
      { timeout: 15_000 }
    )
    .toBe(true);
}

test(
  specTitle(["ADM-037"], "restrict toggle persists with saving then success"),
  { tag: specTags(["ADM-037"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotWorkspaceConfig(session);
    try {
      await openWorkspaceList(driver, seed);
      await driver.adminStartConfigPatchSpy();
      const initial = await driver.adminWorkspaceListFacts();
      expect(initial.toggleVisible).toBe(true);
      const target = !initial.toggleOn;
      await driver.adminWorkspaceSetCreationRestricted(target);
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Configuration saved successfully");
      const bodies = await driver.adminConfigPatchBodies();
      expect(bodies.length).toBeGreaterThan(0);
      const last = bodies[bodies.length - 1] as Record<string, string>;
      expect(last["DISABLE_WORKSPACE_CREATION"]).toBe(target ? "1" : "0");
      const after = await serverAdminGetConfigurations(session);
      expect(after["DISABLE_WORKSPACE_CREATION"]).toBe(target ? "1" : "0");
      const facts = await driver.adminWorkspaceListFacts();
      expect(facts.toggleOn).toBe(target);
    } finally {
      await restoreWorkspaceConfig(session, before);
    }
  }
);

test(
  specTitle(["ADM-037"], "in-flight toggle save disables the switch"),
  { tag: specTags(["ADM-037"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotWorkspaceConfig(session);
    try {
      await openWorkspaceList(driver, seed);
      await driver.adminStartConfigPatchSpy();
      const initial = await driver.adminWorkspaceListFacts();
      const target = !initial.toggleOn;
      await driver.adminDelayConfigPatch(3000);
      await driver.adminWorkspaceSetCreationRestricted(target);
      await expect
        .poll(async () => (await driver.adminWorkspaceListFacts()).toggleDisabled, { timeout: 15_000 })
        .toBe(true);
      await driver.adminClearConfigPatchFault();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success");
      const facts = await driver.adminWorkspaceListFacts();
      expect(facts.toggleDisabled).toBe(false);
      expect(facts.toggleOn).toBe(target);
      const after = await serverAdminGetConfigurations(session);
      expect(after["DISABLE_WORKSPACE_CREATION"]).toBe(target ? "1" : "0");
    } finally {
      await driver.adminClearConfigPatchFault().catch(() => {});
      await restoreWorkspaceConfig(session, before);
    }
  }
);

test(
  specTitle(["ADM-037"], "failed toggle save reports failure and keeps the stored value"),
  { tag: specTags(["ADM-037"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotWorkspaceConfig(session);
    try {
      await openWorkspaceList(driver, seed);
      await driver.adminStartConfigPatchSpy();
      const initial = await driver.adminWorkspaceListFacts();
      const target = !initial.toggleOn;
      await driver.adminFailConfigPatch();
      await driver.adminWorkspaceSetCreationRestricted(target);
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Error");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Failed to save configuration");
      const after = await serverAdminGetConfigurations(session);
      expect(after["DISABLE_WORKSPACE_CREATION"]).toBe(before["DISABLE_WORKSPACE_CREATION"]);
      const facts = await driver.adminWorkspaceListFacts();
      expect(facts.toggleOn).toBe(initial.toggleOn);
      expect(facts.toggleDisabled).toBe(false);
    } finally {
      await driver.adminClearConfigPatchFault().catch(() => {});
      await restoreWorkspaceConfig(session, before);
    }
  }
);

test(
  specTitle(["ADM-037"], "toggle row is a skeleton while config loads"),
  { tag: specTags(["ADM-037"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminDelayConfigFetch(3000);
    await driver.adminOpenPath("/workspace/");
    await driver.adminWaitForRootBranch("dashboard");
    await expect
      .poll(async () => (await driver.adminWorkspaceListFacts()).toggleSkeletonVisible, {
        timeout: 15_000,
      })
      .toBe(true);
    const during = await driver.adminWorkspaceListFacts();
    expect(during.toggleVisible).toBe(false);
    await driver.adminClearConfigFetchFault();
    await expect
      .poll(async () => (await driver.adminWorkspaceListFacts()).toggleVisible, { timeout: 15_000 })
      .toBe(true);
    const after = await driver.adminWorkspaceListFacts();
    expect(after.toggleSkeletonVisible).toBe(false);
  }
);

test(
  specTitle(["ADM-038"], "list shows the count with per-row detail matching the server"),
  { tag: specTags(["ADM-038"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const server = await serverAdminListWorkspaces(session);
    expect(server.results.length).toBeGreaterThan(0);
    await openWorkspaceList(driver, seed);
    const facts = await driver.adminWorkspaceListFacts();
    expect(facts.count).toBe(server.results.length);
    const rows = await driver.adminWorkspaceRows();
    expect(rows.length).toBe(server.results.length);
    for (const slated of server.results) {
      const row = rows.find((candidate) => candidate.slug === slated.slug);
      expect(row).toBeDefined();
      expect(row?.name).toBe(slated.name);
      expect(row?.ownerEmail).toBe(slated.ownerEmail);
      expect(row?.totalProjects).toBe(slated.totalProjects);
      expect(row?.totalMembers).toBe(slated.totalMembers);
      if (slated.logoUrl !== null) {
        expect(row?.logoImageVisible).toBe(true);
        expect(row?.logoLetter).toBe(null);
      } else {
        expect(row?.logoImageVisible).toBe(false);
        expect(row?.logoLetter).toBe(slated.name.slice(0, 1));
      }
    }
  }
);

test(
  specTitle(["ADM-039"], "rows deep-link into the main web app in a new tab"),
  { tag: specTags(["ADM-039"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const server = await serverAdminListWorkspaces(session);
    expect(server.results.length).toBeGreaterThan(0);
    await openWorkspaceList(driver, seed);
    const rows = await driver.adminWorkspaceRows();
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.href).toContain(`/${encodeURIComponent(row.slug)}`);
      expect(row.href).not.toContain("/god-mode/");
      expect(row.target).toBe("_blank");
      expect(row.rel).toContain("noreferrer");
    }
    const path = await driver.adminCurrentPath();
    expect(path).toContain("/workspace/");
  }
);

test(
  specTitle(["ADM-040"], "load-more appends the next page with a busy disabled state"),
  { tag: specTags(["ADM-040"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const prefix = `${SPEC_SLUG_PREFIX}${Date.now().toString(36)}-`;
    const created: string[] = [];
    try {
      for (let i = 0; i < 11; i++) {
        const slug = `${prefix}${i}`;
        await serverAdminCreateWorkspace(session, {
          name: `Load More ${i}`,
          slug,
          organizationSize: "2-10",
        });
        created.push(slug);
      }
      await openWorkspaceList(driver, seed);
      await driver.adminStartWorkspacesFetchSpy();
      let facts = await driver.adminWorkspaceListFacts();
      expect(facts.loadMoreVisible).toBe(true);
      expect(facts.loadMoreDisabled).toBe(false);
      const before = await driver.adminWorkspaceRows();
      expect(before.length).toBe(10);
      await driver.adminDelayWorkspacesFetch(2000);
      await driver.adminWorkspaceLoadMore();
      await expect
        .poll(async () => (await driver.adminWorkspaceListFacts()).loadMoreDisabled, {
          timeout: 15_000,
        })
        .toBe(true);
      await driver.adminClearWorkspacesFetchFault();
      await expect
        .poll(async () => (await driver.adminWorkspaceRows()).length, { timeout: 15_000 })
        .toBeGreaterThan(10);
      facts = await driver.adminWorkspaceListFacts();
      expect(facts.count).toBeGreaterThan(10);
      const urls = await driver.adminWorkspacesFetchUrls();
      expect(urls.some((url) => url.includes("cursor="))).toBe(true);
      for (const slug of created) {
        const server = await serverAdminListWorkspaces(session);
        expect(server.results.some((row) => row.slug === slug) || facts.count! > 10).toBe(true);
        break;
      }
    } finally {
      await driver.adminClearWorkspacesFetchFault().catch(() => {});
      await serverAdminDeleteWorkspacesBySlugPrefix(prefix);
    }
  }
);

test(
  specTitle(["ADM-041"], "whole section is a skeleton on first load"),
  { tag: specTags(["ADM-041"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminDelayWorkspacesFetch(3000);
    await driver.adminOpenPath("/workspace/");
    await driver.adminWaitForRootBranch("dashboard");
    await expect
      .poll(async () => (await driver.adminWorkspaceListFacts()).listSkeletonVisible, {
        timeout: 15_000,
      })
      .toBe(true);
    const during = await driver.adminWorkspaceListFacts();
    expect(during.listSkeletonRows).toBe(4);
    expect(during.count).toBe(null);
    await driver.adminClearWorkspacesFetchFault();
    await expect.poll(async () => (await driver.adminWorkspaceListFacts()).count, { timeout: 15_000 }).not.toBe(null);
    const after = await driver.adminWorkspaceListFacts();
    expect(after.listSkeletonVisible).toBe(false);
  }
);

test(
  specTitle(["ADM-041"], "spinner beside the count during pagination"),
  { tag: specTags(["ADM-041"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const prefix = `${SPEC_SLUG_PREFIX}spin${Date.now().toString(36)}-`;
    try {
      for (let i = 0; i < 11; i++) {
        await serverAdminCreateWorkspace(session, {
          name: `Spin ${i}`,
          slug: `${prefix}${i}`,
          organizationSize: "2-10",
        });
      }
      await openWorkspaceList(driver, seed);
      const facts = await driver.adminWorkspaceListFacts();
      expect(facts.loadMoreVisible).toBe(true);
      expect(facts.countSpinnerVisible).toBe(false);
      await driver.adminDelayWorkspacesFetch(3000);
      await driver.adminWorkspaceLoadMore();
      await expect
        .poll(async () => (await driver.adminWorkspaceListFacts()).countSpinnerVisible, {
          timeout: 15_000,
        })
        .toBe(true);
      await driver.adminClearWorkspacesFetchFault();
      await expect
        .poll(async () => (await driver.adminWorkspaceRows()).length, { timeout: 15_000 })
        .toBeGreaterThan(10);
      const settled = await driver.adminWorkspaceListFacts();
      expect(settled.countSpinnerVisible).toBe(false);
    } finally {
      await driver.adminClearWorkspacesFetchFault().catch(() => {});
      await serverAdminDeleteWorkspacesBySlugPrefix(prefix);
    }
  }
);

test(
  specTitle(["ADM-041"], "empty instance shows a zero count with no rows"),
  { tag: specTags(["ADM-041"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await driver.adminEnsureSignedIn(seed.email, seed.password);
    await driver.adminStubEmptyWorkspaces();
    await driver.adminOpenPath("/workspace/");
    await driver.adminWaitForRootBranch("dashboard");
    await expect.poll(async () => (await driver.adminWorkspaceListFacts()).count, { timeout: 15_000 }).toBe(0);
    const facts = await driver.adminWorkspaceListFacts();
    expect(facts.listSkeletonVisible).toBe(false);
    expect(facts.loadMoreVisible).toBe(false);
    expect(facts.helperVisible).toBe(true);
    const rows = await driver.adminWorkspaceRows();
    expect(rows.length).toBe(0);
    await driver.adminClearWorkspacesFetchFault();
  }
);

test(
  specTitle(["ADM-042"], "helper states the limits and rows expose only the open link"),
  { tag: specTags(["ADM-042"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openWorkspaceList(driver, seed);
    const facts = await driver.adminWorkspaceListFacts();
    expect(facts.helperVisible).toBe(true);
    expect(facts.createHref).toContain("/workspace/create");
    const rows = await driver.adminWorkspaceRows();
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.exposesMutation).toBe(false);
      expect(row.target).toBe("_blank");
    }
  }
);

test(
  specTitle(["ADM-043"], "slug derives from the name and stays independently editable"),
  { tag: specTags(["ADM-043"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openWorkspaceCreate(driver, seed);
    let facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.slugPrefix).toMatch(/^https?:\/\//);
    expect(facts.submitDisabled).toBe(true);
    expect(facts.backHref).toContain("/workspace");
    await driver.adminWorkspaceCreateFillName("My Cool Workspace");
    await expect.poll(async () => (await driver.adminWorkspaceCreateFacts()).slug).toBe("my-cool-workspace");
    await driver.adminWorkspaceCreateFillSlug("custom-slug-1");
    facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.slug).toBe("custom-slug-1");
    expect(facts.name).toBe("My Cool Workspace");
    await driver.adminWorkspaceCreateFillSlug("UPPER-SLUG");
    facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.slug).toBe("upper-slug");
    expect(facts.orgSize).toBe(null);
    expect(facts.submitDisabled).toBe(true);
    await driver.adminWorkspaceCreateSelectOrgSize("2-10");
    facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.orgSize).toBe("2-10");
    expect(facts.submitDisabled).toBe(false);
    expect(facts.submitLabel).toBe("Create workspace");
  }
);

test(
  specTitle(["ADM-043"], "name and slug validation messages gate the submit"),
  { tag: specTags(["ADM-043"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await serverAdminListWorkspaces(session);
    await openWorkspaceCreate(driver, seed);
    await driver.adminWorkspaceCreateSelectOrgSize("2-10");
    await driver.adminWorkspaceCreateFillName("Bad{Name}");
    let facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.nameError).toContain("Workspace name");
    expect(facts.submitDisabled).toBe(true);
    await driver.adminWorkspaceCreateFillName("Good Name");
    await driver.adminWorkspaceCreateFillSlug("bad slug!");
    facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.invalidSlugVisible).toBe(true);
    expect(facts.submitDisabled).toBe(true);
    await driver.adminWorkspaceCreateFillSlug("good-slug-1");
    facts = await driver.adminWorkspaceCreateFacts();
    expect(facts.invalidSlugVisible).toBe(false);
    expect(facts.nameError).toBe(null);
    expect(facts.slugError).toBe(null);
    expect(facts.submitDisabled).toBe(false);
    const after = await serverAdminListWorkspaces(session);
    expect(after.results.length).toBe(before.results.length);
  }
);

test(
  specTitle(["ADM-044"], "taken slug shows the inline error and creates nothing"),
  { tag: specTags(["ADM-044"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await serverAdminListWorkspaces(session);
    expect(before.results.some((row) => row.slug === seed.workspaceSlug)).toBe(true);
    await openWorkspaceCreate(driver, seed);
    await driver.adminStartSlugCheckSpy();
    await driver.adminStartWorkspaceCreateSpy();
    await driver.adminWorkspaceCreateFillName("Taken Slug Probe");
    await driver.adminWorkspaceCreateFillSlug(seed.workspaceSlug);
    await driver.adminWorkspaceCreateSelectOrgSize("2-10");
    await driver.adminWorkspaceCreateSubmit();
    await expect
      .poll(async () => (await driver.adminWorkspaceCreateFacts()).slugTakenVisible, {
        timeout: 15_000,
      })
      .toBe(true);
    const slugs = await driver.adminSlugCheckSlugs();
    expect(slugs).toContain(seed.workspaceSlug);
    const creates = await driver.adminWorkspaceCreateBodies();
    expect(creates.length).toBe(0);
    const after = await serverAdminListWorkspaces(session);
    expect(after.results.length).toBe(before.results.length);
    const path = await driver.adminCurrentPath();
    expect(path).toContain("/workspace/create");
  }
);

test(
  specTitle(["ADM-044"], "restricted slug shows the taken error without a create"),
  { tag: specTags(["ADM-044"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await serverAdminListWorkspaces(session);
    const available = await serverAdminSlugCheck(session, "god-mode");
    expect(available).toBe(false);
    await openWorkspaceCreate(driver, seed);
    await driver.adminStartWorkspaceCreateSpy();
    await driver.adminWorkspaceCreateFillName("Restricted Probe");
    await driver.adminWorkspaceCreateFillSlug("god-mode");
    await driver.adminWorkspaceCreateSelectOrgSize("2-10");
    await driver.adminWorkspaceCreateSubmit();
    await expect
      .poll(async () => (await driver.adminWorkspaceCreateFacts()).slugTakenVisible, {
        timeout: 15_000,
      })
      .toBe(true);
    const creates = await driver.adminWorkspaceCreateBodies();
    expect(creates.length).toBe(0);
    const after = await serverAdminListWorkspaces(session);
    expect(after.results.length).toBe(before.results.length);
  }
);

test(
  specTitle(["ADM-044"], "slug-check failure toasts an error and stays on the form"),
  { tag: specTags(["ADM-044"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await serverAdminListWorkspaces(session);
    await openWorkspaceCreate(driver, seed);
    await driver.adminStartWorkspaceCreateSpy();
    await driver.adminFailSlugCheck();
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await driver.adminWorkspaceCreateFillName("Slug Failure Probe");
      await driver.adminWorkspaceCreateFillSlug(slug);
      await driver.adminWorkspaceCreateSelectOrgSize("2-10");
      await driver.adminWorkspaceCreateSubmit();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Error!");
      const creates = await driver.adminWorkspaceCreateBodies();
      expect(creates.length).toBe(0);
      const after = await serverAdminListWorkspaces(session);
      expect(after.results.length).toBe(before.results.length);
      const path = await driver.adminCurrentPath();
      expect(path).toContain("/workspace/create");
    } finally {
      await driver.adminClearSlugCheckFault().catch(() => {});
      await serverAdminDeleteWorkspacesBySlugPrefix(slug);
    }
  }
);

test(
  specTitle(["ADM-045"], "create succeeds with a notice and lands on the list as server owner"),
  { tag: specTags(["ADM-045"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    const name = `Created ${slug.slice(-6)}`;
    try {
      await openWorkspaceCreate(driver, seed);
      await driver.adminStartWorkspaceCreateSpy();
      await driver.adminWorkspaceCreateFillName(name);
      await driver.adminWorkspaceCreateFillSlug(slug);
      await driver.adminWorkspaceCreateSelectOrgSize("11-50");
      await driver.adminWorkspaceCreateSubmit();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success!");
      const toast = await driver.adminLastToast();
      expect(toast?.message).toContain("Workspace created successfully");
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 15_000 }).toContain("/workspace");
      const path = await driver.adminCurrentPath();
      expect(path).not.toContain("/create");
      const creates = await driver.adminWorkspaceCreateBodies();
      expect(creates.length).toBe(1);
      const body = creates[0] as Record<string, unknown>;
      expect(body["name"]).toBe(name);
      expect(body["slug"]).toBe(slug);
      expect(body["organization_size"]).toBe("11-50");
      expect("owner" in body).toBe(false);
      const server = await serverAdminListWorkspaces(session);
      const created = server.results.find((row) => row.slug === slug);
      expect(created).toBeDefined();
      expect(created?.name).toBe(name);
      expect(created?.ownerEmail).toBe(seed.email);
      await expect.poll(async () => (await driver.adminWorkspaceRows()).length, { timeout: 15_000 }).toBeGreaterThan(0);
      const rows = await driver.adminWorkspaceRows();
      expect(rows.some((row) => row.slug === slug)).toBe(true);
    } finally {
      await serverAdminDeleteWorkspacesBySlugPrefix(slug);
    }
  }
);

test(
  specTitle(["ADM-045"], "create failure toasts an error and stays on the form"),
  { tag: specTags(["ADM-045"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await serverAdminListWorkspaces(session);
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await openWorkspaceCreate(driver, seed);
      await driver.adminFailWorkspaceCreate();
      await driver.adminWorkspaceCreateFillName("Create Failure Probe");
      await driver.adminWorkspaceCreateFillSlug(slug);
      await driver.adminWorkspaceCreateSelectOrgSize("2-10");
      await driver.adminWorkspaceCreateSubmit();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Error!");
      const after = await serverAdminListWorkspaces(session);
      expect(after.results.length).toBe(before.results.length);
      const path = await driver.adminCurrentPath();
      expect(path).toContain("/workspace/create");
    } finally {
      await driver.adminClearWorkspaceCreateFault().catch(() => {});
      await serverAdminDeleteWorkspacesBySlugPrefix(slug);
    }
  }
);

test(
  specTitle(["ADM-043"], "submit shows a busy label while the create is in flight"),
  { tag: specTags(["ADM-043"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const slug = uniqueSlug(SPEC_SLUG_PREFIX);
    try {
      await openWorkspaceCreate(driver, seed);
      await driver.adminWorkspaceCreateFillName("Busy Label Probe");
      await driver.adminWorkspaceCreateFillSlug(slug);
      await driver.adminWorkspaceCreateSelectOrgSize("2-10");
      await driver.adminDelayWorkspaceCreate(3000);
      await driver.adminWorkspaceCreateSubmit();
      await expect
        .poll(async () => (await driver.adminWorkspaceCreateFacts()).submitLabel, { timeout: 15_000 })
        .toBe("Creating workspace");
      await driver.adminClearWorkspaceCreateFault();
      await expect.poll(async () => (await driver.adminLastToast())?.title, { timeout: 15_000 }).toBe("Success!");
    } finally {
      await driver.adminClearWorkspaceCreateFault().catch(() => {});
      await serverAdminDeleteWorkspacesBySlugPrefix(slug);
    }
  }
);
