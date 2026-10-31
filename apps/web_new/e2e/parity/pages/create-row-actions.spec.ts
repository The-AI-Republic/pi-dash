// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-313): pages header quick-create, the
// form-based creation dialog, the per-row favorite star, and the row
// overflow/context menu actions (new tab, copy link, duplicate,
// visibility, lock, shelve/restore, delete) plus the community move pin.
// Rows: PAGE-007–PAGE-017.
//
// List isolation comes from scratch projects created in-spec (the seed
// project is only used where seeded membership matters). Pages are
// created through the API with per-attempt unique names so oracle
// retries converge; behavior that needs a second viewer drives a peer
// context through the same driver interface.
import { test, expect } from "../fixtures";
import { createDriver } from "../drivers/index";
import type { Browser } from "@playwright/test";
import {
  requireMentionMember,
  serverCreatePage,
  serverCreateProject,
  serverDeletePage,
  serverEnsureProjectGuest,
  serverEnsureProjectMember,
  serverFavoritePage,
  serverMovePageStatus,
  serverPageAccessStatus,
  serverPageArchive,
  serverPagesRowEnableGuestViewAll,
  serverPagesRowFlags,
  serverPagesRowList,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Per-attempt unique marker so oracle retries never collide on names. */
function stamp(): string {
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1296)
    .toString(36)
    .padStart(2, "0")}`;
}

function listUrl(seed: ParitySeedFacts, projectId: string): string {
  return `/${seed.workspaceSlug}/projects/${projectId}/pages`;
}

function detailUrl(seed: ParitySeedFacts, projectId: string, pageId: string): string {
  // The app routes detail addresses with a trailing slash.
  return `/${seed.workspaceSlug}/projects/${projectId}/pages/${pageId}/`;
}

/** Canonical page address as the row actions share it (no trailing slash). */
function canonicalDetailUrl(seed: ParitySeedFacts, projectId: string, pageId: string): string {
  return `/${seed.workspaceSlug}/projects/${projectId}/pages/${pageId}`;
}

/** Detail landings differ by trailing slash between direct loads and
 * client-side navigation; comparisons ignore it. */
function bare(path: string): string {
  return path.replace(/\/$/, "");
}

async function ownerSession(seed: ParitySeedFacts): Promise<string> {
  return signInSession(seed.email, seed.password);
}

async function memberSession(seed: ParitySeedFacts): Promise<string> {
  const member = requireMentionMember(seed);
  return signInSession(member.email, member.password);
}

async function guestSession(seed: ParitySeedFacts): Promise<string> {
  if (!seed.guestEmail || !seed.guestPassword) throw new Error("[parity] seed carries no guest identity.");
  return signInSession(seed.guestEmail, seed.guestPassword);
}

/** Scratch project with the pages feature on (default) for list isolation. */
async function scratchProject(seed: ParitySeedFacts, session: string, tag: string): Promise<string> {
  const suffix = stamp();
  const identifier = `W${suffix.slice(-6).toUpperCase()}`;
  return serverCreateProject(seed.workspaceSlug, session, `Pages313 ${tag} ${suffix}`, identifier);
}

async function openPeer(
  browser: Browser,
  target: ParityDriver["target"],
  url: string,
  sessionCookie: string
): Promise<{ close: () => Promise<void>; driver: ParityDriver }> {
  const context = await browser.newContext();
  const peer = createDriver(await context.newPage(), target);
  await peer.openAuthenticated(url, sessionBrowserCookies(sessionCookie));
  return { close: () => context.close(), driver: peer };
}

test(
  specTitle(["PAGE-007"], "header quick-create makes a page instantly and lands on its detail"),
  { tag: specTags(["PAGE-007"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "quick");
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    expect(await driver.pagesCreateQuickVisible()).toBe(true);
    expect(await driver.pagesRowTitles()).toEqual([]);

    const pageId = await driver.pagesCreateQuick();
    expect(pageId).toMatch(/^[0-9a-f-]{36}$/);
    expect(bare(await driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));

    // No form was shown and the record exists, openly visible by default.
    const state = await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session);
    expect(state.access).toBe(0);
    expect(state.archivedAt).toBeNull();
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    expect((await driver.pagesRowTitles()).length).toBe(1);
  }
);

test(
  specTitle(["PAGE-007"], "quick-create inherits visibility from the active tab"),
  { tag: specTags(["PAGE-007"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "quicktab");
    const first = await serverCreatePage(seed.workspaceSlug, projectId, session, `Seeded ${stamp()}`);
    expect(first).toMatch(/^[0-9a-f-]{36}$/);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "restricted");
    expect(await driver.pagesRowActiveTab()).toBe("restricted");

    const pageId = await driver.pagesCreateQuick();
    const state = await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session);
    expect(state.access).toBe(1);

    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "restricted");
    expect((await driver.pagesRowTitles()).length).toBe(1);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "open");
    expect((await driver.pagesRowTitles()).length).toBe(1);
  }
);

test(
  specTitle(["PAGE-007"], "failed quick-create stays on the list with an error notice"),
  { tag: specTags(["PAGE-007"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "quickfail");
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    await driver.pagesCreateFailNextCreate(500);

    const before = await driver.currentUrlPath();
    await expect(driver.pagesCreateQuick()).rejects.toThrow();
    expect(await driver.currentUrlPath()).toBe(before);
    expect(await driver.pagesRowTitles()).toEqual([]);
    expect((await driver.toastText()) ?? "").toMatch(/could not be created|error/i);
    expect(await serverPagesRowList(seed.workspaceSlug, projectId, session)).toEqual([]);
  }
);

test(
  specTitle(
    ["PAGE-007", "PAGE-009"],
    "guests see no create control, no stars, and a reduced row menu"
  ),
  { tag: specTags(["PAGE-007", "PAGE-009"]) },
  async ({ driver, seed, browser }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "guestgate");
    await serverPagesRowEnableGuestViewAll(seed.workspaceSlug, projectId, session);
    if (!seed.guestEmail) throw new Error("[parity] seed carries no guest identity.");
    await serverEnsureProjectGuest(seed.workspaceSlug, projectId, seed.guestEmail, session);
    const name = `Guest Row ${stamp()}`;
    await serverCreatePage(seed.workspaceSlug, projectId, session, name);

    const guest = await guestSession(seed);
    const peer = await openPeer(browser, driver.target, listUrl(seed, projectId), guest);
    try {
      await peer.driver.pagesRowOpenList(seed.workspaceSlug, projectId);
      expect(await peer.driver.pagesCreateQuickVisible()).toBe(false);
      expect(await peer.driver.pagesRowTitles()).toEqual([name]);
      expect(await peer.driver.pagesRowStarVisible(name)).toBe(false);
      await peer.driver.pagesRowOpenMenu(name);
      expect(await peer.driver.pagesRowMenuItems()).toEqual(["Open in new tab", "Copy link"]);
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(
    ["PAGE-008"],
    "creation dialog validates the title, then creates with visibility and mark"
  ),
  { tag: specTags(["PAGE-008"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "dialog");
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    await driver.pagesCreateOpenDialog();
    expect(await driver.pagesCreateDialogVisible()).toBe(true);
    expect(await driver.pagesCreateDialogAccess()).toBe("open");

    // Over-length titles surface an inline block and cannot submit.
    await driver.pagesCreateDialogSetTitle("d".repeat(256));
    expect(await driver.pagesCreateDialogOverLengthVisible()).toBe(true);
    expect(await driver.pagesCreateDialogSubmitEnabled()).toBe(false);

    const name = `Dialog Page ${stamp()}`;
    await driver.pagesCreateDialogSetTitle(name);
    expect(await driver.pagesCreateDialogOverLengthVisible()).toBe(false);
    expect(await driver.pagesCreateDialogSubmitEnabled()).toBe(true);
    await driver.pagesCreateDialogSetAccess("restricted");
    await driver.pagesCreateDialogPickIconMark();
    expect(await driver.pagesCreateDialogHasMark()).toBe(true);

    const pageId = await driver.pagesCreateDialogSubmit();
    expect(bare(await driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));
    const state = await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session);
    expect(state.name).toBe(name);
    expect(state.access).toBe(1);
    expect(state.markInUse).toBe("icon");
  }
);

test(
  specTitle(["PAGE-008"], "empty dialog titles cannot submit and cancel discards the draft"),
  { tag: specTags(["PAGE-008"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "dialogcancel");
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    await driver.pagesCreateOpenDialog();

    // The required title blocks an empty submit (native validation).
    await expect(driver.pagesCreateDialogSubmit()).rejects.toThrow();
    expect(await driver.pagesCreateDialogVisible()).toBe(true);
    expect(await driver.currentUrlPath()).toBe(listUrl(seed, projectId));

    await driver.pagesCreateDialogSetTitle(`Discarded ${stamp()}`);
    await driver.pagesCreateDialogCancel();
    expect(await driver.pagesCreateDialogVisible()).toBe(false);
    expect(await driver.pagesRowTitles()).toEqual([]);
    expect(await serverPagesRowList(seed.workspaceSlug, projectId, session)).toEqual([]);
  }
);

test(
  specTitle(["PAGE-009"], "row star toggles the favorite state with notices"),
  { tag: specTags(["PAGE-009"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "star");
    const name = `Star Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    expect(await driver.pagesRowStarVisible(name)).toBe(true);
    expect(await driver.pagesRowStarSelected(name)).toBe(false);

    await driver.pagesRowToggleStar(name);
    expect(await driver.pagesRowStarSelected(name)).toBe(true);
    await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toMatch(/added to favorites/i);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).isFavorite).toBe(true);

    await driver.pagesRowToggleStar(name);
    expect(await driver.pagesRowStarSelected(name)).toBe(false);
    // The notice arrives a beat after the optimistic star flips, and the
    // previous notice is still showing until it does.
    await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toMatch(/removed from favorites/i);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).isFavorite).toBe(false);
  }
);

