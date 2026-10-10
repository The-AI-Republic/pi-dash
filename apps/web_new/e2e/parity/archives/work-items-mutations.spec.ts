// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-223): archived work-item mutations —
// restore an archived item from the row menu (ARCH-008), permanently
// delete one with confirmation (ARCH-009), copy-link and open-in-new-tab
// from the row menu (ARCH-010), the archived detail address with its
// banner (ARCH-011), detail quick actions with locked editing (ARCH-012),
// and the archive-a-live-item dialog with completed-or-canceled gating
// (ARCH-013). Green on apps/web first; every branch asserts what the
// user sees plus the resulting server state.
//
// Four scenarios pin old-app bugs (bug:): the row menu omits Delete
// (NEWFRONT-165), row links point at a broken browse/undefined-N address
// (NEWFRONT-235), a cold detail entry shows no loader while fetching
// (NEWFRONT-245), and a missing item renders blank instead of not-found
// (NEWFRONT-236). Each asserts the actual behavior; the linked issue and
// the inventory row record the intended behavior.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  issueSeqForName,
  issueStatus,
  parityProjectIdentifier,
  serverArchivedIssues,
  serverArchivedIssuesStatus,
  serverArchiveIssue,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDeleteState,
  serverIssue,
  serverIssues,
  serverMe,
  serverPatchIssue,
  serverProject,
  serverProjectStates,
  serverUnarchiveIssue,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test.use({ viewport: { width: 1600, height: 900 } });

type Seed = Pick<ParitySeedFacts, "email" | "password" | "workspaceSlug" | "projectId">;

async function openPath(
  driver: Pick<ParityDriver, "openAuthenticated">,
  path: string,
  sessionCookie: string
): Promise<void> {
  await driver.openAuthenticated(path, sessionBrowserCookies(sessionCookie));
}

/** A scenario-owned project; the caller deletes it in a finally block. */
async function makeProject(session: string, seed: Seed, tag: string, prefix: string): Promise<string> {
  return serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier(prefix),
    {},
    session
  );
}

/**
 * A completed-or-canceled state of the project: reuse a seeded one when
 * present, else mint a scenario-owned one (returned for teardown).
 */
async function doneState(
  workspaceSlug: string,
  projectId: string,
  tag: string,
  session: string
): Promise<{ id: string; owned: string | null }> {
  const states = await serverProjectStates(workspaceSlug, projectId, session);
  const seededDone = states.find((s) => s.group === "completed" || s.group === "cancelled");
  if (seededDone !== undefined) return { id: seededDone.id, owned: null };
  const owned = await serverCreateState(workspaceSlug, projectId, `${tag} done`, "completed", session);
  return { id: owned, owned };
}

async function deleteOwnedState(
  workspaceSlug: string,
  projectId: string,
  owned: string | null,
  session: string
): Promise<void> {
  if (owned !== null) await serverDeleteState(workspaceSlug, projectId, owned, session).catch(() => {});
}

/** An issue moved to `doneId` and archived; the caller cleans it up. */
async function makeArchived(
  workspaceSlug: string,
  projectId: string,
  name: string,
  doneId: string,
  session: string
): Promise<{ id: string; name: string }> {
  const issue = await serverCreateIssueFull(workspaceSlug, projectId, name, session);
  await serverPatchIssue(workspaceSlug, projectId, issue.id, { state_id: doneId }, session);
  await serverArchiveIssue(workspaceSlug, projectId, issue.id, session);
  return issue;
}

