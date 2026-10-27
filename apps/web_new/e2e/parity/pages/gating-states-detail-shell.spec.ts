// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-314): pages permission gating as control
// visibility, list loading/empty/error states, fetch and caching behavior,
// list URL semantics, and the detail shell (record load, error panel,
// breadcrumb cluster with sibling switcher, read gating, loading states,
// deep links). Rows: PAGE-018–PAGE-025, PAGE-079–PAGE-080.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  requireMentionMember,
  serverAddProjectMembers,
  serverArchivePage,
  serverCleanupProject,
  serverCreatePage,
  serverCreateProjectWithFlags,
  serverDeletePage,
  serverEnsureProjectGuest,
  serverFavoritePage,
  serverListPages,
  serverPageLock,
  serverPageRecord,
  serverPageRequestStatus,
  serverPageUnlock,
  serverPageVersionDetail,
  serverPageVersions,
  serverPatchProject,
  serverRenamePage,
  serverSetPageAccess,
  serverUpdatePageDescription,
  signInSessionWithRetry,
  type PagesListServerRow,
  type ParityPageVersion,
  type ParitySeedFacts,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { PagesGatingRequest } from "../drivers/parity-driver";

/** Project MEMBER role (the seed owner is admin; guests hold 5). */
const PROJECT_MEMBER_ROLE = 15;
/** Project ADMIN role for the non-owner admin edges. */
const PROJECT_ADMIN_ROLE = 20;

/** Unique area tag so scratch projects and page names never collide. */
function tag(area: string): string {
  return `NF314 ${area} ${Date.now()}`;
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

/** Scratch project for one scenario (deleted with the project in `finally`). */
async function scratchProject(seed: ParitySeedFacts, area: string, session: string): Promise<string> {
  // Area feeds the project name, which rejects special characters
  // (hyphens included): keep it to letters and digits.
  // Creation 400s transiently under stack load (identifier mint collisions
  // across parallel runs); a fresh mint retries with backoff.
  return setupRetry(
    `create project ${area}`,
    () =>
      serverCreateProjectWithFlags(
        seed.workspaceSlug,
        `${tag(area)} project`,
        parityProjectIdentifier("N314"),
        {},
        session
      ),
    6
  );
}

/** Best-effort cleanup that cannot hang the scenario past `timeoutMs`. */
async function cleanupProject(
  seed: ParitySeedFacts,
  projectId: string,
  session: string,
  timeoutMs = 30_000
): Promise<void> {
  await Promise.race([
    serverCleanupProject(seed.workspaceSlug, projectId, session).catch(() => {}),
    new Promise((resolve) => setTimeout(resolve, timeoutMs)),
  ]);
}

/** Display title the list shows for a server row (blank reads back untitled). */
function displayTitle(row: PagesListServerRow): string {
  return row.name.trim() === "" ? "Untitled" : row.name;
}

/**
 * Poll the version list until `count` snapshots persist (creation runs in
 * a background worker, so fixtures never assume it is synchronous).
 */
async function waitVersions(
  seed: ParitySeedFacts,
  projectId: string,
  pageId: string,
  session: string,
  count: number,
  timeoutMs = 150_000
): Promise<ParityPageVersion[]> {
  const started = Date.now();
  for (;;) {
    const rows = await serverPageVersions(seed.workspaceSlug, projectId, pageId, session);
    if (rows.length >= count) return rows;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[parity] versions stuck at ${rows.length}, wanted ${count}.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 3_000));
  }
}

/** Minimal TipTap document with one paragraph per text. */
function tiptapParagraphs(...texts: string[]): string {
  return JSON.stringify({
    type: "doc",
    content: texts.map((text) => ({
      type: "paragraph",
      content: text === "" ? [] : [{ type: "text", text }],
    })),
  });
}

/** Require the seed guest identity (plain error when the seed lacks one). */
function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (seed.guestEmail === undefined || seed.guestPassword === undefined) {
    throw new Error("[parity] seed facts carry no guest; re-run the stack seed step (see stack/README.md).");
  }
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/**
 * D17 (nested pages) added two entries the oracle predates: `Add sub-page`
 * (PAGE-084) and `Move to…` (PAGE-087). They are pinned new-feature rows,
 * not a change to the oracle's surface, so these matrix scenarios read the
 * oracle's exact list on both legs and expect the documented suffix only on
 * the web_new leg (the same leg split PAGE-083 already uses).
 */
const D17_ROW_SUFFIX = ["Add sub-page", "Move to…"];

/**
 * The same two entries follow their own gates, which the oracle rows it
 * shares a menu with never had to express: `Add sub-page` needs member and
 * up, `Move to…` needs the page's owner or an admin (PAGE-084/087). An
 * archived page takes only `Move to…`: a shelved page cannot parent a
 * sub-page, so the tree entry never appears there.
 */
const D17_ROW_MEMBER_SUFFIX = ["Add sub-page"];
const D17_ROW_ARCHIVED_SUFFIX = ["Move to…"];

/** A D17 detail-menu suffix: `Add sub-page` for members and above. */
const D17_DETAIL_ENTRY = "Add sub-page";

/** The detail menu's own pinned prefix, with the D17 entry after history. */
function withD17Detail(items: string[], leg: string): string[] {
  if (leg !== "web_new") return items;
  const history = items.indexOf("Version history");
  if (history < 0) return [...items, D17_DETAIL_ENTRY];
  return [...items.slice(0, history + 1), D17_DETAIL_ENTRY, ...items.slice(history + 1)];
}

function withD17(items: string[], suffix: readonly string[], leg: string): string[] {
  return leg === "web_new" ? [...items, ...suffix] : items;
}

/** True when the status reads as a successful mutation answer. */
function isOkStatus(status: number): boolean {
  return status === 200 || status === 201 || status === 204;
}