test(
  specTitle(["PAGE-009"], "favorites-only filter shows starred pages alone"),
  { tag: specTags(["PAGE-009"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "starfilter");
    const tag = stamp();
    const starred = `Starred ${tag}`;
    const plain = `Plain ${tag}`;
    const starredId = await serverCreatePage(seed.workspaceSlug, projectId, session, starred);
    await serverCreatePage(seed.workspaceSlug, projectId, session, plain);
    await serverFavoritePage(seed.workspaceSlug, projectId, starredId, starred, session);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    expect(await driver.pagesRowTitles()).toEqual(expect.arrayContaining([starred, plain]));

    await driver.pagesRowFilterFavoritesOnly();
    expect(await driver.pagesRowTitles()).toEqual([starred]);

    await driver.pagesRowClearFilters();
    expect(await driver.pagesRowTitles()).toEqual(expect.arrayContaining([starred, plain]));
  }
);

test(
  specTitle(["PAGE-009"], "the first favorite expands the favorites navigation section"),
  { tag: specTags(["PAGE-009"]) },
  async ({ driver, seed, browser }) => {
    const session = await ownerSession(seed);
    const name = `Member Star ${stamp()}`;
    await serverCreatePage(seed.workspaceSlug, seed.projectId, session, name);

    // The seeded member holds no favorites, so their navigation starts
    // with the favorites section collapsed.
    const memberSessionCookie = await memberSession(seed);
    const peer = await openPeer(browser, driver.target, listUrl(seed, seed.projectId), memberSessionCookie);
    try {
      await peer.driver.pagesRowOpenList(seed.workspaceSlug, seed.projectId);
      expect(await peer.driver.pagesRowFavoritesNavOpen()).toBe(false);
      expect(await peer.driver.pagesRowStarVisible(name)).toBe(true);
      await peer.driver.pagesRowToggleStar(name);
      expect(await peer.driver.pagesRowStarSelected(name)).toBe(true);
      await expect.poll(() => peer.driver.pagesRowFavoritesNavOpen(), { timeout: 30_000 }).toBe(true);
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-010"], "row menu opens the page in a fresh tab"),
  { tag: specTags(["PAGE-010"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "newtab");
    const name = `New Tab ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    const opened = await driver.pagesRowOpenInNewTab(name);
    expect(new URL(opened).pathname).toBe(canonicalDetailUrl(seed, projectId, pageId));

    // The opened address lands on the same page.
    await driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
    expect(bare(await driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));
  }
);

test(
  specTitle(["PAGE-011"], "row menu copies the canonical page address"),
  { tag: specTags(["PAGE-011"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "copylink");
    const name = `Copy Link ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowCopyLink(name);
    expect((await driver.toastText()) ?? "").toMatch(/copied to clipboard/i);
    const pasted = await driver.readClipboard();
    expect(new URL(pasted).pathname).toBe(canonicalDetailUrl(seed, projectId, pageId));

    // Pasting the address reopens the same page.
    await driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
    expect(bare(await driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));
  }
);

test(
  specTitle(["PAGE-012"], "row menu duplicates the page into a standalone copy"),
  { tag: specTags(["PAGE-012"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "duplicate");
    const name = `Duplicate Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowDuplicate(name);
    expect((await driver.toastText()) ?? "").toMatch(/duplicated successfully/i);

    // The client never adds the copy to its local store, so the row
    // appears once the list refetches.
    const copyName = `${name} (Copy)`;
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);
    expect(await driver.pagesRowTitles()).toEqual(expect.arrayContaining([name, copyName]));
    const rows = await serverPagesRowList(seed.workspaceSlug, projectId, session);
    const copy = rows.find((row) => row.id !== pageId);
    expect(copy?.name).toBe(copyName);
    // The copy stands alone: removing the source keeps it.
    await serverDeletePage(seed.workspaceSlug, projectId, pageId, session);
    const after = await serverPagesRowList(seed.workspaceSlug, projectId, session);
    expect(after.map((row) => row.name)).toEqual([copyName]);
  }
);

test(
  specTitle(["PAGE-013"], "visibility toggle migrates the row across tabs"),
  { tag: specTags(["PAGE-013"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "access");
    const name = `Toggle Access ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).toContain("Make private");
    await driver.pagesRowMenuClick("Make private");
    await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toMatch(/marked private/i);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).access).toBe(1);

    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "open");
    expect(await driver.pagesRowTitles()).toEqual([]);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "restricted");
    expect(await driver.pagesRowTitles()).toEqual([name]);

    await driver.pagesRowToggleAccess(name);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).access).toBe(0);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "open");
    expect(await driver.pagesRowTitles()).toEqual([name]);
  }
);