test(
  specTitle(["ARCH-008"], "restore an archived work item from the row menu"),
  { tag: specTags(["ARCH-008"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 restore ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223R");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await makeArchived(seed.workspaceSlug, projectId, name, done.id, session);
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues`, session);
      await driver.archivesOpenList(seed.workspaceSlug, projectId);

      await test.step("the archived list shows the item with a restore entry", async () => {
        await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 300_000 }).toContain(name);
        expect(await driver.layoutsRowMenuItems(name)).toContain("Restore");
        expect((await serverArchivedIssues(seed.workspaceSlug, projectId, session)).map((r) => r.id)).toContain(
          issue.id
        );
      });

      await test.step("a failed restore keeps the item archived with an error confirmation", async () => {
        await driver.archivesFailNextMutation("DELETE", `/issues/${issue.id}/archive/`);
        try {
          await driver.layoutsRowMenuChoose(name, "Restore");
          await expect.poll(() => driver.sawToast("could not be restored"), { timeout: 60_000 }).toBe(true);
        } finally {
          await driver.archivesClearMutationFailure();
        }
        expect(await driver.archivesVisibleIssueNames()).toContain(name);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).archived_at).not.toBeNull();
      });

      await test.step("restore removes it from archives and it reappears among live items", async () => {
        await driver.layoutsRowMenuChoose(name, "Restore");
        await expect.poll(() => driver.sawToast("Restore success"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 120_000 }).not.toContain(name);
        expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).archived_at).toBeNull();
        expect((await serverArchivedIssues(seed.workspaceSlug, projectId, session)).map((r) => r.id)).not.toContain(
          issue.id
        );

        await driver.openProjectIssues(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.visibleIssueNames(), { timeout: 300_000 }).toContain(name);
        expect((await serverIssues(seed.workspaceSlug, projectId, session)).map((r) => r.id)).toContain(issue.id);
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-008"], "guests see no archived rows and no restore entry"),
  { tag: specTags(["ARCH-008"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    if (!seed.guestEmail || !seed.guestPassword) {
      throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
    }
    const tag = `NF223 guest ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const guestSession = await signInSession(seed.guestEmail, seed.guestPassword);
    const guestId = (await serverMe(guestSession)).id;
    // The seeded project: the guest is already seated on it.
    const done = await doneState(seed.workspaceSlug, seed.projectId, tag, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, seed.projectId, name, session);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { assignee_ids: [guestId] }, session);
    await serverPatchIssue(seed.workspaceSlug, seed.projectId, issue.id, { state_id: done.id }, session);
    await serverArchiveIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
    try {
      // Server proof first: the endpoint refuses guests, so no rows can
      // ever render for them; the owner reads the same list fine.
      const guestRead = await serverArchivedIssuesStatus(seed.workspaceSlug, seed.projectId, guestSession);
      expect(guestRead.status).toBe(403);
      const ownerRead = await serverArchivedIssuesStatus(seed.workspaceSlug, seed.projectId, session);
      expect(ownerRead.status).toBe(200);
      expect(ownerRead.rows.map((r) => r.id)).toContain(issue.id);

      await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/archives/issues`, guestSession);
      await driver.archivesOpenList(seed.workspaceSlug, seed.projectId);
      await expect.poll(() => driver.hasVisibleText("Display"), { timeout: 300_000 }).toBe(true);
      expect(await driver.archivesVisibleIssueNames()).toEqual([]);
      expect(await driver.hasVisibleText("Restore")).toBe(false);
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, seed.projectId, done.owned, session);
    }
  }
);

test(
  specTitle(["ARCH-009"], "bug: NEWFRONT-165 archived row menu offers no delete entry"),
  { tag: specTags(["ARCH-009"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 norowdel ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223N");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await makeArchived(seed.workspaceSlug, projectId, name, done.id, session);
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues`, session);
      await driver.archivesOpenList(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 300_000 }).toContain(name);

      // Actual: even the project owner gets no Delete entry; the menu
      // itself proves it opened on the right row.
      const entries = await driver.layoutsRowMenuItems(name);
      expect(entries).not.toContain("Delete");
      expect(entries).toContain("Restore");
      expect(entries).toContain("Copy link");
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-009"], "delete an archived work item with confirmation from its detail"),
  { tag: specTags(["ARCH-009"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 delete ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223D");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await makeArchived(seed.workspaceSlug, projectId, name, done.id, session);
    try {
      // The row menu carries no Delete (bug NEWFRONT-165), so the dialog
      // flow runs where Delete exists: the detail header menu.
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues/${issue.id}`, session);
      await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issue.id);
      expect(await driver.layoutsDetailMenuItems()).toContain("Delete");

      await test.step("a failed delete keeps the item with an error confirmation", async () => {
        await driver.archivesDetailMenuChoose("Delete");
        expect(await driver.layoutsDeleteModalVisible()).toBe(true);
        await driver.archivesFailNextMutation("DELETE", `/issues/${issue.id}/`);
        try {
          await driver.layoutsDeleteModalConfirm();
          await expect.poll(() => driver.sawToast("delete failed"), { timeout: 60_000 }).toBe(true);
        } finally {
          await driver.archivesClearMutationFailure();
        }
        expect((await issueStatus(seed.workspaceSlug, projectId, issue.id, session)).status).toBe(200);
      });

      await test.step("confirming deletes the row from the UI and the server", async () => {
        await driver.archivesDetailMenuChoose("Delete");
        expect(await driver.layoutsDeleteModalVisible()).toBe(true);
        await driver.layoutsDeleteModalConfirm();
        await expect.poll(() => driver.sawToast("deleted successfully"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.currentUrlPath(), { timeout: 120_000 }).toMatch(/\/archives\/issues\/?$/);
        await driver.archivesOpenList(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 120_000 }).not.toContain(name);
        expect((await issueStatus(seed.workspaceSlug, projectId, issue.id, session)).status).toBe(404);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-010"], "bug: NEWFRONT-235 archived row links point at browse/undefined-N"),
  { tag: specTags(["ARCH-010"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 link ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223L");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, name, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: done.id }, session);
    // Sequence first: the live-issues read behind issueSeqForName stops
    // listing the issue once it is archived.
    const identifier = (await serverProject(seed.workspaceSlug, projectId, session)).identifier;
    const ref = await issueSeqForName(seed.workspaceSlug, projectId, identifier, name, session);
    const seqNum = ref.seq.split("-").pop() ?? "";
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues`, session);
      await driver.archivesOpenList(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 300_000 }).toContain(name);

      await test.step("open-in-new-tab lands on the broken undefined-identifier address", async () => {
        const url = await driver.layoutsRowMenuOpenNewTabUrl(name);
        expect(url).toContain(`/browse/undefined-${seqNum}/`);
        expect(url).not.toContain(`/archives/issues/${issue.id}`);
      });

      await test.step("copy-link copies the same broken address with a success confirmation", async () => {
        await driver.layoutsRowMenuChoose(name, "Copy link");
        await expect.poll(() => driver.sawToast("Link copied"), { timeout: 60_000 }).toBe(true);
        const clipboard = await driver.readClipboard();
        expect(clipboard).toContain(`/browse/undefined-${seqNum}/`);
        expect(clipboard).not.toContain(`/archives/issues/${issue.id}`);
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-011"], "archived work-item detail address with archive banner"),
  { tag: specTags(["ARCH-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 detail ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223V");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, name, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: done.id }, session);
    // Sequence first: the live-issues read behind issueSeqForName stops
    // listing the issue once it is archived.
    const identifier = (await serverProject(seed.workspaceSlug, projectId, session)).identifier;
    const ref = await issueSeqForName(seed.workspaceSlug, projectId, identifier, name, session);
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues/${issue.id}`, session);
      await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the breadcrumb names the item by identifier plus sequence number", async () => {
        await expect.poll(() => driver.archivesDetailBreadcrumbText(), { timeout: 120_000 }).toContain(ref.seq);
      });

      await test.step("the banner explains the archive and leads back to the archived list", async () => {
        await expect.poll(() => driver.archivesDetailBannerText(), { timeout: 120_000 }).toContain("archived");
        await driver.archivesDetailBannerBack();
        await expect.poll(() => driver.currentUrlPath(), { timeout: 120_000 }).toMatch(/\/archives\/issues\/?$/);
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-011"], "bug: NEWFRONT-245 archived detail shows no loader while fetching"),
  { tag: specTags(["ARCH-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 noloader ${Date.now()}`;
    const name = `${tag} item`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223H");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, name, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, issue.id, { state_id: done.id }, session);
    // Sequence first: the live-issues read behind issueSeqForName stops
    // listing the issue once it is archived.
    const identifier = (await serverProject(seed.workspaceSlug, projectId, session)).identifier;
    const ref = await issueSeqForName(seed.workspaceSlug, projectId, identifier, name, session);
    await serverArchiveIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/archives/issues/${issue.id}`, session);

      // Hold the record read on cold entry: the pending screen is sampled
      // with the fetch provably outstanding, so the sample cannot be
      // mistaken for a slow boot.
      const out = await driver.archivesDetailPendingStateOnHeldFetch(seed.workspaceSlug, projectId, issue.id);

      // Actual: the content area stays blank — no banner, no activity
      // section — and the breadcrumb carries no identifier-plus-sequence
      // reference until the record lands. Intended: a loader shows while
      // fetching (bug NEWFRONT-245).
      expect(out.heldRequests).toBeGreaterThan(0);
      expect(out.bannerDuringHold).toBeNull();
      expect(out.breadcrumbDuringHold).not.toContain(ref.seq);
      expect(out.activityDuringHold).toBe(false);
      expect(out.settled).toBe(true);

      // The released fetch settles on the full detail, and the hold never
      // mutated anything: the item is still archived on the server.
      await expect.poll(() => driver.archivesDetailBannerText(), { timeout: 120_000 }).toContain("archived");
      await expect.poll(() => driver.archivesDetailBreadcrumbText(), { timeout: 120_000 }).toContain(ref.seq);
      expect((await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).archived_at).not.toBeNull();
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-011"], "bug: NEWFRONT-236 missing archived item renders blank, not not-found"),
  { tag: specTags(["ARCH-011"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const missingId = "00000000-0000-0000-0000-000000000000";
    const session = await signInSession(seed.email, seed.password);
    await openPath(driver, `/${seed.workspaceSlug}/projects/${seed.projectId}/archives/issues/${missingId}`, session);
    await driver.archivesOpenDetailRaw(seed.workspaceSlug, seed.projectId, missingId);

    // Settle on the chrome, then prove the blank: no not-found state, no
    // banner, no error text anywhere, and no redirect away. The failing
    // record fetch resolves in milliseconds, so a short grace past the
    // chrome paint makes the negatives deterministic.
    await expect.poll(() => driver.hasVisibleText("Archives"), { timeout: 300_000 }).toBe(true);
    await driver.page.waitForTimeout(5000);
    expect(await driver.archivesDetailNotFoundVisible()).toBe(false);
    expect(await driver.archivesDetailBannerText()).toBeNull();
    expect(await driver.hasVisibleText("does not exist")).toBe(false);
    expect(await driver.currentUrlPath()).toContain(missingId);
  }
);

test(
  specTitle(["ARCH-012"], "detail quick actions: restore, delete, hidden subscribe, locked editing"),
  { tag: specTags(["ARCH-012"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 detailact ${Date.now()}`;
    const restoreName = `${tag} restore`;
    const deleteName = `${tag} delete`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223Q");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const restoreIssue = await makeArchived(seed.workspaceSlug, projectId, restoreName, done.id, session);
    const deleteIssue = await makeArchived(seed.workspaceSlug, projectId, deleteName, done.id, session);
    try {
      await openPath(
        driver,
        `/${seed.workspaceSlug}/projects/${projectId}/archives/issues/${restoreIssue.id}`,
        session
      );
      await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, restoreIssue.id);

      await test.step("no subscribe control renders and editing stays locked", async () => {
        expect(await driver.subscribeToggle()).toBeNull();
        expect(await driver.archivesDetailComposerVisible()).toBe(false);
        expect(await driver.archivesDetailReactionControlEnabled()).toBe(false);
        expect(await driver.layoutsDetailMenuItems()).toContain("Restore");
      });

      await test.step("a failed detail restore keeps the item archived with an error confirmation", async () => {
        await driver.archivesFailNextMutation("DELETE", `/issues/${restoreIssue.id}/archive/`);
        try {
          await driver.archivesDetailMenuChoose("Restore");
          await expect.poll(() => driver.sawToast("could not be restored"), { timeout: 60_000 }).toBe(true);
        } finally {
          await driver.archivesClearMutationFailure();
        }
        expect((await serverIssue(seed.workspaceSlug, projectId, restoreIssue.id, session)).archived_at).not.toBeNull();
      });

      await test.step("restore from detail navigates to the live work-item address", async () => {
        await driver.archivesDetailMenuChoose("Restore");
        await expect.poll(() => driver.sawToast("Restore success"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.currentUrlPath(), { timeout: 120_000 }).toContain("/browse/");
        expect((await serverIssue(seed.workspaceSlug, projectId, restoreIssue.id, session)).archived_at).toBeNull();
      });

      await test.step("a failed detail delete keeps the item archived with an error confirmation", async () => {
        await driver.openArchivedIssueDetail(seed.workspaceSlug, projectId, deleteIssue.id);
        await driver.archivesDetailMenuChoose("Delete");
        expect(await driver.layoutsDeleteModalVisible()).toBe(true);
        await driver.archivesFailNextMutation("DELETE", `/issues/${deleteIssue.id}/`);
        try {
          await driver.layoutsDeleteModalConfirm();
          await expect.poll(() => driver.sawToast("delete failed"), { timeout: 60_000 }).toBe(true);
        } finally {
          await driver.archivesClearMutationFailure();
        }
        expect((await issueStatus(seed.workspaceSlug, projectId, deleteIssue.id, session)).status).toBe(200);
      });

      await test.step("delete from detail navigates back to the archived list", async () => {
        await driver.archivesDetailMenuChoose("Delete");
        expect(await driver.layoutsDeleteModalVisible()).toBe(true);
        await driver.layoutsDeleteModalConfirm();
        await expect.poll(() => driver.sawToast("deleted successfully"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.currentUrlPath(), { timeout: 120_000 }).toMatch(/\/archives\/issues\/?$/);
        expect(await driver.currentUrlPath()).not.toContain(deleteIssue.id);
        expect((await issueStatus(seed.workspaceSlug, projectId, deleteIssue.id, session)).status).toBe(404);
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, restoreIssue.id, session).catch(() => {});
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, deleteIssue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, restoreIssue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, deleteIssue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ARCH-013"], "archive-a-live-item dialog with completed-or-canceled gating"),
  { tag: specTags(["ARCH-013"]) },
  async ({ driver, seed }) => {
    test.setTimeout(720_000);
    const tag = `NF223 archive ${Date.now()}`;
    const openName = `${tag} open`;
    const doneName = `${tag} done`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await makeProject(session, seed, tag, "A223A");
    const done = await doneState(seed.workspaceSlug, projectId, tag, session);
    const openIssue = await serverCreateIssueFull(seed.workspaceSlug, projectId, openName, session);
    const doneIssue = await serverCreateIssueFull(seed.workspaceSlug, projectId, doneName, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, doneIssue.id, { state_id: done.id }, session);
    try {
      const identifier = (await serverProject(seed.workspaceSlug, projectId, session)).identifier;
      const ref = await issueSeqForName(seed.workspaceSlug, projectId, identifier, doneName, session);
      // The dialog heading carries identifier and sequence as separate
      // words ("Archive Work item IDENT SEQ"), not the dashed reference.
      const dialogRef = `${identifier} ${ref.seq.split("-").pop() ?? ""}`;

      await openPath(driver, `/${seed.workspaceSlug}/projects/${projectId}/issues`, session);
      await driver.openProjectIssues(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.visibleIssueNames(), { timeout: 300_000 }).toContain(openName);

      await test.step("the archive entry stays disabled with an explanation off the done states", async () => {
        expect(await driver.layoutsRowMenuItemDisabled(openName, "Archive")).toBe(true);
        const note = await driver.layoutsRowMenuItemNote(openName, "Archive");
        expect(note).not.toBeNull();
        expect(note as string).toMatch(/completed|canceled/i);
      });

      await test.step("the archive entry enables on a completed item", async () => {
        expect(await driver.layoutsRowMenuItemDisabled(doneName, "Archive")).toBe(false);
      });

      await test.step("canceling the dialog changes nothing", async () => {
        await driver.layoutsRowMenuChoose(doneName, "Archive");
        expect(await driver.layoutsArchiveModalVisible()).toBe(true);
        expect(await driver.archivesArchiveModalTitle()).toContain(dialogRef);
        expect(await driver.archivesArchiveModalBody()).toMatch(/restor/i);
        await driver.archivesArchiveModalCancel();
        expect(await driver.layoutsArchiveModalVisible()).toBe(false);
        expect((await serverIssue(seed.workspaceSlug, projectId, doneIssue.id, session)).archived_at).toBeNull();
        expect(await driver.visibleIssueNames()).toContain(doneName);
      });

      await test.step("a failed archive keeps the item live with an error confirmation", async () => {
        await driver.layoutsRowMenuChoose(doneName, "Archive");
        expect(await driver.layoutsArchiveModalVisible()).toBe(true);
        await driver.archivesFailNextMutation("POST", `/issues/${doneIssue.id}/archive/`);
        try {
          await driver.confirmArchive();
          await expect.poll(() => driver.sawToast("could not be archived"), { timeout: 60_000 }).toBe(true);
        } finally {
          await driver.archivesClearMutationFailure();
        }
        // The dialog stays open on failure; dismiss it before moving on.
        expect(await driver.layoutsArchiveModalVisible()).toBe(true);
        await driver.archivesArchiveModalCancel();
        expect((await serverIssue(seed.workspaceSlug, projectId, doneIssue.id, session)).archived_at).toBeNull();
      });

      await test.step("confirming archives the item into the archives tab", async () => {
        await driver.layoutsRowMenuChoose(doneName, "Archive");
        expect(await driver.layoutsArchiveModalVisible()).toBe(true);
        await driver.confirmArchive();
        await expect.poll(() => driver.sawToast("Archive success"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.visibleIssueNames(), { timeout: 120_000 }).not.toContain(doneName);
        expect((await serverIssue(seed.workspaceSlug, projectId, doneIssue.id, session)).archived_at).not.toBeNull();

        await driver.archivesOpenList(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.archivesVisibleIssueNames(), { timeout: 300_000 }).toContain(doneName);
        expect((await serverArchivedIssues(seed.workspaceSlug, projectId, session)).map((r) => r.id)).toContain(
          doneIssue.id
        );
      });
    } finally {
      await serverUnarchiveIssue(seed.workspaceSlug, projectId, doneIssue.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, openIssue.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, doneIssue.id, session);
      await deleteOwnedState(seed.workspaceSlug, projectId, done.owned, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
