// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// web_new-only scenarios for nested pages (D17). The old app cannot show
// nested pages, so every test skips on the oracle leg with its row's D17
// reason and runs only against apps/web_new. Rows: PAGE-084–PAGE-090.
// Conventions: one file (parity/specs/pages.md §12.3), titles and tags name
// the rows, each scenario seeds and removes its own scratch project, and
// only the driver plus API helpers are driven.
import { test, expect } from "../fixtures";
import type { ParitySeedFacts } from "../drivers/parity-driver";
import {
  parityProjectIdentifier,
  serverArchivePage,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverDeletePage,
  serverEnsureProjectMember,
  serverFavoritePage,
  serverListChildPages,
  serverListPages,
  serverPageRecord,
  serverPageRequestStatus,
  serverPatchPageParent,
  serverSetPageAccess,
  signInSessionWithRetry,
  type PagesListServerRow,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF850 ${area} ${Date.now()}`;
}

/** Idempotent setup writes retry: the API 400/401s transiently under stack load. */
async function setupRetry<T>(label: string, fn: () => Promise<T>, attempts = 4): Promise<T> {
  let error: unknown = null;
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      return await fn();
    } catch (caught) {
      error = caught;
      await new Promise((resolve) => setTimeout(resolve, 2_000 * attempt));
    }
  }
  throw new Error(
    `[parity] setup step never landed (${label}): ${error instanceof Error ? error.message : String(error)}`
  );
}

async function scratchProject(seed: ParitySeedFacts, area: string, session: string): Promise<string> {
  return setupRetry(
    `create project ${area}`,
    () =>
      serverCreateProjectWithFlags(
        seed.workspaceSlug,
        `${tag(area)} project`,
        parityProjectIdentifier("N850"),
        {},
        session
      ),
    6
  );
}

async function cleanupProject(seed: ParitySeedFacts, projectId: string, session: string): Promise<void> {
  await Promise.race([
    serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, 30_000)),
  ]);
}

/** Second seeded member, used where visibility must differ from the owner. */
function requireMentionMember(seed: ParitySeedFacts): { email: string; password: string; displayName: string } {
  if (seed.mentionMember === undefined) {
    throw new Error("[parity] seed facts carry no mention member; rerun parity-up.sh.");
  }
  return {
    email: seed.mentionMember.email,
    password: seed.mentionMember.password,
    displayName: seed.mentionMember.displayName,
  };
}

/** Poll a page's parent link until the server settles it (drag writes race a bare read). */
async function waitForServerParent(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  expected: string | null
): Promise<void> {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, pageId, session);
    if (record.parentId === expected) return;
    if (Date.now() > deadline) {
      throw new Error(`[parity] parent of ${pageId} never settled to ${String(expected)}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

/**
 * The nested surface lands with PDASHOSS01-270 (§11.7 detection): create a
 * page, read it back, and require the two nested fields. A backend that has
 * not landed cannot answer them, so the scenario skips with the reason.
 */
async function requireNestedBackend(seed: ParitySeedFacts, projectId: string, session: string): Promise<boolean> {
  const probeId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${tag("probe")} page`);
  try {
    const record = await serverPageRecord(seed.workspaceSlug, projectId, probeId, session).catch(() => null);
    if (record === null) return false;
    return record.parentId !== undefined && record.subPagesCount !== undefined;
  } finally {
    // The exact row assertions below must never see the probe. Deleting an
    // archived-only page is the same two-step the UI performs, and a failed
    // cleanup must not mask the detection result.
    await serverDeletePage(seed.workspaceSlug, projectId, probeId, session).catch(() => {});
  }
}

function titleOf(row: PagesListServerRow): string {
  return row.name.trim() === "" ? "Untitled" : row.name;
}

test(
  specTitle(["PAGE-084"], "Add sub-page creates a child under the source and opens it"),
  { tag: specTags(["PAGE-084"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-084 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "addsubpage", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("parent")} handbook`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);

      // The row menu is one of the two entries the row offers (PAGE-084).
      await driver.pagesNestedRowMenuOpen(parentName);
      expect(await driver.pagesNestedOptionsItems()).toContain("Add sub-page");
      await driver.pagesNestedOptionsPick("Add sub-page");

      // The new child opens: its detail header paints and the URL names a
      // page other than the source. The record proves the parent link.
      await driver.pagesNestedWaitDetail();
      const rows = await setupRetry("list created child", () =>
        serverListChildPages(seed.workspaceSlug, projectId, parentId, session)
      );
      const child = rows[0];
      expect(child).toBeDefined();
      expect(titleOf(child as PagesListServerRow)).toBe("Untitled");
      const childRecord = await serverPageRecord(seed.workspaceSlug, projectId, child?.id as string, session);
      expect(childRecord.parentId).toBe(parentId);
      expect(await driver.currentUrlPath()).toBe(
        `/${seed.workspaceSlug}/projects/${projectId}/pages/${child?.id as string}`
      );

      // Back on the list the parent's count reflects the child.
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesNestedChildCount(parentName)).toBe(1);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-084"], "the detail header offers Add sub-page and opens the child"),
  { tag: specTags(["PAGE-084"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-084 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "headersubpage", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("header")} parent`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenPage(seed.workspaceSlug, projectId, parentId);
      await driver.pagesNestedOptionsOpen();
      expect(await driver.pagesNestedOptionsItems()).toContain("Add sub-page");
      await driver.pagesNestedOptionsPick("Add sub-page");
      await driver.pagesNestedWaitDetail();
      // The source detail's header is already on screen when the menu is
      // opened, so the header wait alone can settle on the parent. Wait for
      // the route to name the new page before reading it back.
      const sourcePath = `/${seed.workspaceSlug}/projects/${projectId}/pages/${parentId}`;
      await expect.poll(() => driver.currentUrlPath(), { timeout: 30_000 }).not.toBe(sourcePath);
      const rows = await setupRetry("list header child", () =>
        serverListChildPages(seed.workspaceSlug, projectId, parentId, session)
      );
      const child = rows[0];
      expect(child).toBeDefined();
      expect((await serverPageRecord(seed.workspaceSlug, projectId, child?.id as string, session)).parentId).toBe(
        parentId
      );
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-085"], "pages with children show a count, and expanding indents them recursively"),
  { tag: specTags(["PAGE-085"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-085 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "tree", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("tree")} parent`;
      const childName = `${tag("tree")} child`;
      const grandchildName = `${tag("tree")} grandchild`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      const grandchildId = await serverCreatePage(seed.workspaceSlug, projectId, session, grandchildName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      await serverPatchPageParent(seed.workspaceSlug, projectId, grandchildId, childId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesNestedChildCount(parentName)).toBe(1);
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName]);

      await driver.pagesNestedExpand(parentName);
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName, childName]);
      expect(await driver.pagesNestedDepth(childName)).toBe(1);

      // The second level expands the same way, one lazy read at a time.
      await driver.pagesNestedExpand(childName);
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName, childName, grandchildName]);
      expect(await driver.pagesNestedDepth(grandchildName)).toBe(2);

      // Expansion survives a client-side route change and back (PAGE-085).
      await driver.pagesNestedOpenRow(grandchildName);
      await driver.pagesNestedBack();
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName, childName, grandchildName]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-085"], "a page the viewer cannot see never appears, and counts exclude it"),
  { tag: specTags(["PAGE-085"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-085 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "hiddenchild", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const member = requireMentionMember(seed);
      const parentName = `${tag("hidden")} parent`;
      const visibleName = `${tag("hidden")} visible`;
      const hiddenName = `${tag("hidden")} private`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const visibleId = await serverCreatePage(seed.workspaceSlug, projectId, session, visibleName);
      const hiddenId = await serverCreatePage(seed.workspaceSlug, projectId, session, hiddenName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, visibleId, parentId, session);
      await serverSetPageAccess(seed.workspaceSlug, projectId, hiddenId, 1, session);
      await serverPatchPageParent(seed.workspaceSlug, projectId, hiddenId, parentId, session);
      await setupRetry("add member", () =>
        serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session)
      );

      await driver.openEntry();
      await driver.signInWithPassword(member.email, member.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      // Owner sees both; the member's count excludes the private child.
      expect(await driver.pagesNestedChildCount(parentName)).toBe(1);
      await driver.pagesNestedExpand(parentName);
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName, visibleName]);
      expect(await driver.pagesNestedRowTitles()).not.toContain(hiddenName);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-086"], "a child opens by URL with clickable ancestor breadcrumbs"),
  { tag: specTags(["PAGE-086"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-086 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "breadcrumbs", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const topName = `${tag("crumb")} handbook`;
      const middleName = `${tag("crumb")} policies`;
      const leafName = `${tag("crumb")} leave`;
      const topId = await serverCreatePage(seed.workspaceSlug, projectId, session, topName);
      const middleId = await serverCreatePage(seed.workspaceSlug, projectId, session, middleName);
      const leafId = await serverCreatePage(seed.workspaceSlug, projectId, session, leafName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, middleId, topId, session);
      await serverPatchPageParent(seed.workspaceSlug, projectId, leafId, middleId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenPage(seed.workspaceSlug, projectId, leafId);
      expect(await driver.pagesNestedBreadcrumbs()).toEqual([topName, middleName]);
      expect(await driver.pagesNestedBreadcrumbLinked(topName)).toBe(true);
      expect(await driver.pagesNestedBreadcrumbLinked(middleName)).toBe(true);

      // Following an ancestor crumb is a real navigation to that page.
      await driver.pagesNestedBreadcrumbClick(middleName);
      expect(await driver.currentUrlPath()).toBe(`/${seed.workspaceSlug}/projects/${projectId}/pages/${middleId}`);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-086"], "an invisible ancestor renders as Restricted page with no link"),
  { tag: specTags(["PAGE-086"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-086 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "restrictedcrumb", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const member = requireMentionMember(seed);
      const parentName = `${tag("restricted")} parent`;
      const childName = `${tag("restricted")} child`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      await serverSetPageAccess(seed.workspaceSlug, projectId, parentId, 1, session);
      await setupRetry("add member", () =>
        serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session)
      );

      await driver.openEntry();
      await driver.signInWithPassword(member.email, member.password);
      await driver.pagesNestedOpenPage(seed.workspaceSlug, projectId, childId);
      expect(await driver.pagesNestedBreadcrumbs()).toEqual(["Restricted page"]);
      expect(await driver.pagesNestedBreadcrumbLinked("Restricted page")).toBe(false);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-086"], "breadcrumbs resolve an archived middle ancestor under a live parent"),
  { tag: specTags(["PAGE-086"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-086 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "archivedcrumb", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const topName = `${tag("midcrumb")} handbook`;
      const middleName = `${tag("midcrumb")} policies`;
      const leafName = `${tag("midcrumb")} leave`;
      const topId = await serverCreatePage(seed.workspaceSlug, projectId, session, topName);
      const middleId = await serverCreatePage(seed.workspaceSlug, projectId, session, middleName);
      const leafId = await serverCreatePage(seed.workspaceSlug, projectId, session, leafName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, middleId, topId, session);
      await serverPatchPageParent(seed.workspaceSlug, projectId, leafId, middleId, session);
      // Archiving the middle cascades to the leaf; the top stays live and
      // reports no live children, which must not hide the middle (§11.7).
      await serverArchivePage(seed.workspaceSlug, projectId, middleId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenPage(seed.workspaceSlug, projectId, leafId);
      expect(await driver.pagesNestedBreadcrumbs()).toEqual([topName, middleName]);
      expect(await driver.pagesNestedBreadcrumbLinked(topName)).toBe(true);
      expect(await driver.pagesNestedBreadcrumbLinked(middleName)).toBe(true);

      await driver.pagesNestedBreadcrumbClick(middleName);
      expect(await driver.currentUrlPath()).toBe(`/${seed.workspaceSlug}/projects/${projectId}/pages/${middleId}`);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-087"], "Move to… puts a page and its subtree under another page or the top level"),
  { tag: specTags(["PAGE-087"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-087 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "movepicker", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const targetName = `${tag("move")} target`;
      const movingName = `${tag("move")} moving`;
      const childName = `${tag("move")} child`;
      const targetId = await serverCreatePage(seed.workspaceSlug, projectId, session, targetName);
      const movingId = await serverCreatePage(seed.workspaceSlug, projectId, session, movingName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, movingId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedMovePickerOpen(movingName);
      // Children are offered too, not only the top-level collection.
      expect((await driver.pagesNestedMoveTargets()).join("|")).toContain(targetName);
      await driver.pagesNestedMovePick(targetName);
      await driver.pagesNestedMoveConfirm();

      expect((await serverPageRecord(seed.workspaceSlug, projectId, movingId, session)).parentId).toBe(targetId);
      // The whole subtree follows the move.
      expect((await serverPageRecord(seed.workspaceSlug, projectId, childId, session)).parentId).toBe(movingId);

      // Back to the top level through the picker's top-level row. The
      // reload collapses the tree, so the moved row needs its parent
      // expanded before its menu can open.
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedExpand(targetName);
      await driver.pagesNestedMovePickerOpen(movingName);
      await driver.pagesNestedMovePick(null);
      await driver.pagesNestedMoveConfirm();
      expect((await serverPageRecord(seed.workspaceSlug, projectId, movingId, session)).parentId).toBeNull();
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-087"], "moving under itself, a descendant or an archived page is refused"),
  { tag: specTags(["PAGE-087"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-087 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "moverefuse", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const topName = `${tag("refuse")} top`;
      const middleName = `${tag("refuse")} middle`;
      const shelvedName = `${tag("refuse")} shelved`;
      const topId = await serverCreatePage(seed.workspaceSlug, projectId, session, topName);
      const middleId = await serverCreatePage(seed.workspaceSlug, projectId, session, middleName);
      const shelvedId = await serverCreatePage(seed.workspaceSlug, projectId, session, shelvedName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, middleId, topId, session);
      await serverArchivePage(seed.workspaceSlug, projectId, shelvedId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedMovePickerOpen(topName);
      // The picker never offers the page itself, its subtree or an archived
      // page; a stale target still gets the server's refusal.
      const targets = (await driver.pagesNestedMoveTargets()).join("|");
      expect(targets).not.toContain(middleName);
      expect(targets).not.toContain(shelvedName);
      expect(targets).not.toContain(topName);
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${topId}/`, session, { parent: middleId })).toBe(
        400
      );
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${topId}/`, session, { parent: shelvedId })).toBe(
        400
      );

      // The visible UI refuses the same illegal drops with a clear notice,
      // before any write reaches the server (PAGE-087).
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedExpand(topName);
      await driver.pagesNestedDragRow(topName, topName);
      await driver.pagesNestedWaitToast("A page cannot be moved under itself.");
      await driver.pagesNestedDragRow(topName, middleName);
      await driver.pagesNestedWaitToast("A page cannot be moved under one of its own sub-pages.");

      // An archived destination is refused too: once both pages are shelved,
      // dragging one archived row onto another cannot nest it.
      await serverArchivePage(seed.workspaceSlug, projectId, topId, session);
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      await driver.pagesNestedDragRow(shelvedName, topName);
      await driver.pagesNestedWaitToast("A page cannot be moved under an archived page.");
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(
    ["PAGE-087"],
    "dragging a row onto another page reparents it, and onto the tree returns it to the top level"
  ),
  { tag: specTags(["PAGE-087"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-087 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "movedrag", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const targetName = `${tag("drag")} target`;
      const movingName = `${tag("drag")} moving`;
      const targetId = await serverCreatePage(seed.workspaceSlug, projectId, session, targetName);
      const movingId = await serverCreatePage(seed.workspaceSlug, projectId, session, movingName);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedDragRow(movingName, targetName);
      // The notice fires on the write's success, so the server assert below
      // cannot race the PATCH.
      await driver.pagesNestedWaitToast("Page moved.");
      expect(await driver.pagesNestedToastText()).toContain("Page moved.");
      expect((await serverPageRecord(seed.workspaceSlug, projectId, movingId, session)).parentId).toBe(targetId);

      await driver.pagesNestedExpand(targetName);
      await driver.pagesNestedDragRow(movingName, null);
      // The first notice may still be visible, so the top-level write is
      // awaited on the server state instead of a second toast.
      await waitForServerParent(seed, projectId, movingId, session, null);
      expect((await serverPageRecord(seed.workspaceSlug, projectId, movingId, session)).parentId).toBeNull();
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-088"], "archiving a page cascades to its descendants"),
  { tag: specTags(["PAGE-088"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-088 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "archivecascade", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("archive")} parent`;
      const childName = `${tag("archive")} child`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedRowMenuOpen(parentName);
      await driver.pagesNestedOptionsPick("Archive");
      await driver.pagesNestedWaitToast("Page archived successfully.");

      expect((await serverPageRecord(seed.workspaceSlug, projectId, childId, session)).archivedAt).not.toBeNull();
      // The whole subtree shows in the archived tab, parent above child. The
      // contract count names live children only, so a cascaded archive has
      // no count until the shelved subtree is read through the expand.
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      await driver.pagesNestedExpand(parentName);
      expect(await driver.pagesNestedRowTitles()).toEqual([parentName, childName]);
      expect(await driver.pagesNestedChildCount(parentName)).toBe(1);
      // The top-level archived read answers roots only; a shelved child is
      // reached through its parent's children read, which keeps the archive
      // stamp and so shows up when archived rows are asked for.
      const archivedChildren = await serverListChildPages(seed.workspaceSlug, projectId, parentId, session, true);
      expect(archivedChildren.map(titleOf)).toEqual([childName]);
      expect(archivedChildren[0]?.archivedAt).not.toBeNull();
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-088"], "restoring under an archived parent detaches to the top level with a notice"),
  { tag: specTags(["PAGE-088"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-088 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "restoretop", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("restore")} parent`;
      const childName = `${tag("restore")} child`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      // The cascade leaves both the parent and child shelved; restoring the
      // child under the still-shelved parent is the detach case.
      await serverArchivePage(seed.workspaceSlug, projectId, parentId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      // Restoring the child while the parent is still shelved detaches it.
      await driver.pagesNestedExpand(parentName);
      await driver.pagesNestedRowMenuOpen(childName);
      await driver.pagesNestedOptionsPick("Restore");
      await driver.pagesNestedWaitToast("Page restored to the top level.");
      expect((await serverPageRecord(seed.workspaceSlug, projectId, childId, session)).parentId).toBeNull();
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-089"], "deleting a page keeps its children at the top level and names the count"),
  { tag: specTags(["PAGE-089"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-089 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "deleterehome", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("delete")} parent`;
      const childName = `${tag("delete")} child`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      await serverArchivePage(seed.workspaceSlug, projectId, parentId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      // Delete is offered on the archived row as it renders, collapsed: the
      // confirmation must resolve the direct children itself (PAGE-089).
      await driver.pagesNestedRowMenuOpen(parentName);
      await driver.pagesNestedOptionsPick("Delete");
      expect(await driver.pagesNestedDeleteDialogText()).toContain("1 sub-page will move to the top level.");
      await driver.pagesNestedDeleteConfirm();
      await driver.pagesNestedWaitToast("Page deleted successfully.");

      // The child survives, detached to the top level and otherwise intact.
      const child = await serverPageRecord(seed.workspaceSlug, projectId, childId, session);
      expect(child.parentId).toBeNull();
      expect(child.name).toBe(childName);
      const archived = await serverListPages(seed.workspaceSlug, projectId, session, true);
      expect(archived.map(titleOf)).toEqual([childName]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-090"], "search, favourites and the palette find child pages with their parent path"),
  { tag: specTags(["PAGE-090"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-090 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "searchchildren", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      const parentName = `${tag("search")} handbook`;
      const childName = `${tag("search")} leave policy`;
      const otherName = `${tag("search")} payroll`;
      const member = requireMentionMember(seed);
      await setupRetry("add member", () =>
        serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session)
      );
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      // The child is owned by the second member so a Created-by filter can
      // isolate it while the parent stays collapsed (PAGE-090 filters).
      const memberSession = await signInSessionWithRetry(member.email, member.password);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, memberSession, childName);
      await serverCreatePage(seed.workspaceSlug, projectId, session, otherName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedSearch("leave");
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect(await driver.pagesNestedParentPath(childName)).toBe(parentName);
      await driver.pagesNestedSearchClear();

      // Favourites is one of the filters that must span the tree: a
      // favourited child shows outside its unexpanded parent.
      await serverFavoritePage(seed.workspaceSlug, projectId, childId, childName, session);
      await driver.pagesNestedOpenList(seed.workspaceSlug, projectId);
      await driver.pagesNestedFavoritesFilter();
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect(await driver.pagesNestedParentPath(childName)).toBe(parentName);

      // A structured Created-by filter spans the tree too: the member's
      // child appears outside its unexpanded parent with the parent path.
      await driver.pagesNestedCreatorFilter(member.displayName);
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect(await driver.pagesNestedParentPath(childName)).toBe(parentName);
      await driver.pagesNestedFiltersClear();

      // The palette jumps to a child and names its path.
      const palette = await driver.pagesNestedPaletteFind("leave");
      expect(palette.join("|")).toContain(childName);
      expect(palette.join("|")).toContain(parentName);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-090"], "search, filters and the palette find a shelved child under its parent path"),
  { tag: specTags(["PAGE-090"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-090 is new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "searcharchived", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      // A cascaded archive leaves the parent reporting no live children, so
      // discovery must walk it to reach the shelved child (§11.7).
      const parentName = `${tag("archivedsearch")} handbook`;
      const childName = `${tag("archivedsearch")} leave policy`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      await serverArchivePage(seed.workspaceSlug, projectId, parentId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      await driver.pagesNestedSearch("leave");
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect(await driver.pagesNestedParentPath(childName)).toBe(parentName);
      await driver.pagesNestedSearchClear();

      // The palette reads the same forest, so a shelved child is reachable
      // by name with its parent path even while the parent stays collapsed.
      const palette = await driver.pagesNestedPaletteFind("leave");
      expect(palette.join("|")).toContain(childName);
      expect(palette.join("|")).toContain(parentName);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-088", "PAGE-089", "PAGE-090"], "a shelved child under a live parent stays reachable"),
  { tag: specTags(["PAGE-088", "PAGE-089", "PAGE-090"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "PAGE-088/089/090 are new (D17); no oracle");
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "archivedliveparent", session);
    try {
      if (!(await requireNestedBackend(seed, projectId, session))) {
        testInfo.skip(true, "PDASHOSS01-270 has not landed on this stack");
        return;
      }
      // A live parent counts only live children, so its shelved child leaves
      // it reporting zero. Discovery must still reach the child, or restore
      // and delete are unreachable from the Archived tab (§11.7).
      const parentName = `${tag("liveparent")} handbook`;
      const childName = `${tag("liveparent")} leave policy`;
      const parentId = await serverCreatePage(seed.workspaceSlug, projectId, session, parentName);
      const childId = await serverCreatePage(seed.workspaceSlug, projectId, session, childName);
      await serverPatchPageParent(seed.workspaceSlug, projectId, childId, parentId, session);
      await serverArchivePage(seed.workspaceSlug, projectId, childId, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesNestedOpenArchivedList(seed.workspaceSlug, projectId);
      // The archived child is a root in the Archived tab; the live parent
      // never joins that tab.
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect((await serverPageRecord(seed.workspaceSlug, projectId, parentId, session)).archivedAt).toBeNull();

      // Search finds it with its parent path even though the parent stays live.
      await driver.pagesNestedSearch("leave");
      expect(await driver.pagesNestedRowTitles()).toEqual([childName]);
      expect(await driver.pagesNestedParentPath(childName)).toBe(parentName);
      await driver.pagesNestedSearchClear();

      // The palette reads the same forest.
      const palette = await driver.pagesNestedPaletteFind("leave");
      expect(palette.join("|")).toContain(childName);
      expect(palette.join("|")).toContain(parentName);

      // Restore is reachable on the discovered row; the live parent no longer
      // counts the child as archived once restored.
      await driver.pagesNestedRowMenuOpen(childName);
      await driver.pagesNestedOptionsPick("Restore");
      await driver.pagesNestedWaitToast("Page restored successfully.");
      expect((await serverPageRecord(seed.workspaceSlug, projectId, childId, session)).archivedAt).toBeNull();
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);