test(
  specTitle(
    ["PAGE-013"],
    "visibility toggle hides on shelved pages and the server refuses non-owners"
  ),
  { tag: specTags(["PAGE-013"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const member = requireMentionMember(seed);
    const projectId = await scratchProject(seed, session, "accessgate");
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const memberCookie = await memberSession(seed);
    const name = `Member Page ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, memberCookie, name);

    // Shelved rows carry no visibility toggle.
    await serverPageArchive(seed.workspaceSlug, projectId, pageId, memberCookie);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(memberCookie));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    await driver.pagesRowOpenMenu(name);
    const items = await driver.pagesRowMenuItems();
    expect(items).not.toContain("Make private");
    expect(items).not.toContain("Make public");

    // The client offers the toggle to admins, but the server only honors
    // the owner: a non-owner admin's switch is refused and rolls back.
    expect(await serverPageAccessStatus(seed.workspaceSlug, projectId, pageId, 1, session)).toBe(400);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, memberCookie)).access).toBe(0);
  }
);

test(
  specTitle(["PAGE-014"], "lock toggle refuses edits from others until unlocked"),
  { tag: specTags(["PAGE-014"]) },
  async ({ driver, seed, browser }) => {
    const session = await ownerSession(seed);
    const member = requireMentionMember(seed);
    const projectId = await scratchProject(seed, session, "lock");
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const name = `Lock Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).toContain("Lock");
    await driver.pagesRowMenuClick("Lock");
    await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toMatch(/locked successfully/i);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).isLocked).toBe(true);
    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).toContain("Unlock");

    // Others find the locked page read-only.
    const memberCookie = await memberSession(seed);
    const peer = await openPeer(browser, driver.target, detailUrl(seed, projectId, pageId), memberCookie);
    try {
      await peer.driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
      expect(await peer.driver.pagesRowDetailReadOnly()).toBe(true);
    } finally {
      await peer.close();
    }

    await driver.pagesRowToggleLock(name);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).isLocked).toBe(false);
    const peer2 = await openPeer(browser, driver.target, detailUrl(seed, projectId, pageId), memberCookie);
    try {
      await peer2.driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
      expect(await peer2.driver.pagesRowDetailReadOnly()).toBe(false);
    } finally {
      await peer2.close();
    }
  }
);