test(
  specTitle(["PAGE-018"], "owners see the full control surface on list and detail rows"),
  { tag: specTags(["PAGE-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "ownermatrix", session);
    try {
      const stamp = tag("own");
      const openName = `${stamp} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      const shelvedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} shelved`);
      await setupRetry("archive page", () => serverArchivePage(seed.workspaceSlug, projectId, shelvedId, session));

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingCreateOffered()).toBe(true);
      expect(await driver.pagesGatingRowStarOffered(openName)).toBe(true);
      expect(await driver.pagesGatingRowMenuItems(openName)).toEqual(
        withD17(
          ["Open in new tab", "Copy link", "Make a copy", "Lock", "Make private", "Archive"],
          D17_ROW_SUFFIX,
          driver.target
        )
      );
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, "?type=private");
      expect(await driver.pagesGatingRowMenuItems(`${stamp} private`)).toEqual(
        withD17(
          ["Open in new tab", "Copy link", "Make a copy", "Lock", "Make public", "Archive"],
          D17_ROW_SUFFIX,
          driver.target
        )
      );
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, "?type=archived");
      expect(await driver.pagesGatingRowMenuItems(`${stamp} shelved`)).toEqual(
        withD17(
          ["Open in new tab", "Copy link", "Make a copy", "Lock", "Restore", "Delete"],
          D17_ROW_ARCHIVED_SUFFIX,
          driver.target
        )
      );

      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingDetailCluster()).toMatchObject({ star: true, copyLink: true, menu: true });
      expect(await driver.pagesGatingDetailMenuItems()).toEqual(
        withD17Detail(
          [
            "Full width",
            "Sticky toolbar",
            "Copy markdown",
            "Version history",
            "Make a copy",
            "Archive",
            "Make private",
            "Export",
          ],
          driver.target
        )
      );

      // Server baselines: the owner passes every gate (exact codes pinned).
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(await serverPageRequestStatus("GET", `${base}/pages/${openId}/`, session)).toBe(200);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${privId}/`, session)).toBe(200);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/lock/`, session, {})).toBe(204);
      expect(await serverPageRequestStatus("DELETE", `${base}/pages/${openId}/lock/`, session)).toBe(204);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/archive/`, session, {})).toBe(200);
      expect(await serverPageRequestStatus("DELETE", `${base}/pages/${openId}/archive/`, session)).toBe(204);
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${openId}/`, session, { access: 1 })).toBe(200);
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${openId}/`, session, { access: 0 })).toBe(200);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/duplicate/`, session, {})).toBe(201);
      expect(await serverPageRequestStatus("POST", `${base}/pages/`, session, { name: `${stamp} extra` })).toBe(201);
      const rows = await serverListPages(seed.workspaceSlug, projectId, session, true);
      expect(rows.some((row) => row.id === openId && row.access === 0)).toBe(true);
      expect(rows.some((row) => row.id === privId && row.access === 1)).toBe(true);
      expect(rows.some((row) => row.id === shelvedId && row.archivedAt !== null)).toBe(true);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-018"], "members keep create, star, and duplicate while admin actions stay hidden"),
  { tag: specTags(["PAGE-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "membermatrix", session);
    try {
      const stamp = tag("mem");
      const openName = `${stamp} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );

      // Member description write lands before any UI opens the page: the
      // member's open editor session intermittently autosaves its stale
      // empty document over a later PATCH, wiping it. Early, the PATCH
      // proves the matrix's member-edit permission deterministically.
      const memberSession = await signInSessionWithRetry(mention.email, mention.password);
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(
        await serverPageRequestStatus("PATCH", `${base}/pages/${openId}/description/`, memberSession, {
          description_html: "<p>member edit</p>",
        })
      ).toBe(200);
      await expect
        .poll(async () => (await serverPageRecord(seed.workspaceSlug, projectId, openId, session)).descriptionHtml, {
          timeout: 60_000,
        })
        .toContain("member edit");

      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingCreateOffered()).toBe(true);
      expect(await driver.pagesGatingRowStarOffered(openName)).toBe(true);
      expect(await driver.pagesGatingRowMenuItems(openName)).toEqual(
        withD17(["Open in new tab", "Copy link", "Make a copy"], D17_ROW_MEMBER_SUFFIX, driver.target)
      );
      expect(await driver.pagesGatingRowTitles()).toEqual([openName]);

      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingDetailCluster()).toMatchObject({ star: true, copyLink: true, menu: true });
      expect(await driver.pagesGatingDetailMenuItems()).toEqual(
        withD17Detail(
          ["Full width", "Sticky toolbar", "Copy markdown", "Version history", "Make a copy", "Export"],
          driver.target
        )
      );

      // Server half of the matrix: reads and member actions pass while the
      // admin actions refuse (lock excepted — the server allows it although
      // the UI hides it). The description PATCH already landed before the UI
      // opened (see above); the end-of-test read re-checks the flags.
      expect(
        await serverPageRequestStatus("POST", `${base}/pages/`, memberSession, { name: `${stamp} by member` })
      ).toBe(201);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${openId}/`, memberSession)).toBe(200);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${privId}/`, memberSession)).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/lock/`, memberSession, {})).toBe(204);
      await serverPageUnlock(seed.workspaceSlug, projectId, openId, session);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/archive/`, memberSession, {})).toBe(400);
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${openId}/`, memberSession, { access: 1 })).toBe(
        400
      );
      expect(await serverPageRequestStatus("DELETE", `${base}/pages/${openId}/`, memberSession)).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/duplicate/`, memberSession, {})).toBe(201);
      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(rows.some((row) => row.name === `${stamp} by member`)).toBe(true);
      const record = await serverPageRecord(seed.workspaceSlug, projectId, openId, session);
      expect(record.isLocked).toBe(false);
      expect(record.access).toBe(0);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-018"], "guests see no rows and no creation, and direct links show the panel"),
  { tag: specTags(["PAGE-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const guest = requireGuest(seed);
    const projectId = await scratchProject(seed, "guestmatrix", session);
    try {
      const stamp = tag("gue");
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} open`);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      await setupRetry("add guest", () =>
        serverEnsureProjectGuest(seed.workspaceSlug, projectId, guest.email, session)
      );

      await driver.openEntry();
      await driver.signInWithPassword(guest.email, guest.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingListVariant()).toBe("docs-empty");
      expect(await driver.pagesGatingCreateOffered()).toBe(false);
      expect(await driver.pagesGatingRowTitles()).toEqual([]);
      // The empty-collection create action is the one gated control that
      // renders disabled instead of hiding.
      expect(await driver.pagesGatingEmptyCreateState()).toBe("disabled");

      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("panel");
      expect(await driver.pagesGatingEditableCount()).toBe(0);

      // Server half: the guest list reads back empty and every mutation
      // refuses; reads refuse too (open reads answer 400, restricted 403).
      const guestSession = await signInSessionWithRetry(guest.email, guest.password);
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(await serverPageRequestStatus("GET", `${base}/pages/`, guestSession)).toBe(200);
      expect((await serverListPages(seed.workspaceSlug, projectId, guestSession)).length).toBe(0);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${openId}/`, guestSession)).toBe(400);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${privId}/`, guestSession)).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/`, guestSession, { name: "guest try" })).toBe(403);
      expect(
        await serverPageRequestStatus("PATCH", `${base}/pages/${openId}/`, guestSession, { name: "guest try" })
      ).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/lock/`, guestSession, {})).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/archive/`, guestSession, {})).toBe(403);
      expect(await serverPageRequestStatus("DELETE", `${base}/pages/${openId}/`, guestSession)).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${openId}/duplicate/`, guestSession, {})).toBe(403);
      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(rows.length).toBe(2);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-018"], "admins without ownership match owners on open pages but not on restricted ones"),
  { tag: specTags(["PAGE-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "adminmatrix", session);
    try {
      const stamp = tag("adm");
      const openName = `${stamp} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      await setupRetry("add admin", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_ADMIN_ROLE }],
          session
        )
      );

      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingCreateOffered()).toBe(true);
      expect(await driver.pagesGatingRowStarOffered(openName)).toBe(true);
      expect(await driver.pagesGatingRowMenuItems(openName)).toEqual(
        withD17(
          ["Open in new tab", "Copy link", "Make a copy", "Lock", "Make private", "Archive"],
          D17_ROW_SUFFIX,
          driver.target
        )
      );
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, "?type=private");
      expect(await driver.pagesGatingRowTitles()).toEqual([]);

      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingDetailCluster()).toMatchObject({ star: true, copyLink: true, menu: true });
      await expect.poll(async () => driver.pagesGatingEditableCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, privId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("panel");
      expect(await driver.pagesGatingEditableCount()).toBe(0);

      // Server half: restricted records refuse the non-owner admin outright.
      const adminSession = await signInSessionWithRetry(mention.email, mention.password);
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(await serverPageRequestStatus("GET", `${base}/pages/${openId}/`, adminSession)).toBe(200);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${privId}/`, adminSession)).toBe(403);
      expect(await serverPageRequestStatus("POST", `${base}/pages/${privId}/lock/`, adminSession, {})).toBe(403);
      expect(await serverPageRequestStatus("PATCH", `${base}/pages/${privId}/`, adminSession, { access: 0 })).toBe(403);
      expect(await serverPageRequestStatus("DELETE", `${base}/pages/${privId}/`, adminSession)).toBe(403);
      const memberRows = await serverListPages(seed.workspaceSlug, projectId, adminSession);
      expect(memberRows.map((row) => row.name)).toEqual([openName]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-018"], "page moves offer no control and change nothing from the API"),
  { tag: specTags(["PAGE-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "movematrix", session);
    const targetId = await scratchProject(seed, "movetarget", session);
    try {
      const openName = `${tag("mov")} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      // This row pins the absence of the licensed cross-project move. D17
      // added a different capability — moving a page inside its project's
      // tree — so the web_new leg reads that entry as the documented
      // PAGE-087 suffix and the oracle leg still carries no move at all.
      const withoutTreeMove = (items: string[]): string[] =>
        driver.target === "web_new" ? items.filter((item) => item !== "Move to…") : items;
      const rowItems = await driver.pagesGatingRowMenuItems(openName);
      expect(withoutTreeMove(rowItems).some((item) => item.toLowerCase().includes("move"))).toBe(false);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      const detailItems = await driver.pagesGatingDetailMenuItems();
      expect(withoutTreeMove(detailItems).some((item) => item.toLowerCase().includes("move"))).toBe(false);

      // The community tree carries no move affordance; the endpoint call
      // leaves the page where it is (the exact refusal is recorded, not
      // asserted, since licensed builds may answer differently).
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      const moveStatus = await serverPageRequestStatus("POST", `${base}/pages/${openId}/move/`, session, {
        new_project_id: targetId,
      });
      expect(isOkStatus(moveStatus)).toBe(false);
      expect((await serverListPages(seed.workspaceSlug, projectId, session)).some((row) => row.id === openId)).toBe(
        true
      );
      expect((await serverListPages(seed.workspaceSlug, targetId, session)).length).toBe(0);
    } finally {
      await cleanupProject(seed, projectId, session);
      await cleanupProject(seed, targetId, session);
    }
  }
);

test(
  specTitle(["PAGE-019"], "first load skeletons while the collection fetch is held, then rows settle"),
  { tag: specTags(["PAGE-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "skeleton", session);
    try {
      const name = `${tag("skel")} page`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, name);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingStallListFetch();
      try {
        // Mount-only open: the held fetch never settles while stalled.
        await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, undefined, false);
        await expect.poll(async () => driver.pagesGatingSkeletonCount(), { timeout: 60_000 }).toBeGreaterThan(0);
        expect(await driver.pagesGatingListVariant()).toBe("loading");
        expect(await driver.pagesGatingRowTitles()).toEqual([]);
      } finally {
        await driver.pagesGatingReleaseListFetch();
      }
      await expect.poll(async () => driver.pagesGatingRowTitles(), { timeout: 60_000 }).toEqual([name]);
      expect(await driver.pagesGatingSkeletonCount()).toBe(0);
      expect(await driver.pagesGatingListVariant()).toBe("rows");

      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(rows.map(displayTitle)).toEqual([name]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-019"], "empty variants differ per case with role-gated creation"),
  { tag: specTags(["PAGE-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "empties", session);
    try {
      const name = `${tag("empty")} page`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, name);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      // Docs-flavored empty with an enabled create action for the owner.
      const bareId = await scratchProject(seed, "bareempty", session);
      await driver.pagesGatingOpenList(seed.workspaceSlug, bareId);
      expect(await driver.pagesGatingListVariant()).toBe("docs-empty");
      expect(await driver.pagesGatingEmptyCreateState()).toBe("enabled");
      expect((await serverListPages(seed.workspaceSlug, bareId, session)).length).toBe(0);

      // No-action shelved variant once pages exist elsewhere.
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, "?type=archived");
      expect(await driver.pagesGatingListVariant()).toBe("archived-empty");
      expect(await driver.pagesGatingEmptyCreateState()).toBe("absent");

      // No-results variant for filters and search.
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      await driver.pagesGatingSearchType("zzz-no-such-page");
      expect(await driver.pagesGatingListVariant()).toBe("no-results");
      expect(await driver.pagesGatingRowTitles()).toEqual([]);

      // Feature-off variant replaces the whole list.
      await setupRetry("patch page_view off", () =>
        serverPatchProject(seed.workspaceSlug, projectId, session, { page_view: false })
      );
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingListVariant()).toBe("gate");
      expect(await driver.pagesGatingRowTitles()).toEqual([]);
      // The switch gates the UI only: the API still serves the pages.
      expect((await serverListPages(seed.workspaceSlug, projectId, session)).length).toBe(1);
      await cleanupProject(seed, bareId, session);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-019"], "collection failures fall through to empty content, never a blank panel"),
  { tag: specTags(["PAGE-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "listfail", session);
    try {
      const name = `${tag("fail")} page`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, name);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingFailListFetch();
      try {
        await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
        // The store holds nothing, so the failure lands on the
        // docs-flavored empty with its create action — a full variant,
        // not a blank panel.
        await expect.poll(async () => driver.pagesGatingListVariant(), { timeout: 60_000 }).toBe("docs-empty");
        expect(await driver.pagesGatingRowTitles()).toEqual([]);
        expect(await driver.pagesGatingEmptyCreateState()).toBe("enabled");
      } finally {
        await driver.pagesGatingUnfailListFetch();
      }
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      await expect.poll(async () => driver.pagesGatingRowTitles(), { timeout: 60_000 }).toEqual([name]);

      expect((await serverListPages(seed.workspaceSlug, projectId, session)).map(displayTitle)).toEqual([name]);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-019"], "failed creation reports through a notice and stays on the list"),
  { tag: specTags(["PAGE-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "createfail", session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingListVariant()).toBe("docs-empty");

      await driver.pagesGatingFailListCreates();
      try {
        await driver.pagesGatingClickCreate();
        await expect
          .poll(async () => (await driver.pagesGatingToastTexts()).join(" | "), { timeout: 60_000 })
          .toContain("could not be created");
        expect(await driver.pagesGatingCurrentUrl()).toContain(`/projects/${projectId}/pages`);
        expect(await driver.pagesGatingRowTitles()).toEqual([]);
      } finally {
        await driver.pagesGatingUnfailListCreates();
      }
      expect((await serverListPages(seed.workspaceSlug, projectId, session)).length).toBe(0);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-020"], "one cached collection read feeds every tab and survives refetch"),
  { tag: specTags(["PAGE-020"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "fetchcache", session);
    try {
      const stamp = tag("fet");
      const openName = `${stamp} open`;
      await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      const shelvedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} shelved`);
      await setupRetry("archive page", () => serverArchivePage(seed.workspaceSlug, projectId, shelvedId, session));

      const collectionGets = async (): Promise<PagesGatingRequest[]> =>
        (await driver.pagesGatingLoggedRequests()).filter(
          // Child reads (`?parent=`) are the nested tree's own requests; the
          // one-collection pin is about the top-level list read (D17).
          (entry) => entry.method === "GET" && /\/pages\/$/.test(entry.path) && !entry.query.includes("parent=")
        );

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingStartRequestSpy();
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      expect(await driver.pagesGatingRowTitles()).toEqual([openName]);
      expect((await collectionGets()).length).toBe(1);

      // Tab switches partition client-side: no further collection reads.
      await driver.pagesGatingClickTab("private");
      expect(await driver.pagesGatingRowTitles()).toEqual([`${stamp} private`]);
      await driver.pagesGatingClickTab("archived");
      expect(await driver.pagesGatingRowTitles()).toEqual([`${stamp} shelved`]);
      await driver.pagesGatingClickTab("public");
      expect(await driver.pagesGatingRowTitles()).toEqual([openName]);
      expect((await collectionGets()).length).toBe(1);
      const dedicated = (await driver.pagesGatingLoggedRequests()).filter(
        (entry) => entry.path.includes("favorite-pages") || entry.path.includes("archived-pages")
      );
      expect(dedicated).toEqual([]);

      // Past the client dedupe window a back-navigation revalidates once,
      // and the cached rows stay up under a non-blocking refetch (a full
      // reload would drop the in-memory cache, so this half navigates
      // client-side: row click out, back button in).
      await driver.pagesGatingOpenRow(openName);
      await new Promise((resolve) => setTimeout(resolve, 3_000));
      await driver.pagesGatingResetRequestSpy();
      await driver.pagesGatingStallListFetch();
      try {
        await driver.pagesGatingGoBack();
        await expect.poll(async () => (await collectionGets()).length, { timeout: 60_000 }).toBe(1);
        await new Promise((resolve) => setTimeout(resolve, 2_000));
        expect(await driver.pagesGatingRowTitles()).toEqual([openName]);
        expect(await driver.pagesGatingListVariant()).toBe("rows");
      } finally {
        await driver.pagesGatingReleaseListFetch();
      }
      await expect.poll(async () => driver.pagesGatingRowTitles(), { timeout: 60_000 }).toEqual([openName]);
      expect((await collectionGets()).length).toBe(1);

      // Server half: one read serves every tab, and the dedicated
      // favorites/shelved endpoints do not exist (the UI never calls them).
      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      const rows = await serverListPages(seed.workspaceSlug, projectId, session, true);
      expect(rows.length).toBe(3);
      expect(await serverPageRequestStatus("GET", `${base}/favorite-pages/`, session)).toBe(404);
      expect(await serverPageRequestStatus("GET", `${base}/archived-pages/`, session)).toBe(404);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-021"], "tab rides the URL while search, sort, and filters stay client-side"),
  { tag: specTags(["PAGE-021"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "listurls", session);
    try {
      const stamp = tag("url");
      const alpha = `${stamp} Alpha`;
      const beta = `${stamp} Beta`;
      const favId = await serverCreatePage(seed.workspaceSlug, projectId, session, alpha);
      await setupRetry("favorite page", () => serverFavoritePage(seed.workspaceSlug, projectId, favId, alpha, session));
      await serverCreatePage(seed.workspaceSlug, projectId, session, beta);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );

      const queryKeys = async (): Promise<string[]> => {
        const url = new URL(await driver.pagesGatingCurrentUrl());
        return [...url.searchParams.keys()].sort();
      };

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId, "?type=private");
      expect(await driver.pagesGatingActiveTab()).toBe("private");
      expect(await driver.pagesGatingRowTitles()).toEqual([`${stamp} private`]);
      await driver.pagesGatingClickTab("public");
      expect(await driver.pagesGatingQueryParam("type")).toBe("public");

      // Search narrows without touching the URL.
      await driver.pagesGatingSearchType("Alpha");
      expect(await driver.pagesGatingSearchValue()).toBe("Alpha");
      expect(await driver.pagesGatingRowTitles()).toEqual([alpha]);
      expect(await queryKeys()).toEqual(["type"]);

      // Sort reorders without touching the URL.
      const before = await driver.pagesGatingSortLabel();
      await driver.pagesGatingSortCycle();
      expect(await driver.pagesGatingSortLabel()).not.toBe(before);
      expect(await queryKeys()).toEqual(["type"]);
      // Restore the default ordering for the filter step below.
      await driver.pagesGatingSearchType("");

      // The favorites filter chips without touching the URL.
      await driver.pagesGatingFilterToggleFavorites();
      expect(await driver.pagesGatingChips()).not.toEqual([]);
      expect(await driver.pagesGatingRowTitles()).toEqual([alpha]);
      expect(await queryKeys()).toEqual(["type"]);

      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(rows.filter((row) => row.isFavorite).map(displayTitle)).toEqual([alpha]);
      expect(rows.length).toBe(3);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-021"], "client state resets on project switch and unknown tabs land on open"),
  { tag: specTags(["PAGE-021"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectA = await scratchProject(seed, "urlresetA", session);
    const projectB = await scratchProject(seed, "urlresetB", session);
    try {
      await serverCreatePage(seed.workspaceSlug, projectA, session, `${tag("resetA")} page`);
      await serverCreatePage(seed.workspaceSlug, projectB, session, `${tag("resetB")} page`);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectA);
      await driver.pagesGatingSearchType("resetA");
      expect(await driver.pagesGatingSearchValue()).toBe("resetA");

      // Switching projects drops the client-held search (and friends).
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectB);
      expect(await driver.pagesGatingSearchValue()).toBe(null);
      expect((await driver.pagesGatingRowTitles()).length).toBe(1);

      // Unknown tab values land on the open tab.
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectA, "?type=bogus");
      expect(await driver.pagesGatingActiveTab()).toBe("public");
      expect((await driver.pagesGatingRowTitles()).length).toBe(1);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectA);
      expect(await driver.pagesGatingActiveTab()).toBe("public");

      expect((await serverListPages(seed.workspaceSlug, projectA, session)).length).toBe(1);
      expect((await serverListPages(seed.workspaceSlug, projectB, session)).length).toBe(1);
    } finally {
      await cleanupProject(seed, projectA, session);
      await cleanupProject(seed, projectB, session);
    }
  }
);

test(
  specTitle(["PAGE-021"], "row links deep-link and quick-create navigates to the new id"),
  { tag: specTags(["PAGE-021"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "deeplinks", session);
    try {
      const name = `${tag("deep")} page`;
      const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);

      // Pasting a row URL lands on the page.
      const href = await driver.pagesGatingRowHref(name);
      expect(href).not.toBe(null);
      expect(href ?? "").toContain(`/pages/${pageId}`);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, pageId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingDocTitle()).toBe(name);

      // Quick-create navigates to the new id.
      await driver.pagesGatingOpenList(seed.workspaceSlug, projectId);
      const createdId = await driver.pagesGatingQuickCreate();
      expect(createdId).toMatch(/^[0-9a-fA-F-]{36}$/);
      expect(await driver.pagesGatingCurrentUrl()).toContain(`/pages/${createdId}`);

      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      const createdRow = rows.find((row) => row.id === createdId);
      expect(createdRow).not.toBe(undefined);
      // The detail record omits updater fields on fresh pages, so the
      // visibility half reads back through the list row instead.
      expect(createdRow?.access).toBe(0);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-022"], "detail loads one record with visit tracking, preloaded siblings, and a following title"),
  { tag: specTags(["PAGE-022"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "detailload", session);
    try {
      const stamp = tag("det");
      const first = `${stamp} first`;
      const second = `${stamp} second`;
      const firstId = await serverCreatePage(seed.workspaceSlug, projectId, session, first);
      await serverCreatePage(seed.workspaceSlug, projectId, session, second);
      // Pin updated_by before the UI opens: fresh pages carry a null updater
      // (visit tracking does not set it) and the record read requires one.
      // A description PATCH is the deterministic setter.
      expect(
        await serverPageRequestStatus(
          "PATCH",
          `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/pages/${firstId}/description/`,
          session,
          { description_html: "<p>detail draft</p>" }
        )
      ).toBe(200);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingStartRequestSpy();
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, firstId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");

      // One detail read carrying the visit-tracking flag, plus the
      // preloaded collection that feeds the sibling switcher.
      const detailGets = (await driver.pagesGatingLoggedRequests()).filter(
        (entry) => entry.method === "GET" && entry.path.endsWith(`/pages/${firstId}/`)
      );
      expect(detailGets.length).toBe(1);
      expect(new URLSearchParams(detailGets[0]?.query ?? "").get("track_visit")).toBe("true");
      const collectionGets = (await driver.pagesGatingLoggedRequests()).filter(
        (entry) => entry.method === "GET" && /\/pages\/$/.test(entry.path)
      );
      expect(collectionGets.length).toBeGreaterThanOrEqual(1);

      // The switcher lists the preloaded siblings and the tab title
      // follows the page name.
      expect(await driver.pagesGatingSwitcherOptions()).toEqual(expect.arrayContaining([first, second]));
      expect(await driver.pagesGatingDocTitle()).toBe(first);

      const record = await serverPageRecord(seed.workspaceSlug, projectId, firstId, session);
      expect(record.name).toBe(first);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-022"], "deleted records panel on direct load and hold viewers put behind a delete"),
  { tag: specTags(["PAGE-022"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "deletedload", session);
    try {
      const stamp = tag("del");
      const doomedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} doomed`);
      // serverDeletePage archives first itself (the API refuses live deletes).
      await setupRetry("delete doomed", () => serverDeletePage(seed.workspaceSlug, projectId, doomedId, session));
      const liveId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} live`);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      // Direct links to deleted ids yield the panel, never the editor, and
      // the URL stays on the record (no bounce to the list).
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, doomedId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("panel");
      expect(await driver.pagesGatingErrorPanelVisible()).toBe(true);
      expect(await driver.pagesGatingEditableCount()).toBe(0);
      expect(await driver.pagesGatingCurrentUrl()).toContain(doomedId);

      // A viewer with the page open stays put when it is deleted behind
      // them (no live push arrives without the live server).
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, liveId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      await setupRetry("delete live", () => serverDeletePage(seed.workspaceSlug, projectId, liveId, session));
      await new Promise((resolve) => setTimeout(resolve, 15_000));
      expect(await driver.pagesGatingCurrentUrl()).toContain(liveId);
      expect(await driver.pagesGatingErrorPanelVisible()).toBe(false);

      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      expect(await serverPageRequestStatus("GET", `${base}/pages/${doomedId}/`, session)).toBe(404);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${liveId}/`, session)).toBe(404);
      expect((await serverListPages(seed.workspaceSlug, projectId, session, true)).length).toBe(0);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-023"], "forbidden and missing ids panel with a path back while unresolved records spin"),
  { tag: specTags(["PAGE-023"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "errorpanel", session);
    try {
      const stamp = tag("err");
      const openName = `${stamp} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const privId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} private`);
      await setupRetry("set access private", () =>
        serverSetPageAccess(seed.workspaceSlug, projectId, privId, 1, session)
      );
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );

      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);

      // Forbidden ids yield the explanatory panel, never the editor.
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, privId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("panel");
      const panel = await driver.pagesGatingErrorPanel();
      expect(panel.heading).toBe("Page not found");
      expect(panel.body ?? "").toContain("permission");
      expect(panel.backHref).not.toBe(null);
      expect(panel.backHref ?? "").toContain(`/projects/${projectId}/pages`);

      // Missing ids panel identically.
      const missingId = "00000000-0000-0000-0000-000000000000";
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, missingId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("panel");
      expect((await driver.pagesGatingErrorPanel()).heading).toBe("Page not found");
      expect(await driver.pagesGatingEditableCount()).toBe(0);

      // An unresolved record shows the centered spinner until it resolves.
      // Both reads stall: the shell renders from preloaded list rows without
      // waiting for the detail fetch (see PAGE-079), so a detail-only stall
      // races the collection response.
      await driver.pagesGatingStallListFetch();
      await driver.pagesGatingStallDetailFetch(openId);
      try {
        await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
        await expect.poll(async () => driver.pagesGatingDetailSpinnerVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.pagesGatingErrorPanelVisible()).toBe(false);
      } finally {
        await driver.pagesGatingReleaseDetailFetch();
        await driver.pagesGatingReleaseListFetch();
      }
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingDetailSpinnerVisible()).toBe(false);

      const base = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}`;
      const memberSession = await signInSessionWithRetry(mention.email, mention.password);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${privId}/`, memberSession)).toBe(403);
      expect(await serverPageRequestStatus("GET", `${base}/pages/${missingId}/`, memberSession)).toBe(404);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-024"], "breadcrumb cluster carries project, back link, switcher, sync, and actions"),
  { tag: specTags(["PAGE-024"]) },
  async ({ driver, seed }) => {
    test.slow();
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "crumbs", session);
    try {
      const stamp = tag("cru");
      const first = `${stamp} first`;
      const second = `${stamp} second`;
      const firstId = await serverCreatePage(seed.workspaceSlug, projectId, session, first);
      await serverCreatePage(seed.workspaceSlug, projectId, session, second);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, firstId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");

      // Project context plus the back-to-list link.
      const project = await driver.pagesGatingBreadcrumbProject();
      expect(project).not.toBe(null);
      expect(project ?? "").toContain("NF314 crumbs");
      const backHref = await driver.pagesGatingBackLinkHref();
      expect(backHref).not.toBe(null);
      expect(backHref ?? "").toContain(`/projects/${projectId}/pages`);

      // Searchable sibling switcher that navigates on select.
      expect(await driver.pagesGatingSwitcherOptions()).toEqual(expect.arrayContaining([first, second]));
      expect(await driver.pagesGatingSwitcherSearch("second")).toEqual([second]);
      await driver.pagesGatingSwitcherSelect(second);
      await expect.poll(async () => driver.pagesGatingDocTitle(), { timeout: 30_000 }).toBe(second);

      // Sync indicator plus the header action cluster. The badge is a
      // transitional/fault surface only, so a live stack (the gate runs one
      // for collab-live) reports nothing on a converged editor. Drive the
      // fault the way PAGE-061 does — refuse the collaboration socket and
      // reload so the initial dial is refused — which reaches the same
      // terminal badge on either leg, in a stack with or without live.
      await driver.pagesLiveBlockCollaborationSocket();
      try {
        await driver.page.reload();
        await expect
          .poll(async () => (await driver.pagesGatingDetailCluster()).syncBadge, { timeout: 180_000 })
          .toBe("Connection lost");
        expect(await driver.pagesGatingDetailCluster()).toMatchObject({ star: true, copyLink: true, menu: true });
      } finally {
        await driver.pagesLiveUnblockCollaborationSocket();
      }

      const rows = await serverListPages(seed.workspaceSlug, projectId, session);
      expect(rows.map(displayTitle).sort()).toEqual([first, second].sort());
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-025"], "open pages read for role holders while locked and shelved render read-only"),
  { tag: specTags(["PAGE-025"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "readedit", session);
    try {
      const stamp = tag("rea");
      const openName = `${stamp} open`;
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, openName);
      const lockedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} locked`);
      await setupRetry("lock page", () => serverPageLock(seed.workspaceSlug, projectId, lockedId, session));
      const shelvedId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} shelved`);
      await setupRetry("archive page", () => serverArchivePage(seed.workspaceSlug, projectId, shelvedId, session));
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );

      // Owner: open pages edit; locked and shelved render read-only.
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      await expect.poll(async () => driver.pagesGatingEditableCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, lockedId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingEditableCount()).toBe(0);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, shelvedId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingEditableCount()).toBe(0);

      // Member: open pages edit; locked and shelved render read-only.
      await driver.pagesGatingSignOut();
      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      await expect.poll(async () => driver.pagesGatingEditableCount(), { timeout: 60_000 }).toBeGreaterThan(0);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, lockedId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingEditableCount()).toBe(0);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, shelvedId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingEditableCount()).toBe(0);

      const locked = await serverPageRecord(seed.workspaceSlug, projectId, lockedId, session);
      expect(locked.isLocked).toBe(true);
      const rows = await serverListPages(seed.workspaceSlug, projectId, session, true);
      expect(rows.find((row) => row.id === shelvedId)?.archivedAt).not.toBe(null);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-079"], "Detail loading spins, skeletons the body, and never collapses the title"),
  { tag: specTags(["PAGE-079"]) },
  async ({ driver, seed }) => {
    let session = await signInSessionWithRetry(seed.email, seed.password);
    const projectId = await scratchProject(seed, "detailload", session);
    try {
      const stamp = tag("ldg");
      const openId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} open`);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      // Body skeleton blocks while the document is still unknown on a cold
      // load. A live stack hands the document to the editor faster than a
      // polling sample can catch, so hold the document unknown the way
      // PAGE-067 does: refuse the collaboration socket before the
      // navigation, then let the normal dial through. Once the document is
      // known the skeleton never returns for this page in this session.
      await driver.pagesLiveBlockCollaborationSocket();
      try {
        await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, openId);
        await expect.poll(async () => driver.pagesGatingBodySkeletonVisible(), { timeout: 60_000 }).toBe(true);
      } finally {
        await driver.pagesLiveUnblockCollaborationSocket();
      }
      // A fresh page switch is the recovery the refused dial needs on both
      // legs (the old client never redials a refused initial connection).
      await driver.page.reload();
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingBodySkeletonVisible()).toBe(false);

      // The spinner target is minted after the cold load above so no earlier
      // read could have cached it.
      const untitledId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} blank`);
      // The stack 401s this burst-adjacent rename sporadically with the
      // session unknown server-side although sibling calls pass; a fresh
      // session recovers where a same-session retry does not.
      await setupRetry("blank the title", async () => {
        try {
          await serverRenamePage(seed.workspaceSlug, projectId, untitledId, "", session);
        } catch (error) {
          if (error instanceof Error && /HTTP 401/.test(error.message)) {
            session = await signInSessionWithRetry(seed.email, seed.password);
          }
          throw error;
        }
      });

      // Full-view spinner before the record resolves. Both reads stall: the
      // detail navigation also preloads the collection for the sibling
      // switcher, and the shell renders from those rows (switcher, title,
      // actions) without waiting for the held detail fetch — stalling only
      // the detail read polls flakily depending on which response lands first.
      await driver.pagesGatingStallListFetch();
      await driver.pagesGatingStallDetailFetch(untitledId);
      try {
        await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, untitledId);
        await expect.poll(async () => driver.pagesGatingDetailSpinnerVisible(), { timeout: 60_000 }).toBe(true);
      } finally {
        await driver.pagesGatingReleaseDetailFetch();
        await driver.pagesGatingReleaseListFetch();
      }
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");

      // Empty title space shows its placeholder and never collapses.
      const facts = await driver.pagesGatingTitleFacts(untitledId);
      expect(facts.text).toBe("");
      expect(facts.placeholder).toBe("Untitled");
      expect(facts.height).toBeGreaterThan(0);

      const blank = await serverPageRecord(seed.workspaceSlug, projectId, untitledId, session);
      expect(blank.name).toBe("");
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-079"], "version snapshots skeleton while their detail loads"),
  { tag: specTags(["PAGE-079"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "snapskel", session);
    try {
      const stamp = tag("snp");
      const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} page`);
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );
      const memberSession = await signInSessionWithRetry(mention.email, mention.password);
      // Distinct authors: same-author snapshots coalesce server-side.
      // Serialized on the worker: each snapshot reads current page state
      // at task time, so the second write waits for the first snapshot.
      await setupRetry("owner draft", () =>
        serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          `<p>${stamp} owner draft</p>`,
          tiptapParagraphs(`${stamp} owner draft`)
        )
      );
      await waitVersions(seed, projectId, pageId, session, 1);
      await setupRetry("member draft", () =>
        serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          `<p>${stamp} member draft</p>`,
          tiptapParagraphs(`${stamp} member draft`)
        )
      );
      const versions = await waitVersions(seed, projectId, pageId, session, 2);
      const versionId = versions[versions.length - 1]?.id ?? "";
      expect(versionId).not.toBe("");

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.pagesGatingStallVersionDetail();
      try {
        await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, pageId, `?paneTab=info&version=${versionId}`);
        await expect.poll(async () => driver.pagesGatingOverlaySkeletonVisible(), { timeout: 60_000 }).toBe(true);
      } finally {
        await driver.pagesGatingReleaseVersionDetail();
      }
      await expect.poll(async () => driver.pagesGatingOverlayOpen(), { timeout: 60_000 }).toBe(true);
      await expect.poll(async () => driver.pagesGatingOverlaySkeletonVisible(), { timeout: 60_000 }).toBe(false);
      expect(await driver.pagesGatingOverlayTitle()).not.toBe("");

      const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, versionId, session);
      expect(detail.id).toBe(versionId);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-080"], "tab-plus-version links open the same pane and snapshot on a fresh load"),
  { tag: specTags(["PAGE-080"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "deeplinks", session);
    try {
      const stamp = tag("lnk");
      const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, `${stamp} page`);
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );
      const memberSession = await signInSessionWithRetry(mention.email, mention.password);
      // Serialized on the worker (see above): the second write waits for
      // the first snapshot.
      await setupRetry("owner draft", () =>
        serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          session,
          `<p>${stamp} owner draft</p>`,
          tiptapParagraphs(`${stamp} owner draft`)
        )
      );
      await waitVersions(seed, projectId, pageId, session, 1);
      await setupRetry("member draft", () =>
        serverUpdatePageDescription(
          seed.workspaceSlug,
          projectId,
          pageId,
          memberSession,
          `<p>${stamp} member draft</p>`,
          tiptapParagraphs(`${stamp} member draft`)
        )
      );
      const versions = await waitVersions(seed, projectId, pageId, session, 2);
      // Resolve the keyed version by content (timeline order is newest-first).
      let versionId = "";
      let versionText = "";
      for (const version of versions) {
        const detail = await serverPageVersionDetail(seed.workspaceSlug, projectId, pageId, version.id, session);
        if (detail.html.includes(`${stamp} member draft`)) {
          versionId = version.id;
          versionText = `${stamp} member draft`;
          break;
        }
      }
      expect(versionId).not.toBe("");

      // Views as the member who wrote last: the live store coalesces into
      // the viewer's latest same-author snapshot, so viewing as the owner
      // would rewrite the owner's snapshot with the member's content.
      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);

      // A version link opens the overlay directly on load, on the keyed pane.
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, pageId, `?paneTab=info&version=${versionId}`);
      await expect.poll(async () => driver.pagesGatingOverlayOpen(), { timeout: 60_000 }).toBe(true);
      expect(await driver.pagesGatingPaneActiveTab()).toBe("Info");
      await expect.poll(async () => driver.pagesGatingOverlayText(), { timeout: 60_000 }).toContain(versionText);
      const sharedUrl = await driver.pagesGatingCurrentUrl();
      expect(new URL(sharedUrl).searchParams.get("version")).toBe(versionId);
      expect(new URL(sharedUrl).searchParams.get("paneTab")).toBe("info");

      // Sharing that URL reproduces the same view on a fresh load.
      const query = sharedUrl.slice(sharedUrl.indexOf("?"));
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, pageId, query);
      await expect.poll(async () => driver.pagesGatingOverlayOpen(), { timeout: 60_000 }).toBe(true);
      expect(await driver.pagesGatingPaneActiveTab()).toBe("Info");
      await expect.poll(async () => driver.pagesGatingOverlayText(), { timeout: 60_000 }).toContain(versionText);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);

