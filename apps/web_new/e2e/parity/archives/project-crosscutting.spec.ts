// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-226): whole-project archive/restore through
// the confirmation dialog, the auto-archive project setting, and the
// archives cross-cutting rows (permissions, addresses, skeletons,
// confirmations, explicit absences).
// Rows: ARCH-026–ARCH-032.
//
// Scope notes, read before extending:
// - The project archive dialog is also driven from the projects list
//   (SHELL-037..039); this spec reuses those driver methods read-only and
//   proves the dialog-body warnings, the favorites cleanup, and the full
//   archive/restore round trip from the archives side. The auto-archive
//   row reuses the automations drivers read-only (AGT-034/036 own the
//   toggle/preset/custom behavior); this spec proves the same surface
//   from the archives side plus the admin/non-admin split.
// - Gap (mirrors the AGT-034 oracle gap): the rows promise non-admins
//   disabled auto-archive controls, but the settings page gates every
//   non-admin to the not-authorized view (AGT-037), so no read-only row
//   renders for them. The oracle asserts the refusal, and the rows carry
//   the gap note.
// - Cold entry (deep link, reload, shared URL, direct navigation) to the
//   archived cycles/modules tabs never settles for any role: the list
//   reads succeed but no live fetch arms the archived selectors, so the
//   skeleton persists (bug NEWFRONT-239). Content assertions on those
//   tabs drive through primed client-side navigation instead
//   (archivesPrimeAndEnter + archivesTabClick); the stuck behavior itself
//   is pinned by bug: scenarios.
// - Guests are refused the archived-issues/cycles reads server-side
//   (bug NEWFRONT-240) and have no in-app path to archives, so the guest
//   matrix asserts the settled refusals plus the API proofs.
// - ARCH-031's "every archive/restore" is proven here on the project
//   dialog (success, failure, progress, reset) plus one item-level
//   failure; the item surfaces belong to the sibling archives specs.
// - Fixtures are API-created in-spec on scratch projects; the seed
//   project itself is never archived. Primed flows create one live cycle
//   so the prime can settle deterministically.
import { test, expect } from "../fixtures";
import {
  ROLE,
  addProjectMembers,
  archivesArchiveCycle,
  archivesArchiveModule,
  archivesArchivedIssueStatus,
  archivesArchivedListStatus,
  archivesCycleArchivedAt,
  archivesModuleArchivedAt,
  archivesPatchModule,
  archivesProjectArchivedAt,
  createFavorite,
  ensureWorkspaceMember,
  listFavorites,
  parityProjectIdentifier,
  serverArchiveIssue,
  serverCleanupProject,
  serverCreateCycle,
  serverCreateIssueFull,
  serverCreateModule,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverIssue,
  serverPatchIssue,
  serverProjectAutomations,
  serverProjectStates,
  serverSessionUserId,
  signInSession,
  uniqueEmail,
} from "../helpers/api";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import { specTags, specTitle } from "../helpers/tags";

const PROJECT_DIALOG = ["ARCH-026"];
const AUTO_ARCHIVE = ["ARCH-027"];
const AUTO_ARCHIVE_GATE = ["ARCH-027", "ARCH-028"];
const PERMISSIONS_MEMBER = ["ARCH-028"];
const PERMISSIONS_GUEST = ["ARCH-028"];
const COLD_STUCK = ["ARCH-028", "ARCH-029", "ARCH-030"];
const ADDRESSES = ["ARCH-029"];
const SKELETONS = ["ARCH-030"];
const CONFIRMATIONS = ["ARCH-031"];
const ABSENCES = ["ARCH-032"];

/** Completed-or-cancelled state id, reusing a default or minting one. */
async function doneStateId(workspaceSlug: string, projectId: string, session: string, tag: string): Promise<string> {
  const states = await serverProjectStates(workspaceSlug, projectId, session);
  const seeded = states.find((s) => s.group === "completed" || s.group === "cancelled");
  if (seeded !== undefined) return seeded.id;
  return serverCreateState(workspaceSlug, projectId, `${tag} done`, "completed", session);
}

/** Create, complete, and archive one issue; resolves with its id. */
async function archivedIssue(
  workspaceSlug: string,
  projectId: string,
  name: string,
  session: string,
  tag: string
): Promise<string> {
  const issue = await serverCreateIssueFull(workspaceSlug, projectId, name, session);
  const stateId = await doneStateId(workspaceSlug, projectId, session, tag);
  await serverPatchIssue(workspaceSlug, projectId, issue.id, { state_id: stateId }, session);
  await serverArchiveIssue(workspaceSlug, projectId, issue.id, session);
  return issue.id;
}