test(
  specTitle(["PAGE-015"], "shelve and restore migrate the row and drop favorites"),
  { tag: specTags(["PAGE-015"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "shelve");
    const name = `Shelve Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await serverFavoritePage(seed.workspaceSlug, projectId, pageId, name, session);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowToggleArchive(name);
    expect((await driver.toastText()) ?? "").toMatch(/archived successfully/i);
    const shelved = await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session);
    expect(shelved.archivedAt).not.toBeNull();
    expect(shelved.isFavorite).toBe(false);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "open");
    expect(await driver.pagesRowTitles()).toEqual([]);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    expect(await driver.pagesRowTitles()).toEqual([name]);

    await driver.pagesRowToggleArchive(name);
    expect((await driver.toastText()) ?? "").toMatch(/restored successfully/i);
    const restored = await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session);
    expect(restored.archivedAt).toBeNull();
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "open");
    expect(await driver.pagesRowTitles()).toEqual([name]);
  }
);

test(
  specTitle(["PAGE-016"], "delete confirmation names the shelved page and cancels cleanly"),
  { tag: specTags(["PAGE-016"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "delete");
    const name = `Delete Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    // Live rows offer no delete action at all.
    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).not.toContain("Delete");
    await driver.pagesRowToggleArchive(name);

    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).toContain("Delete");
    await driver.pagesRowMenuClick("Delete");
    await expect.poll(() => driver.pagesRowDeleteModalVisible(), { timeout: 15_000 }).toBe(true);
    const modalText = await driver.pagesRowDeleteModalText();
    expect(modalText).toContain(name);
    expect(modalText).toMatch(/permanently|cannot be undone/i);

    await driver.pagesRowDeleteModalCancel();
    expect(await driver.pagesRowDeleteModalVisible()).toBe(false);
    expect(await driver.pagesRowTitles()).toEqual([name]);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).name).toBe(name);
  }
);