test(
  specTitle(["PAGE-080"], "pane controls write the deep-link keys while the switcher and copy-link drop them"),
  { tag: specTags(["PAGE-080"]) },
  async ({ driver, seed }) => {
    const session = await signInSessionWithRetry(seed.email, seed.password);
    const mention = requireMentionMember(seed);
    const projectId = await scratchProject(seed, "keyround", session);
    try {
      const stamp = tag("key");
      const first = `${stamp} first`;
      const second = `${stamp} second`;
      const firstId = await serverCreatePage(seed.workspaceSlug, projectId, session, first);
      await serverCreatePage(seed.workspaceSlug, projectId, session, second);
      await setupRetry("add member", () =>
        serverAddProjectMembers(
          seed.workspaceSlug,
          projectId,
          [{ memberId: mention.id, role: PROJECT_MEMBER_ROLE }],
          session
        )
      );
      const memberSession = await signInSessionWithRetry(mention.email, mention.password);
      // Serialized on the worker (see above): the second write waits for
      // the first snapshot.
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        firstId,
        session,
        `<p>${stamp} owner draft</p>`,
        tiptapParagraphs(`${stamp} owner draft`)
      );
      await waitVersions(seed, projectId, firstId, session, 1);
      await serverUpdatePageDescription(
        seed.workspaceSlug,
        projectId,
        firstId,
        memberSession,
        `<p>${stamp} member draft</p>`,
        tiptapParagraphs(`${stamp} member draft`)
      );
      const versions = await waitVersions(seed, projectId, firstId, session, 2);
      const versionId = versions[0]?.id ?? "";
      expect(versionId).not.toBe("");
      const versionDetail = await serverPageVersionDetail(seed.workspaceSlug, projectId, firstId, versionId, session);

      // Views as the member who wrote last (see above): viewing as the
      // owner would rewrite the owner's snapshot.
      await driver.openEntry();
      await driver.signInWithPassword(mention.email, mention.password);
      await driver.pagesGatingOpenDetail(seed.workspaceSlug, projectId, firstId);
      expect(await driver.pagesGatingWaitDetailSettled()).toBe("editor");
      expect(await driver.pagesGatingPaneActiveTab()).toBe(null);

      // The history shortcut writes the pane key.
      await driver.pagesGatingClickOptionsItem("Version history");
      await expect.poll(async () => driver.pagesGatingQueryParam("paneTab"), { timeout: 60_000 }).toBe("info");
      await expect.poll(async () => driver.pagesGatingPaneActiveTab(), { timeout: 30_000 }).toBe("Info");

      // The version timeline writes the version key and opens the overlay.
      await driver.pagesGatingClickVersionLink(versionId);
      expect(await driver.pagesGatingQueryParam("version")).toBe(versionId);
      expect(await driver.pagesGatingQueryParam("paneTab")).toBe("info");
      expect(await driver.pagesGatingOverlayTitle()).not.toBe("");

      // The copy-link control emits the canonical page address: it drops
      // the composed keys rather than echoing them.
      const copied = await driver.pagesGatingCopyLinkText();
      const origin = new URL(await driver.pagesGatingCurrentUrl()).origin;
      expect(copied).toBe(`${origin}/${seed.workspaceSlug}/projects/${projectId}/pages/${firstId}`);
      expect(copied).not.toContain("paneTab=");
      expect(copied).not.toContain("version=");

      // The sibling switcher resets both keys on navigation.
      await driver.pagesGatingSwitcherSelect(second);
      await expect.poll(async () => driver.pagesGatingDocTitle(), { timeout: 30_000 }).toBe(second);
      expect(await driver.pagesGatingQueryParam("version")).toBe(null);
      expect(await driver.pagesGatingQueryParam("paneTab")).toBe(null);

      expect(versionDetail.id).toBe(versionId);
    } finally {
      await cleanupProject(seed, projectId, session);
    }
  }
);