/** Create a past cycle and archive it; resolves with its id. */
async function archivedCycle(workspaceSlug: string, projectId: string, name: string, session: string): Promise<string> {
  const id = await serverCreateCycle(workspaceSlug, projectId, name, "2020-01-01", "2020-01-31", session);
  await archivesArchiveCycle(workspaceSlug, projectId, id, session);
  return id;
}

/** Create a module, complete it, and archive it; resolves with its id. */
async function archivedModule(
  workspaceSlug: string,
  projectId: string,
  name: string,
  session: string
): Promise<string> {
  const id = await serverCreateModule(workspaceSlug, projectId, name, session);
  await archivesPatchModule(workspaceSlug, projectId, id, { status: "completed" }, session);
  await archivesArchiveModule(workspaceSlug, projectId, id, session);
  return id;
}

/** Create a live cycle for deterministic prime settling. */
async function liveCycle(workspaceSlug: string, projectId: string, name: string, session: string): Promise<string> {
  return serverCreateCycle(workspaceSlug, projectId, name, "2026-01-01", "2026-12-31", session);
}

/** Seed-guest credentials or a loud failure when the seed predates them. */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword) throw new Error("[parity] seed carries no guest identity.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

test(
  specTitle(PROJECT_DIALOG, "project archive warns, leaves lists and favorites, and restores"),
  { tag: specTags(PROJECT_DIALOG) },
  async ({ driver, seed }) => {
    const tag = `NF226 arch ${Date.now()}`;
    const name = `${tag} project`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      name,
      parityProjectIdentifier("N226A"),
      {},
      session
    );
    await createFavorite(seed.workspaceSlug, session, {
      entity_type: "project",
      entity_identifier: projectId,
      project_id: projectId,
      name,
    });
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the archive dialog warns what archiving takes with it", async () => {
        await driver.openArchiveProjectDialog(seed.workspaceSlug, projectId);
        const body = await driver.archiveDialogBodyText();
        expect(body).toContain("work items");
        expect(body).toContain("cycles");
        expect(body).toContain("modules");
        expect(body).toContain("pages");
        expect(body).toContain("search");
      });

      await test.step("confirming archives: toast, projects list, gone from active lists", async () => {
        await driver.confirmArchive();
        await expect.poll(() => driver.isToastVisible("has been archived successfully")).toBe(true);
        await expect.poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session)).not.toBeNull();
        await expect.poll(() => driver.currentUrlPath()).toContain("/projects");
        await expect.poll(() => driver.visibleProjectCardNames()).not.toContain(name);
      });

      await test.step("favorites entries for the project are removed server-side", async () => {
        const favs = await listFavorites(seed.workspaceSlug, session);
        expect(favs.filter((fav) => fav.entity_identifier === projectId)).toEqual([]);
      });

      await test.step("the restore dialog explains re-visibility to members", async () => {
        await driver.openArchivedProjects(seed.workspaceSlug);
        await driver.awaitProjectCard(name);
        await driver.clickCardRestore(name);
        const body = await driver.archiveDialogBodyText();
        expect(body).toContain("visible");
        expect(body).toContain("members");
      });

      await test.step("confirming restores: toast, back in the projects list", async () => {
        await driver.confirmRestore();
        await expect.poll(() => driver.isToastVisible("in your projects")).toBe(true);
        await expect.poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session)).toBeNull();
        await expect.poll(() => driver.currentUrlPath()).not.toContain("/archives");
        await driver.awaitProjectCard(name);
        await expect.poll(() => driver.visibleProjectCardNames()).toContain(name);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(AUTO_ARCHIVE, "auto-archive toggles with a one-month default; presets and custom ranges persist"),
  { tag: specTags(AUTO_ARCHIVE) },
  async ({ driver, seed }) => {
    const tag = `NF226 auto ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226B"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.automationsOpen(seed.workspaceSlug, projectId);
      expect(await driver.automationsNotAuthorizedVisible()).toBe(false);

      await test.step("a disabled row shows the toggle only", async () => {
        expect((await serverProjectAutomations(seed.workspaceSlug, projectId, session)).archive_in).toBe(0);
        const row = await driver.automationsArchiveRow();
        expect(row.toggleOn).toBe(false);
        expect(row.toggleDisabled).toBe(false);
        expect(row.pickerVisible).toBe(false);
      });

      await test.step("enabling defaults to one month and persists", async () => {
        await driver.automationsArchiveToggle();
        await expect.poll(async () => (await driver.automationsArchiveRow()).toggleOn).toBe(true);
        await expect.poll(async () => (await driver.automationsArchiveRow()).pickerVisible).toBe(true);
        await expect.poll(async () => (await driver.automationsArchiveRow()).pickerLabel).toBe("1 month");
        expect((await serverProjectAutomations(seed.workspaceSlug, projectId, session)).archive_in).toBe(1);
      });

      await test.step("a preset delay persists", async () => {
        await driver.automationsArchiveSetPreset(3);
        await expect.poll(async () => (await driver.automationsArchiveRow()).pickerLabel).toBe("3 months");
        expect((await serverProjectAutomations(seed.workspaceSlug, projectId, session)).archive_in).toBe(3);
      });

      await test.step("a custom range persists", async () => {
        await driver.automationsArchiveOpenCustom();
        expect(await driver.automationsMonthModal()).not.toBeNull();
        await driver.automationsMonthFill("7");
        await driver.automationsMonthSubmit();
        await expect.poll(() => driver.automationsMonthModal()).toBeNull();
        await expect.poll(async () => (await driver.automationsArchiveRow()).pickerLabel).toBe("7 months");
        expect((await serverProjectAutomations(seed.workspaceSlug, projectId, session)).archive_in).toBe(7);
      });

      await test.step("disabling returns to zero and hides the picker", async () => {
        await driver.automationsArchiveToggle();
        await expect.poll(async () => (await driver.automationsArchiveRow()).toggleOn).toBe(false);
        await expect.poll(async () => (await driver.automationsArchiveRow()).pickerVisible).toBe(false);
        expect((await serverProjectAutomations(seed.workspaceSlug, projectId, session)).archive_in).toBe(0);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(AUTO_ARCHIVE_GATE, "non-admins are refused the automations page instead of seeing disabled controls"),
  { tag: specTags(AUTO_ARCHIVE_GATE) },
  async ({ driver, seed }) => {
    const tag = `NF226 gate ${Date.now()}`;
    const guest = requireGuest(seed);
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226C"),
      {},
      session
    );
    const memberEmail = uniqueEmail("parity-arc28m");
    const memberSession = await ensureWorkspaceMember(
      seed.workspaceSlug,
      session,
      memberEmail,
      "Parity-Arc28-Member-1",
      ROLE.MEMBER
    );
    const memberId = await serverSessionUserId(memberSession);
    const guestSession = await signInSession(guest.email, guest.password);
    const guestId = await serverSessionUserId(guestSession);
    await addProjectMembers(seed.workspaceSlug, projectId, session, [
      { member_id: memberId, role: ROLE.MEMBER },
      { member_id: guestId, role: ROLE.GUEST },
    ]);
    try {
      await test.step("a project member is refused the page", async () => {
        await driver.rulesEnsureSignedIn(memberEmail, "Parity-Arc28-Member-1", seed.workspaceSlug);
        await driver.automationsOpen(seed.workspaceSlug, projectId);
        expect(await driver.automationsNotAuthorizedVisible()).toBe(true);
      });

      await test.step("a project guest is refused the page", async () => {
        await driver.resetSession();
        await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);
        await driver.automationsOpen(seed.workspaceSlug, projectId);
        expect(await driver.automationsNotAuthorizedVisible()).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(PERMISSIONS_MEMBER, "members browse every archives tab with restore entries and working affordances"),
  { tag: specTags(PERMISSIONS_MEMBER) },
  async ({ driver, seed }) => {
    const tag = `NF226 perm ${Date.now()}`;
    const liveName = `${tag} live`;
    const issueName = `${tag} issue`;
    const cycleName = `${tag} cycle`;
    const moduleName = `${tag} module`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226D"),
      { cycleView: true, moduleView: true },
      session
    );
    await liveCycle(seed.workspaceSlug, projectId, liveName, session);
    const issueId = await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    await archivedCycle(seed.workspaceSlug, projectId, cycleName, session);
    await archivedModule(seed.workspaceSlug, projectId, moduleName, session);
    const memberEmail = uniqueEmail("parity-arc28x");
    const memberSession = await ensureWorkspaceMember(
      seed.workspaceSlug,
      session,
      memberEmail,
      "Parity-Arc28-Member-1",
      ROLE.MEMBER
    );
    const memberId = await serverSessionUserId(memberSession);
    await addProjectMembers(seed.workspaceSlug, projectId, session, [{ member_id: memberId, role: ROLE.MEMBER }]);
    try {
      await driver.rulesEnsureSignedIn(memberEmail, "Parity-Arc28-Member-1", seed.workspaceSlug);
      // Primed client-side entry: cold entry to the cycles/modules tabs
      // never settles (NEWFRONT-239), so the matrix primes first.
      await driver.archivesPrimeAndEnter(seed.workspaceSlug, projectId, liveName);

      await test.step("issues tab: filter, restore menu, copy link, peek", async () => {
        expect(await driver.archivesActiveTab()).toBe("issues");
        expect(await driver.archivesFilterControlVisible()).toBe(true);
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
        await driver.archivesRowMenuOpenFirst(issueName);
        const entries = await driver.archivesRowMenuEntries();
        expect(entries).toContain("Restore");
        expect(entries).toContain("Copy link");
        expect(entries).toContain("Open in new tab");
        expect(entries).not.toContain("Archive");
        // Headless denies clipboard writes by default; grant first, like a
        // real session where the user already allowed it (SHELL-050 does).
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        await driver.archivesRowMenuClick("Copy link");
        await expect.poll(() => driver.isToastVisible("Link copied")).toBe(true);
        // Capability level only: a URL-shaped payload lands. The issue-row
        // payload carries an undefined identifier (bug NEWFRONT-243, owned
        // by ARCH-010/sibling NEWFRONT-223), so this row pins no shape.
        expect(await driver.readClipboard()).toContain("http");
        await driver.archivesPeekOpenFirst(issueName);
        expect(await driver.archivesPeekVisible()).toBe(true);
        await driver.archivesPeekClose();
        expect(await driver.archivesPeekVisible()).toBe(false);
      });

      await test.step("cycles tab: rows, restore menu, peek, search", async () => {
        await driver.archivesTabClick("cycles");
        await expect.poll(() => driver.archivesRowPresent(cycleName)).toBe(true);
        await driver.archivesRowMenuOpenFirst(cycleName);
        const entries = await driver.archivesRowMenuEntries();
        expect(entries).toContain("Restore");
        expect(entries).not.toContain("Archive");
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        await driver.archivesRowMenuClick("Copy link");
        await expect.poll(() => driver.isToastVisible("Link copied")).toBe(true);
        // Capability level only: a URL-shaped payload lands. The issue-row
        // payload carries an undefined identifier (bug NEWFRONT-243, owned
        // by ARCH-010/sibling NEWFRONT-223), so this row pins no shape.
        expect(await driver.readClipboard()).toContain("http");
        await driver.archivesPeekOpenFirst(cycleName);
        expect(await driver.archivesPeekVisible()).toBe(true);
        await driver.archivesPeekClose();
        expect(await driver.archivesPeekVisible()).toBe(false);
        await driver.archivesSearchType(cycleName.slice(0, 12));
        await expect.poll(() => driver.archivesRowPresent(cycleName)).toBe(true);
      });

      await test.step("modules tab: rows, restore menu, peek", async () => {
        await driver.archivesTabClick("modules");
        await expect.poll(() => driver.archivesRowPresent(moduleName)).toBe(true);
        await driver.archivesRowMenuOpenFirst(moduleName);
        const entries = await driver.archivesRowMenuEntries();
        expect(entries).toContain("Restore");
        expect(entries).not.toContain("Archive");
        await driver.archivesPeekOpenFirst(moduleName);
        expect(await driver.archivesPeekVisible()).toBe(true);
        await driver.archivesPeekClose();
        expect(await driver.archivesPeekVisible()).toBe(false);
      });

      await test.step("member opens the archived detail and its banner", async () => {
        await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issueId);
        const banner = await driver.archivesDetailBannerText();
        expect(banner).toContain("archived");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(PERMISSIONS_GUEST, "bug: NEWFRONT-240 guests are refused archived reads and left on stuck shells"),
  { tag: specTags(PERMISSIONS_GUEST) },
  async ({ driver, seed }) => {
    const tag = `NF226 gst ${Date.now()}`;
    const guest = requireGuest(seed);
    const issueName = `${tag} issue`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226E"),
      { cycleView: true, moduleView: true },
      session
    );
    const issueId = await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    await archivedCycle(seed.workspaceSlug, projectId, `${tag} cycle`, session);
    await archivedModule(seed.workspaceSlug, projectId, `${tag} module`, session);
    const guestSession = await signInSession(guest.email, guest.password);
    const guestId = await serverSessionUserId(guestSession);
    await addProjectMembers(seed.workspaceSlug, projectId, session, [{ member_id: guestId, role: ROLE.GUEST }]);
    try {
      await test.step("the server refuses guest list and detail reads, except modules", async () => {
        expect(await archivesArchivedListStatus(seed.workspaceSlug, projectId, "issues", guestSession)).toBe(403);
        expect(await archivesArchivedListStatus(seed.workspaceSlug, projectId, "cycles", guestSession)).toBe(403);
        expect(await archivesArchivedListStatus(seed.workspaceSlug, projectId, "modules", guestSession)).toBe(200);
        expect(await archivesArchivedIssueStatus(seed.workspaceSlug, projectId, issueId, guestSession)).toBe(403);
      });

      await driver.rulesEnsureSignedIn(guest.email, guest.password, seed.workspaceSlug);

      await test.step("the issues tab sticks on its skeleton with no rows", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        await new Promise((resolve) => setTimeout(resolve, 25_000));
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        expect(await driver.archivesRowPresent(issueName)).toBe(false);
      });

      await test.step("the cycles and modules tabs are stuck shells", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "cycles");
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        await new Promise((resolve) => setTimeout(resolve, 25_000));
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "modules");
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        await new Promise((resolve) => setTimeout(resolve, 25_000));
        expect(await driver.archivesSkeletonVisible()).toBe(true);
      });

      await test.step("the detail address renders no banner or detail", async () => {
        await driver.goToPath(`/${seed.workspaceSlug}/projects/${projectId}/archives/issues/${issueId}`);
        await expect.poll(() => driver.currentUrlPath()).toContain(issueId);
        expect(await driver.archivesDetailBannerText()).toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(COLD_STUCK, "bug: NEWFRONT-239 cold entry to the cycles/modules tabs never settles"),
  { tag: specTags(COLD_STUCK) },
  async ({ driver, seed }) => {
    const tag = `NF226 cold ${Date.now()}`;
    const liveName = `${tag} live`;
    const cycleName = `${tag} cycle`;
    const moduleName = `${tag} module`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226F"),
      { cycleView: true, moduleView: true },
      session
    );
    await liveCycle(seed.workspaceSlug, projectId, liveName, session);
    await archivedCycle(seed.workspaceSlug, projectId, cycleName, session);
    await archivedModule(seed.workspaceSlug, projectId, moduleName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the cycles tab reads 200 yet its skeleton never resolves", async () => {
        await driver.archivesBeginTrafficSpy();
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "cycles");
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await expect.poll(async () => (await driver.archivesTrafficCounts()).cyclesReads).toBeGreaterThanOrEqual(1);
        await new Promise((resolve) => setTimeout(resolve, 30_000));
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        expect(await driver.archivesRowPresent(cycleName)).toBe(false);
      });

      await test.step("the modules tab reads 200 yet its skeleton never resolves", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "modules");
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await expect.poll(async () => (await driver.archivesTrafficCounts()).modulesReads).toBeGreaterThanOrEqual(1);
        await new Promise((resolve) => setTimeout(resolve, 30_000));
        expect(await driver.archivesSkeletonVisible()).toBe(true);
        expect(await driver.archivesRowPresent(moduleName)).toBe(false);
      });

      await test.step("a cycle peek opened while primed is gone after reload", async () => {
        await driver.archivesPrimeAndEnter(seed.workspaceSlug, projectId, liveName);
        await driver.archivesTabClick("cycles");
        await expect.poll(() => driver.archivesRowPresent(cycleName)).toBe(true);
        await driver.archivesPeekOpenFirst(cycleName);
        expect(await driver.archivesPeekVisible()).toBe(true);
        await driver.reloadPage();
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await new Promise((resolve) => setTimeout(resolve, 20_000));
        expect(await driver.archivesPeekVisible()).toBe(false);
        expect(await driver.archivesRowPresent(cycleName)).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(ADDRESSES, "archives tab, detail, and issue-peek addresses survive reloads and clear on close"),
  { tag: specTags(ADDRESSES) },
  async ({ driver, seed }) => {
    const tag = `NF226 addr ${Date.now()}`;
    const issueName = `${tag} issue`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226G"),
      { cycleView: true, moduleView: true },
      session
    );
    const issueId = await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    await archivedCycle(seed.workspaceSlug, projectId, `${tag} cycle`, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("the issues address reloads onto the same tab", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        expect(await driver.currentUrlPath()).toContain("/archives/issues");
        await driver.reloadPage();
        await expect.poll(() => driver.archivesActiveTab()).toBe("issues");
        expect(await driver.currentUrlPath()).toContain("/archives/issues");
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
      });

      for (const tab of ["cycles", "modules"] as const) {
        await test.step(`the ${tab} address loads its shell (content sticks per NEWFRONT-239)`, async () => {
          await driver.archivesTabOpen(seed.workspaceSlug, projectId, tab);
          expect(await driver.currentUrlPath()).toContain(`/archives/${tab}`);
          expect(await driver.archivesTabNames()).toEqual(["Work items", "Cycles", "Modules"]);
          await driver.reloadPage();
          await expect.poll(() => driver.archivesActiveTab()).toBe(tab);
          expect(await driver.currentUrlPath()).toContain(`/archives/${tab}`);
        });
      }

      await test.step("the detail address reloads to banner plus detail", async () => {
        await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issueId);
        expect(await driver.currentUrlPath()).toContain(issueId);
        expect(await driver.archivesDetailBannerText()).toContain("archived");
        await driver.reloadPage();
        await expect.poll(() => driver.archivesDetailBannerText()).toContain("archived");
        expect(await driver.currentUrlPath()).toContain(issueId);
      });

      await test.step("the issue peek param persists across reload and clears on close", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await driver.archivesPeekOpenFirst(issueName);
        expect(await driver.archivesPeekVisible()).toBe(true);
        await driver.reloadPage();
        await expect.poll(() => driver.archivesPeekVisible()).toBe(true);
        await driver.archivesPeekClose();
        expect(await driver.archivesPeekVisible()).toBe(false);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(SKELETONS, "archive tabs and detail show skeletons on first fetch, then content, empty, or no-match"),
  { tag: specTags(SKELETONS) },
  async ({ driver, seed }) => {
    const tag = `NF226 skel ${Date.now()}`;
    const liveName = `${tag} live`;
    const issueName = `${tag} issue`;
    const moduleName = `${tag} module`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226H"),
      { cycleView: true, moduleView: true },
      session
    );
    await liveCycle(seed.workspaceSlug, projectId, liveName, session);
    const issueId = await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    await archivedModule(seed.workspaceSlug, projectId, moduleName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("issues tab: skeleton, then the archived row", async () => {
        await driver.archivesDelayNextListReads(3000);
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(false);
      });

      await test.step("cycles tab: skeleton, then the empty state", async () => {
        await driver.archivesPrimeAndEnter(seed.workspaceSlug, projectId, liveName);
        await driver.archivesDelayNextListReads(3000);
        await driver.archivesTabClick("cycles");
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(false);
        expect(await driver.archivesRowPresent(`${tag} nope`)).toBe(false);
        expect(await driver.archivesTabNames()).toContain("Cycles");
      });

      await test.step("modules tab: skeleton, then content, then a no-match search", async () => {
        await driver.archivesDelayNextListReads(3000);
        await driver.archivesTabClick("modules");
        await expect.poll(() => driver.archivesSkeletonVisible()).toBe(true);
        await expect.poll(() => driver.archivesRowPresent(moduleName)).toBe(true);
        await driver.archivesSearchType("zzz-no-such-module");
        expect(await driver.archivesSearchText()).toBe("zzz-no-such-module");
        await expect.poll(() => driver.archivesRowPresent(moduleName)).toBe(false);
      });

      await test.step("Detail: skeleton, then banner plus detail", async () => {
        await driver.archivesDelayNextListReads(3000);
        // The detail loader flashes for a fraction of a second between
        // the store write and the fetch settle, so the step records the
        // mount with an observer instead of racing it with a live poll.
        await driver.archivesArmSkeletonObserver();
        await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issueId);
        expect(await driver.archivesDetailBannerText()).toContain("archived");
        expect(await driver.archivesSkeletonWasSeen()).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(CONFIRMATIONS, "project archive and restore confirm with progress; failures keep prior state"),
  { tag: specTags(CONFIRMATIONS) },
  async ({ driver, seed }) => {
    const tag = `NF226 conf ${Date.now()}`;
    const name = `${tag} project`;
    const issueName = `${tag} issue`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      name,
      parityProjectIdentifier("N226I"),
      { cycleView: true, moduleView: true },
      session
    );
    const issueId = await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("confirm shows progress while the archive runs, then success", async () => {
        await driver.archivesProjectDelayNextWrite(3000);
        await driver.openArchiveProjectDialog(seed.workspaceSlug, projectId);
        expect(await driver.archivesProjectConfirmLabel()).toBe("Archive");
        expect(await driver.archivesProjectConfirmBusy()).toBe(false);
        await Promise.all([expect.poll(() => driver.archivesProjectConfirmBusy()).toBe(true), driver.confirmArchive()]);
        await expect.poll(() => driver.isToastVisible("has been archived successfully")).toBe(true);
        await expect.poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session)).not.toBeNull();
      });

      await test.step("closing resets the dialog; reopening shows the idle confirm", async () => {
        await driver.openArchivedProjects(seed.workspaceSlug);
        await driver.awaitProjectCard(name);
        await driver.clickCardRestore(name);
        expect(await driver.archivesProjectConfirmLabel()).toBe("Restore");
        await driver.archivesPressKey("Escape");
        await expect.poll(() => driver.archiveDialogBodyText()).toBeNull();
        await driver.clickCardRestore(name);
        expect(await driver.archivesProjectConfirmLabel()).toBe("Restore");
        expect(await driver.archivesProjectConfirmBusy()).toBe(false);
      });

      await test.step("a failing restore errors and keeps the project archived", async () => {
        await driver.archivesProjectFailNextWrite();
        await driver.confirmRestore();
        await expect.poll(() => driver.isToastVisible("could not be restored")).toBe(true);
        expect(await archivesProjectArchivedAt(seed.workspaceSlug, projectId, session)).not.toBeNull();
        expect(await driver.archiveDialogBodyText()).not.toBeNull();
        expect(await driver.archivesProjectConfirmBusy()).toBe(false);
      });

      await test.step("retrying restores with success", async () => {
        await driver.confirmRestore();
        await expect.poll(() => driver.isToastVisible("in your projects")).toBe(true);
        await expect.poll(() => archivesProjectArchivedAt(seed.workspaceSlug, projectId, session)).toBeNull();
      });

      await test.step("a failing item restore errors and the item stays archived", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await driver.archivesItemFailNextWrite();
        await driver.archivesRowMenuOpenFirst(issueName);
        await driver.archivesRowMenuClick("Restore");
        await expect.poll(() => driver.isToastVisible("could not be restored")).toBe(true);
        expect((await serverIssue(seed.workspaceSlug, projectId, issueId, session)).archived_at).not.toBeNull();
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(ABSENCES, "archives load once per entry with no shortcuts, drag-drop, exports, or edition splits"),
  { tag: specTags(ABSENCES) },
  async ({ driver, seed }) => {
    const tag = `NF226 abs ${Date.now()}`;
    const issueName = `${tag} issue`;
    const issueName2 = `${tag} issue-b`;
    const cycleName = `${tag} cycle`;
    const moduleName = `${tag} module`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226J"),
      { cycleView: true, moduleView: true },
      session
    );
    await archivedIssue(seed.workspaceSlug, projectId, issueName, session, tag);
    await archivedIssue(seed.workspaceSlug, projectId, issueName2, session, tag);
    const cycleId = await archivedCycle(seed.workspaceSlug, projectId, cycleName, session);
    const moduleId = await archivedModule(seed.workspaceSlug, projectId, moduleName, session);
    expect(await archivesCycleArchivedAt(seed.workspaceSlug, projectId, cycleId, session)).not.toBeNull();
    expect(await archivesModuleArchivedAt(seed.workspaceSlug, projectId, moduleId, session)).not.toBeNull();
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.archivesBeginTrafficSpy();

      await test.step("each tab loads its list once, then idles without polling", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
        // The dev oracle double-mounts (StrictMode), so one entry issues
        // two reads; the absence proof is stability — no polling while idle.
        const first = (await driver.archivesTrafficCounts()).issuesReads;
        expect(first).toBeGreaterThanOrEqual(1);
        await new Promise((resolve) => setTimeout(resolve, 6000));
        const idle = await driver.archivesTrafficCounts();
        expect(idle.issuesReads).toBe(first);
        expect(idle.cyclesReads).toBe(0);
        expect(idle.modulesReads).toBe(0);
        expect(idle.writes).toBe(0);
      });

      await test.step("re-entry refreshes the list", async () => {
        const before = await driver.archivesTrafficCounts();
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "cycles");
        await expect
          .poll(async () => (await driver.archivesTrafficCounts()).cyclesReads)
          .toBeGreaterThan(before.cyclesReads);
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
        expect((await driver.archivesTrafficCounts()).issuesReads).toBeGreaterThan(before.issuesReads);
      });

      await test.step("search filters client-side without new reads", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "cycles");
        const before = (await driver.archivesTrafficCounts()).cyclesReads;
        await driver.archivesSearchType(cycleName.slice(0, 12));
        expect(await driver.archivesSearchText()).toBe(cycleName.slice(0, 12));
        await driver.archivesSearchType("zzz-no-such-cycle");
        expect(await driver.archivesSearchText()).toBe("zzz-no-such-cycle");
        expect((await driver.archivesTrafficCounts()).cyclesReads).toBe(before);
      });

      await test.step("keyboard input alone archives, restores, and opens nothing", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "modules");
        await expect.poll(async () => (await driver.archivesTrafficCounts()).modulesReads).toBeGreaterThan(0);
        for (const key of ["a", "r", "Delete", "Enter"]) {
          await driver.archivesPressKey(key);
        }
        expect(await driver.archiveDialogBodyText()).toBeNull();
        expect(await driver.archivesPeekVisible()).toBe(false);
        expect(await driver.archivesRowMenuEntries()).toEqual([]);
        expect((await driver.archivesTrafficCounts()).writes).toBe(0);
      });

      await test.step("issue rows ignore drag attempts and no export control renders", async () => {
        await driver.archivesTabOpen(seed.workspaceSlug, projectId, "issues");
        await expect.poll(() => driver.archivesRowPresent(issueName)).toBe(true);
        await expect.poll(() => driver.archivesRowPresent(issueName2)).toBe(true);
        // The rows carry a vestigial draggable attribute (shared list-row
        // component), so the absence proof is behavioral: a real drag
        // reorders nothing and writes nothing.
        const writesBefore = (await driver.archivesTrafficCounts()).writes;
        expect(await driver.archivesDragReorders(issueName, issueName2)).toBe(false);
        expect((await driver.archivesTrafficCounts()).writes).toBe(writesBefore);
        expect(await driver.archivesExportControlVisible()).toBe(false);
      });

      await test.step("the oracle build serves the same screens and endpoints", async () => {
        expect(await driver.archivesTabNames()).toEqual(["Work items", "Cycles", "Modules"]);
        const counts = await driver.archivesTrafficCounts();
        expect(counts.issuesReads).toBeGreaterThan(0);
        expect(counts.cyclesReads).toBeGreaterThan(0);
        expect(counts.modulesReads).toBeGreaterThan(0);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(ABSENCES, "cycle and module rows are not draggable either"),
  { tag: specTags(ABSENCES) },
  async ({ driver, seed }) => {
    const tag = `NF226 abs2 ${Date.now()}`;
    const liveName = `${tag} live`;
    const cycleName = `${tag} cycle`;
    const moduleName = `${tag} module`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N226K"),
      { cycleView: true, moduleView: true },
      session
    );
    await liveCycle(seed.workspaceSlug, projectId, liveName, session);
    await archivedCycle(seed.workspaceSlug, projectId, cycleName, session);
    await archivedModule(seed.workspaceSlug, projectId, moduleName, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      // Primed client-side entry: cold entry to the cycles/modules tabs
      // never settles (NEWFRONT-239), so the rows need the prime.
      await driver.archivesPrimeAndEnter(seed.workspaceSlug, projectId, liveName);
      await driver.archivesTabClick("cycles");
      await expect.poll(() => driver.archivesRowPresent(cycleName)).toBe(true);
      expect(await driver.archivesRowDraggable(cycleName)).toBe(false);
      await driver.archivesTabClick("modules");
      await expect.poll(() => driver.archivesRowPresent(moduleName)).toBe(true);
      expect(await driver.archivesRowDraggable(moduleName)).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