test(
  specTitle(["PAGE-016"], "confirmed delete shows a busy state and removes the page"),
  { tag: specTags(["PAGE-016"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "deletebusy");
    const name = `Delete Busy ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await serverPageArchive(seed.workspaceSlug, projectId, pageId, session);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    await driver.pagesRowOpenMenu(name);
    await driver.pagesRowMenuClick("Delete");
    await expect.poll(() => driver.pagesRowDeleteModalVisible(), { timeout: 15_000 }).toBe(true);

    await driver.pagesRowSlowNextDelete(2000);
    const confirmed = driver.pagesRowDeleteModalConfirm();
    await expect.poll(() => driver.pagesRowDeleteConfirmBusy(), { timeout: 15_000 }).toBe(true);
    await confirmed;
    expect(await driver.pagesRowDeleteModalVisible()).toBe(false);
    expect((await driver.toastText()) ?? "").toMatch(/deleted successfully/i);
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    expect(await driver.pagesRowTitles()).toEqual([]);
    await expect(serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).rejects.toThrow();
  }
);

test(
  specTitle(["PAGE-016"], "delete from the detail view navigates back to the list"),
  { tag: specTags(["PAGE-016"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "deletedetail");
    const name = `Delete Detail ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await serverPageArchive(seed.workspaceSlug, projectId, pageId, session);
    // The list lands in history first: after the delete the detail goes
    // back to wherever the viewer came from.
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
    await driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);

    await driver.pagesRowDetailOpenOptions();
    expect(await driver.pagesRowMenuItems()).toContain("Delete");
    await driver.pagesRowMenuClick("Delete");
    await expect.poll(() => driver.pagesRowDeleteModalVisible(), { timeout: 15_000 }).toBe(true);
    await driver.pagesRowDeleteModalConfirm();
    await expect.poll(() => driver.currentUrlPath(), { timeout: 30_000 }).toBe(
      listUrl(seed, projectId)
    );
    expect(await serverPagesRowList(seed.workspaceSlug, projectId, session)).toEqual([]);
  }
);

test(
  specTitle(["PAGE-016"], "viewers of a deleted page meet the not-found view"),
  { tag: specTags(["PAGE-016"]) },
  async ({ driver, seed, browser }) => {
    const session = await ownerSession(seed);
    const member = requireMentionMember(seed);
    const projectId = await scratchProject(seed, session, "deleteviewer");
    await serverEnsureProjectMember(seed.workspaceSlug, projectId, member.email, session);
    const name = `Delete Viewer ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await serverPageArchive(seed.workspaceSlug, projectId, pageId, session);

    // A second viewer holds the detail open while the owner deletes.
    const memberCookie = await memberSession(seed);
    const peer = await openPeer(browser, driver.target, detailUrl(seed, projectId, pageId), memberCookie);
    try {
      await peer.driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
      expect(bare(await peer.driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));

      await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
      await driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
      await driver.pagesRowOpenMenu(name);
      await driver.pagesRowMenuClick("Delete");
      await driver.pagesRowDeleteModalConfirm();

      // Revisiting the deleted record keeps its address and renders the
      // not-found view (no automatic redirect); its recovery link
      // returns the viewer to the list, where the row is gone.
      await peer.driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
      expect(bare(await peer.driver.currentUrlPath())).toBe(bare(detailUrl(seed, projectId, pageId)));
      expect(await peer.driver.pagesRowDetailNotFoundVisible()).toBe(true);
      await peer.driver.pagesRowDetailNotFoundBackToList();
      // The recovery link lands on the list with a trailing slash.
      await expect.poll(() => peer.driver.currentUrlPath(), { timeout: 30_000 }).toBe(
        `${listUrl(seed, projectId)}/`
      );
      await peer.driver.pagesRowOpenList(seed.workspaceSlug, projectId, "shelved");
      expect(await peer.driver.pagesRowTitles()).toEqual([]);
    } finally {
      await peer.close();
    }
  }
);

test(
  specTitle(["PAGE-017"], "community tree carries no move affordance or endpoint"),
  { tag: specTags(["PAGE-017"]) },
  async ({ driver, seed }) => {
    const session = await ownerSession(seed);
    const projectId = await scratchProject(seed, session, "movepin");
    const targetId = await scratchProject(seed, session, "movetarget");
    const name = `Move Me ${stamp()}`;
    const pageId = await serverCreatePage(seed.workspaceSlug, projectId, session, name);
    await driver.openAuthenticated(listUrl(seed, projectId), sessionBrowserCookies(session));
    await driver.pagesRowOpenList(seed.workspaceSlug, projectId);

    await driver.pagesRowOpenMenu(name);
    expect(await driver.pagesRowMenuItems()).not.toContain("Move");
    await driver.pagesRowOpenDetail(seed.workspaceSlug, projectId, pageId);
    await driver.pagesRowDetailOpenOptions();
    expect(await driver.pagesRowMenuItems()).not.toContain("Move");
    expect(await driver.pagesRowDetailMoveVisible()).toBe(false);

    // The community backend carries no move route either.
    expect(await serverMovePageStatus(seed.workspaceSlug, projectId, pageId, targetId, session)).toBe(404);
    expect((await serverPagesRowFlags(seed.workspaceSlug, projectId, pageId, session)).name).toBe(name);
  }
);
